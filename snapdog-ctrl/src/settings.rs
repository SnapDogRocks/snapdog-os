// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Settings export/import with an explicit, bounded archive contract.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};

const DATA_DIR: &str = "/data";
pub const SETTINGS_UPLOAD_LIMIT_BYTES: usize = 10 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 128;
const MAX_ARCHIVE_ENTRY_BYTES: usize = 1024 * 1024;
const MAX_ARCHIVE_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const IMPORT_TRANSACTION_DIRECTORY: &str = ".snapdog-settings-import";
const IMPORT_JOURNAL_FILE: &str = "journal.json";
const IMPORT_JOURNAL_VERSION: u8 = 1;
const MAX_IMPORT_JOURNAL_BYTES: usize = 128 * 1024;

/// The complete committed settings allowlist. `SnapDog` runtime state, recovery
/// copies, candidates, and the operation journal are deliberately absent.
const STATIC_SETTINGS_FILES: &[&str] = &[
    "snapdog/ctrl.toml",
    "snapdog/snapdog.toml",
    "hostname",
    "systemd/network/10-ethernet.network",
    "systemd/network/15-ap.network",
    "systemd/network/20-wifi.network",
    "ssh/authorized_keys",
    "default/snapdog-client",
];

const SETTINGS_DIRECTORIES: &[&str] = &[
    "snapdog",
    "systemd",
    "systemd/network",
    "wpa_supplicant",
    "ssh",
    "default",
];

static IMPORT_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static SETTINGS_MUTATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Serializes every mutation of the files covered by the settings archive, and
/// consistent exports of that same set. The global lock order is:
///
/// 1. server-manager operation lease (when needed),
/// 2. settings mutation lease,
/// 3. controller-config lease (when needed).
///
/// Normal writers acquire only the suffix they need. Keeping this guard in the
/// settings module makes the archive contract the single source of truth for
/// serialization as well as for the file allowlist.
#[must_use]
pub struct SettingsMutationGuard {
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

pub async fn lock_settings_mutation() -> SettingsMutationGuard {
    SettingsMutationGuard {
        _guard: SETTINGS_MUTATION_LOCK.lock().await,
    }
}

/// Summary of an archive's contents for preview.
#[derive(Serialize)]
pub struct SettingsPreview {
    pub hostname: Option<String>,
    pub wifi_configured: bool,
    pub ssh_keys_present: bool,
    pub has_auth: bool,
    pub files: Vec<String>,
}

#[derive(Debug)]
enum ValidatedEntry {
    Directory(PathBuf),
    File { path: PathBuf, contents: Vec<u8> },
}

struct PreviousFile {
    path: PathBuf,
    contents: Option<Vec<u8>>,
    mode: u32,
}

struct PreviousDirectory {
    path: PathBuf,
    mode: Option<u32>,
}

struct ImportSnapshot {
    files: Vec<PreviousFile>,
    directories: Vec<PreviousDirectory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ImportPhase {
    Prepared,
    Applying,
    Applied,
    RollingBack,
    RolledBack,
}

#[derive(Debug, Serialize, Deserialize)]
struct ImportJournal {
    version: u8,
    phase: ImportPhase,
    files: Vec<JournalFile>,
    directories: Vec<JournalDirectory>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalFile {
    path: String,
    existed: bool,
    mode: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalDirectory {
    path: String,
    mode: Option<u32>,
}

/// A completed-on-disk settings import that has not crossed its reboot
/// boundary yet. Dropping it in a still-running controller restores the old
/// settings; an accepted reboot deliberately leaves the Applied WAL for boot
/// recovery to commit and clean.
#[must_use]
pub struct SettingsImportTransaction {
    data_dir: PathBuf,
    journal: ImportJournal,
    terminalized: bool,
}

impl ValidatedEntry {
    fn path(&self) -> &Path {
        match self {
            Self::Directory(path) | Self::File { path, .. } => path,
        }
    }
}

/// Create a tar.gz archive containing committed settings only.
pub fn export_settings(_guard: &SettingsMutationGuard) -> Result<Vec<u8>> {
    export_settings_from(Path::new(DATA_DIR))
}

fn export_settings_from(data_dir: &Path) -> Result<Vec<u8>> {
    ensure_real_data_root(data_dir)?;
    let paths = collect_export_paths(data_dir)?;
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut entry_count = 0_usize;
    let mut total_bytes = 0_usize;

    for relative in paths {
        let source = data_dir.join(&relative);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect {}", source.display()));
            }
        };
        verify_existing_parent_directories(data_dir, &relative)?;
        anyhow::ensure!(
            is_unlinked_regular_file(&metadata),
            "refusing non-regular settings file: {}",
            source.display()
        );
        let size = usize::try_from(metadata.len()).context("settings file is too large")?;
        enforce_archive_limits(entry_count + 1, size, total_bytes)?;
        entry_count += 1;
        total_bytes = total_bytes
            .checked_add(size)
            .context("settings archive size overflow")?;

        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::file());
        header.set_size(u64::try_from(size).context("settings file size overflow")?);
        header.set_mode(normalized_file_mode(&relative)?);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        let file = File::open(&source)
            .with_context(|| format!("failed to open settings file: {}", source.display()))?;
        archive
            .append_data(&mut header, &relative, file)
            .with_context(|| format!("failed to archive: {}", relative.display()))?;
    }

    let encoder = archive
        .into_inner()
        .context("failed to finalize settings archive")?;
    encoder.finish().context("failed to finish settings gzip")
}

fn collect_export_paths(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = STATIC_SETTINGS_FILES
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let wifi_directory = data_dir.join("wpa_supplicant");
    match fs::symlink_metadata(&wifi_directory) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "refusing unsafe settings directory: {}",
                wifi_directory.display()
            );
            for entry in fs::read_dir(&wifi_directory)
                .with_context(|| format!("failed to read {}", wifi_directory.display()))?
            {
                let entry = entry.context("failed to inspect Wi-Fi settings entry")?;
                let relative = Path::new("wpa_supplicant").join(entry.file_name());
                if is_allowed_wpa_file(&relative) {
                    paths.push(relative);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {}", wifi_directory.display()));
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Parse a tar.gz archive and return a preview without extracting.
pub fn preview_settings(data: &[u8]) -> Result<SettingsPreview> {
    let entries = read_validated_archive(data)?;
    let mut files = Vec::with_capacity(entries.len());
    let mut hostname = None;
    let mut wifi_configured = false;
    let mut ssh_keys_present = false;
    let mut has_auth = false;

    for entry in &entries {
        let path = entry.path();
        files.push(path.to_string_lossy().into_owned());
        let ValidatedEntry::File { contents, .. } = entry else {
            continue;
        };
        let text = String::from_utf8_lossy(contents);
        if path == Path::new("hostname") {
            hostname = Some(text.trim().to_string());
        } else if is_allowed_wpa_file(path) {
            wifi_configured |= text.contains("network=");
        } else if path == Path::new("ssh/authorized_keys") {
            ssh_keys_present = true;
        } else if path == Path::new("snapdog/ctrl.toml") {
            has_auth = text
                .parse::<toml_edit::DocumentMut>()
                .ok()
                .and_then(|document| {
                    document
                        .get("auth")?
                        .get("password_hash")?
                        .as_str()
                        .map(str::to_owned)
                })
                .is_some();
        }
    }

    Ok(SettingsPreview {
        hostname,
        wifi_configured,
        ssh_keys_present,
        has_auth,
        files,
    })
}

/// Return the already validated server source, if the archive replaces it.
/// The manager runs the installed `SnapDog` guard against this exact source
/// while holding its operation lock and before stopping the live service.
pub fn validated_server_config_source(data: &[u8]) -> Result<Option<String>> {
    let entries = read_validated_archive(data)?;
    entries
        .into_iter()
        .find_map(|entry| match entry {
            ValidatedEntry::File { path, contents }
                if path == Path::new("snapdog/snapdog.toml") =>
            {
                Some(String::from_utf8(contents).context("snapdog.toml is not valid UTF-8"))
            }
            _ => None,
        })
        .transpose()
}

pub fn begin_settings_import(
    data: &[u8],
    _guard: &SettingsMutationGuard,
) -> Result<SettingsImportTransaction> {
    begin_settings_import_into(data, Path::new(DATA_DIR))
}

/// Resolve a transaction left by a power loss before any settings consumer is
/// initialized. Incomplete application rolls back; a completely Applied set is
/// committed because every target rename and directory fsync already finished.
pub fn recover_interrupted_import() -> Result<()> {
    recover_interrupted_import_from(Path::new(DATA_DIR))
}

/// Retry a rollback that the still-running controller has already chosen after
/// a reboot request failed or an import future was cancelled. Unlike boot
/// recovery, this deliberately rolls back even if the last durable phase is
/// still `Applied`: the runtime has not crossed the accepted-reboot boundary
/// and its in-memory auth/service state still belongs to the previous settings.
pub fn retry_pending_import_rollback() -> Result<()> {
    retry_pending_import_rollback_from(Path::new(DATA_DIR))
}

#[cfg(test)]
fn import_settings_into(data: &[u8], data_dir: &Path) -> Result<()> {
    begin_settings_import_into(data, data_dir)?.commit_now()
}

fn begin_settings_import_into(data: &[u8], data_dir: &Path) -> Result<SettingsImportTransaction> {
    let entries = read_validated_archive(data)?;
    ensure_real_data_root(data_dir)?;
    begin_import_validated_entries(&entries, data_dir, |_| Ok(()))
}

#[cfg(test)]
fn import_validated_entries<F>(
    entries: &[ValidatedEntry],
    data_dir: &Path,
    before_write: F,
) -> Result<()>
where
    F: FnMut(&Path) -> Result<()>,
{
    begin_import_validated_entries(entries, data_dir, before_write)?.commit_now()
}

fn begin_import_validated_entries<F>(
    entries: &[ValidatedEntry],
    data_dir: &Path,
    mut before_write: F,
) -> Result<SettingsImportTransaction>
where
    F: FnMut(&Path) -> Result<()>,
{
    recover_interrupted_import_from(data_dir)?;
    let snapshot = snapshot_import(entries, data_dir)?;
    let mut transaction = create_import_transaction(data_dir, &snapshot)?;
    transaction.set_phase(ImportPhase::Applying)?;
    let apply_result = (|| -> Result<()> {
        for directory in &snapshot.directories {
            ensure_directory_path(data_dir, &directory.path)?;
        }
        for entry in entries {
            if let ValidatedEntry::File { path, contents } = entry {
                before_write(path)?;
                write_normalized_file(data_dir, path, contents)?;
            }
        }
        Ok(())
    })();
    if let Err(import_error) = apply_result {
        if let Err(rollback_error) = transaction.rollback() {
            bail!(
                "settings import failed: {import_error:#}; restoring the previous settings also failed: {rollback_error:#}"
            );
        }
        return Err(import_error).context("settings import was rolled back");
    }
    sync_import_targets(data_dir, &snapshot)?;
    transaction.set_phase(ImportPhase::Applied)?;
    Ok(transaction)
}

fn snapshot_import(entries: &[ValidatedEntry], data_dir: &Path) -> Result<ImportSnapshot> {
    ensure_real_data_root(data_dir)?;
    let mut required_directories = HashSet::new();
    let mut files = Vec::new();
    let mut backup_bytes = 0_usize;
    for entry in entries {
        match entry {
            ValidatedEntry::Directory(path) => {
                required_directories.insert(path.clone());
            }
            ValidatedEntry::File { path, .. } => {
                let destination = data_dir.join(path);
                let (contents, mode) = match fs::symlink_metadata(&destination) {
                    Ok(metadata) => {
                        anyhow::ensure!(
                            is_unlinked_regular_file(&metadata),
                            "settings destination must be a regular file: {}",
                            destination.display()
                        );
                        let size = usize::try_from(metadata.len())
                            .context("existing settings file is too large")?;
                        anyhow::ensure!(
                            size <= MAX_ARCHIVE_ENTRY_BYTES,
                            "existing settings file is too large to back up safely: {}",
                            destination.display()
                        );
                        backup_bytes = backup_bytes
                            .checked_add(size)
                            .context("settings backup size overflow")?;
                        anyhow::ensure!(
                            backup_bytes <= MAX_ARCHIVE_TOTAL_BYTES,
                            "existing settings are too large to back up safely"
                        );
                        (
                            Some(fs::read(&destination).with_context(|| {
                                format!("failed to back up setting: {}", destination.display())
                            })?),
                            permission_mode(&metadata),
                        )
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        (None, normalized_file_mode(path)?)
                    }
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("failed to inspect setting: {}", destination.display())
                        });
                    }
                };
                files.push(PreviousFile {
                    path: path.clone(),
                    contents,
                    mode,
                });
                let mut parent = path.parent();
                while let Some(directory) = parent {
                    if directory.as_os_str().is_empty() {
                        break;
                    }
                    anyhow::ensure!(
                        is_allowed_settings_directory(directory),
                        "settings parent is not permitted: {}",
                        directory.display()
                    );
                    required_directories.insert(directory.to_path_buf());
                    parent = directory.parent();
                }
            }
        }
    }

    let mut paths = required_directories.into_iter().collect::<Vec<_>>();
    paths.sort_by_key(|path| path.components().count());
    let mut directories = Vec::with_capacity(paths.len());
    for path in paths {
        let destination = data_dir.join(&path);
        let mode = match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "settings parent must be a real directory: {}",
                    destination.display()
                );
                Some(permission_mode(&metadata))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect settings parent: {}",
                        destination.display()
                    )
                });
            }
        };
        directories.push(PreviousDirectory { path, mode });
    }
    Ok(ImportSnapshot { files, directories })
}

impl SettingsImportTransaction {
    fn set_phase(&mut self, phase: ImportPhase) -> Result<()> {
        let previous = self.journal.phase;
        self.journal.phase = phase;
        if let Err(error) = write_import_journal(&self.data_dir, &self.journal) {
            self.journal.phase = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Restore the complete pre-import snapshot. This is used when the reboot
    /// command fails while the old in-memory auth/runtime state is still live.
    pub fn rollback(mut self) -> Result<()> {
        let result = rollback_import_journal(&self.data_dir, &mut self.journal);
        if result.is_ok() {
            self.terminalized = true;
        }
        result
    }

    /// Leave the Applied WAL in place after systemd accepted the reboot. The
    /// next controller process commits it before loading any settings.
    pub fn leave_for_restart(mut self) {
        if self.journal.phase == ImportPhase::Applied {
            self.terminalized = true;
        } else {
            tracing::error!(
                phase = ?self.journal.phase,
                "refusing to commit an incomplete settings import"
            );
        }
    }

    #[cfg(test)]
    fn commit_now(mut self) -> Result<()> {
        anyhow::ensure!(
            self.journal.phase == ImportPhase::Applied,
            "settings import is not fully applied"
        );
        // Applied is already a durable commit decision. If cleanup fails, boot
        // recovery must finish it rather than rolling a complete import back.
        self.terminalized = true;
        cleanup_transaction_store(&self.data_dir)
    }
}

impl Drop for SettingsImportTransaction {
    fn drop(&mut self) {
        if self.terminalized {
            return;
        }
        if let Err(error) = rollback_import_journal(&self.data_dir, &mut self.journal) {
            tracing::error!(
                error = %error,
                "failed to roll back an abandoned settings import; boot recovery will retry"
            );
        } else {
            self.terminalized = true;
        }
    }
}

fn transaction_directory(data_dir: &Path) -> PathBuf {
    data_dir.join(IMPORT_TRANSACTION_DIRECTORY)
}

fn import_journal_path(data_dir: &Path) -> PathBuf {
    transaction_directory(data_dir).join(IMPORT_JOURNAL_FILE)
}

fn import_backup_path(data_dir: &Path, index: usize) -> PathBuf {
    transaction_directory(data_dir).join(format!("backup-{index:03}"))
}

fn create_import_transaction(
    data_dir: &Path,
    snapshot: &ImportSnapshot,
) -> Result<SettingsImportTransaction> {
    ensure_real_data_root(data_dir)?;
    let transaction_dir = transaction_directory(data_dir);
    fs::create_dir(&transaction_dir).with_context(|| {
        format!(
            "failed to create settings transaction directory: {}",
            transaction_dir.display()
        )
    })?;
    set_normalized_permissions(&transaction_dir, 0o700)?;
    sync_directory(data_dir)?;

    let journal = ImportJournal {
        version: IMPORT_JOURNAL_VERSION,
        phase: ImportPhase::Prepared,
        files: snapshot
            .files
            .iter()
            .map(|file| {
                Ok(JournalFile {
                    path: path_as_utf8(&file.path)?.to_string(),
                    existed: file.contents.is_some(),
                    mode: file.mode,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        directories: snapshot
            .directories
            .iter()
            .map(|directory| {
                Ok(JournalDirectory {
                    path: path_as_utf8(&directory.path)?.to_string(),
                    mode: directory.mode,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };

    let preparation = (|| -> Result<()> {
        for (index, file) in snapshot.files.iter().enumerate() {
            if let Some(contents) = file.contents.as_deref() {
                write_new_private_file(&import_backup_path(data_dir, index), contents)?;
            }
        }
        sync_directory(&transaction_dir)?;
        // This durable journal is the write-ahead boundary. No target file or
        // directory is changed until it and every referenced backup are synced.
        write_import_journal(data_dir, &journal)
    })();
    if let Err(error) = preparation {
        if let Err(cleanup_error) = cleanup_transaction_store(data_dir) {
            bail!(
                "failed to prepare settings transaction: {error:#}; cleanup also failed: {cleanup_error:#}"
            );
        }
        return Err(error).context("failed to prepare settings transaction");
    }

    Ok(SettingsImportTransaction {
        data_dir: data_dir.to_path_buf(),
        journal,
        terminalized: false,
    })
}

fn path_as_utf8(path: &Path) -> Result<&str> {
    path.to_str().context("settings path is not valid UTF-8")
}

fn write_new_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to create settings backup: {}", path.display()))?;
    set_normalized_permissions(path, 0o600)?;
    file.write_all(contents)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn write_import_journal(data_dir: &Path, journal: &ImportJournal) -> Result<()> {
    validate_import_journal(journal)?;
    let contents = serde_json::to_vec_pretty(journal).context("failed to encode import journal")?;
    anyhow::ensure!(
        contents.len() <= MAX_IMPORT_JOURNAL_BYTES,
        "settings import journal is too large"
    );
    let directory = transaction_directory(data_dir);
    let final_path = directory.join(IMPORT_JOURNAL_FILE);
    let sequence = IMPORT_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(
        ".{IMPORT_JOURNAL_FILE}.tmp.{}.{sequence}",
        std::process::id()
    ));
    write_new_private_file(&temporary, &contents)?;
    if let Err(error) = fs::rename(&temporary, &final_path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).context("failed to activate settings import journal");
    }
    sync_directory(&directory)
}

fn read_import_journal(data_dir: &Path) -> Result<ImportJournal> {
    let path = import_journal_path(data_dir);
    let metadata = fs::symlink_metadata(&path).with_context(|| {
        format!(
            "failed to inspect settings import journal: {}",
            path.display()
        )
    })?;
    anyhow::ensure!(
        is_unlinked_regular_file(&metadata),
        "settings import journal must be a single-link regular file"
    );
    let size = usize::try_from(metadata.len()).context("settings import journal is too large")?;
    anyhow::ensure!(
        size <= MAX_IMPORT_JOURNAL_BYTES,
        "settings import journal is too large"
    );
    let contents = fs::read(&path).context("failed to read settings import journal")?;
    let journal: ImportJournal =
        serde_json::from_slice(&contents).context("settings import journal is invalid")?;
    validate_import_journal(&journal)?;
    Ok(journal)
}

fn validate_import_journal(journal: &ImportJournal) -> Result<()> {
    anyhow::ensure!(
        journal.version == IMPORT_JOURNAL_VERSION,
        "unsupported settings import journal version"
    );
    anyhow::ensure!(
        journal.files.len() <= MAX_ARCHIVE_ENTRIES,
        "settings import journal has too many files"
    );
    anyhow::ensure!(
        journal.directories.len() <= SETTINGS_DIRECTORIES.len(),
        "settings import journal has too many directories"
    );
    let mut seen_files = HashSet::new();
    for file in &journal.files {
        let path = normalize_archive_path(Path::new(&file.path))?;
        anyhow::ensure!(
            path == Path::new(&file.path) && is_allowed_settings_file(&path),
            "settings import journal contains an invalid file path"
        );
        anyhow::ensure!(
            seen_files.insert(path),
            "settings import journal contains a duplicate file"
        );
        anyhow::ensure!(file.mode & !0o7777 == 0, "invalid saved file mode");
    }
    let mut seen_directories = HashSet::new();
    for directory in &journal.directories {
        let path = normalize_archive_path(Path::new(&directory.path))?;
        anyhow::ensure!(
            path == Path::new(&directory.path) && is_allowed_settings_directory(&path),
            "settings import journal contains an invalid directory path"
        );
        anyhow::ensure!(
            seen_directories.insert(path),
            "settings import journal contains a duplicate directory"
        );
        if let Some(mode) = directory.mode {
            anyhow::ensure!(mode & !0o7777 == 0, "invalid saved directory mode");
        }
    }
    Ok(())
}

fn recover_interrupted_import_from(data_dir: &Path) -> Result<()> {
    let transaction_dir = transaction_directory(data_dir);
    let transaction_metadata = match fs::symlink_metadata(&transaction_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) => metadata,
        Err(error) => return Err(error).context("failed to inspect settings transaction store"),
    };
    ensure_real_data_root(data_dir)?;
    anyhow::ensure!(
        transaction_metadata.is_dir() && !transaction_metadata.file_type().is_symlink(),
        "settings import transaction store must be a real directory"
    );
    match fs::symlink_metadata(import_journal_path(data_dir)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The journal is written only after every backup is durable and
            // before the first target mutation. A journal-less store is an
            // interrupted preparation and can be discarded safely.
            return cleanup_transaction_store(data_dir);
        }
        Ok(_) => {}
        Err(error) => return Err(error).context("failed to inspect settings import journal"),
    }
    let mut journal = read_import_journal(data_dir)?;
    match journal.phase {
        ImportPhase::Applied | ImportPhase::RolledBack => cleanup_transaction_store(data_dir),
        ImportPhase::Prepared | ImportPhase::Applying | ImportPhase::RollingBack => {
            rollback_import_journal(data_dir, &mut journal)
        }
    }
}

fn retry_pending_import_rollback_from(data_dir: &Path) -> Result<()> {
    let transaction_dir = transaction_directory(data_dir);
    let transaction_metadata = match fs::symlink_metadata(&transaction_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) => metadata,
        Err(error) => return Err(error).context("failed to inspect settings transaction store"),
    };
    ensure_real_data_root(data_dir)?;
    anyhow::ensure!(
        transaction_metadata.is_dir() && !transaction_metadata.file_type().is_symlink(),
        "settings import transaction store must be a real directory"
    );
    match fs::symlink_metadata(import_journal_path(data_dir)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return cleanup_transaction_store(data_dir);
        }
        Ok(_) => {}
        Err(error) => return Err(error).context("failed to inspect settings import journal"),
    }
    let mut journal = read_import_journal(data_dir)?;
    rollback_import_journal(data_dir, &mut journal)
}

fn rollback_import_journal(data_dir: &Path, journal: &mut ImportJournal) -> Result<()> {
    validate_import_journal(journal)?;
    if journal.phase == ImportPhase::RolledBack {
        return cleanup_transaction_store(data_dir);
    }
    if journal.phase != ImportPhase::RollingBack {
        journal.phase = ImportPhase::RollingBack;
        write_import_journal(data_dir, journal)?;
    }

    let mut failures = Vec::new();
    for (index, previous) in journal.files.iter().enumerate().rev() {
        let relative = Path::new(&previous.path);
        let destination = data_dir.join(relative);
        let restore_result = if previous.existed {
            read_import_backup(data_dir, index).and_then(|contents| {
                write_file_with_mode(data_dir, relative, &contents, previous.mode)
            })
        } else {
            remove_imported_file(&destination)
        };
        if let Err(error) = restore_result {
            failures.push(format!("{}: {error:#}", destination.display()));
        }
    }
    for previous in journal.directories.iter().rev() {
        let destination = data_dir.join(&previous.path);
        let restore_result = previous.mode.map_or_else(
            || match fs::remove_dir(&destination) {
                Ok(()) => destination
                    .parent()
                    .context("restored settings directory has no parent")
                    .and_then(sync_directory),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            },
            |mode| {
                set_normalized_permissions(&destination, mode)?;
                sync_directory(&destination)
            },
        );
        if let Err(error) = restore_result {
            failures.push(format!("{}: {error:#}", destination.display()));
        }
    }
    anyhow::ensure!(
        failures.is_empty(),
        "failed to restore settings: {}",
        failures.join("; ")
    );

    journal.phase = ImportPhase::RolledBack;
    write_import_journal(data_dir, journal)?;
    cleanup_transaction_store(data_dir)
}

fn read_import_backup(data_dir: &Path, index: usize) -> Result<Vec<u8>> {
    let path = import_backup_path(data_dir, index);
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("failed to inspect settings backup: {}", path.display()))?;
    anyhow::ensure!(
        is_unlinked_regular_file(&metadata),
        "settings backup must be a single-link regular file"
    );
    let size = usize::try_from(metadata.len()).context("settings backup is too large")?;
    anyhow::ensure!(
        size <= MAX_ARCHIVE_ENTRY_BYTES,
        "settings backup is too large"
    );
    fs::read(&path).with_context(|| format!("failed to read settings backup: {}", path.display()))
}

fn remove_imported_file(destination: &Path) -> Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_file() || metadata.file_type().is_symlink(),
            "imported setting has an unsafe file type: {}",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    fs::remove_file(destination)?;
    sync_directory(
        destination
            .parent()
            .context("imported setting has no parent")?,
    )
}

fn cleanup_transaction_store(data_dir: &Path) -> Result<()> {
    let directory = transaction_directory(data_dir);
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "settings import transaction store must be a real directory"
        ),
        Err(error) => return Err(error.into()),
    }
    for entry in fs::read_dir(&directory).context("failed to read settings transaction store")? {
        let entry = entry.context("failed to inspect settings transaction artifact")?;
        let metadata = fs::symlink_metadata(entry.path())?;
        anyhow::ensure!(
            !metadata.is_dir(),
            "settings transaction store contains an unexpected directory"
        );
        // remove_file unlinks symlinks/FIFOs themselves and never follows them.
        fs::remove_file(entry.path())?;
    }
    sync_directory(&directory)?;
    fs::remove_dir(&directory)?;
    sync_directory(data_dir)
}

fn sync_import_targets(data_dir: &Path, snapshot: &ImportSnapshot) -> Result<()> {
    let mut directories = HashSet::new();
    directories.insert(data_dir.to_path_buf());
    for directory in &snapshot.directories {
        let path = data_dir.join(&directory.path);
        directories.insert(path.clone());
        if let Some(parent) = path.parent() {
            directories.insert(parent.to_path_buf());
        }
    }
    for file in &snapshot.files {
        if let Some(parent) = data_dir.join(&file.path).parent() {
            directories.insert(parent.to_path_buf());
        }
    }
    for directory in directories {
        sync_directory(&directory)?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("failed to sync directory: {}", path.display()))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn permission_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o600
    }
}

fn is_unlinked_regular_file(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.nlink() == 1
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn read_validated_archive(data: &[u8]) -> Result<Vec<ValidatedEntry>> {
    validate_upload_size(data)?;
    let decoder = GzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(decoder);
    let mut validated = Vec::new();
    let mut seen = HashSet::new();
    let mut total_bytes = 0_usize;
    let mut file_count = 0_usize;

    // Process archive members exactly as encoded. In tar-rs' default processed
    // mode, GNU long-name/link and PAX extension bodies are read into an
    // unbounded Vec before the caller sees the following entry, bypassing our
    // per-entry and expanded-size limits. Raw mode exposes those headers here,
    // where the regular-file/directory allowlist rejects them before reading
    // their bodies.
    for raw_entry in archive
        .entries()
        .context("invalid tar.gz archive")?
        .raw(true)
    {
        let mut entry = raw_entry.context("corrupt archive entry")?;
        let path = normalize_archive_path(&entry.path().context("invalid archive path")?)?;
        anyhow::ensure!(
            seen.insert(path.clone()),
            "duplicate archive path: {}",
            path.display()
        );
        let entry_type = entry.header().entry_type();
        let is_file = entry_type.is_file();
        let is_directory = entry_type.is_dir();
        anyhow::ensure!(
            is_file || is_directory,
            "unsupported archive entry type for {}",
            path.display()
        );
        anyhow::ensure!(
            if is_file {
                is_allowed_settings_file(&path)
            } else {
                is_allowed_settings_directory(&path)
            },
            "settings path is not permitted: {}",
            path.display()
        );

        let size = usize::try_from(entry.size()).context("archive entry is too large")?;
        enforce_archive_limits(validated.len() + 1, size, total_bytes)?;
        total_bytes = total_bytes
            .checked_add(size)
            .context("expanded archive size overflow")?;
        if is_file {
            file_count += 1;
            let mut contents = Vec::with_capacity(size);
            entry
                .read_to_end(&mut contents)
                .with_context(|| format!("failed to read archive entry: {}", path.display()))?;
            anyhow::ensure!(
                contents.len() == size,
                "truncated archive entry: {}",
                path.display()
            );
            validated.push(ValidatedEntry::File { path, contents });
        } else {
            anyhow::ensure!(
                size == 0,
                "directory entry contains data: {}",
                path.display()
            );
            validated.push(ValidatedEntry::Directory(path));
        }
    }

    anyhow::ensure!(file_count > 0, "archive contains no settings files");
    validate_committed_config_entries(&validated)?;
    Ok(validated)
}

fn validate_committed_config_entries(entries: &[ValidatedEntry]) -> Result<()> {
    for entry in entries {
        let ValidatedEntry::File { path, contents } = entry else {
            continue;
        };
        if path == Path::new("snapdog/ctrl.toml") {
            let source = std::str::from_utf8(contents).context("ctrl.toml is not valid UTF-8")?;
            let document = source
                .parse::<toml_edit::DocumentMut>()
                .context("ctrl.toml contains invalid TOML")?;
            crate::system::validate_ctrl_document(&document)
                .context("ctrl.toml contains invalid controller settings")?;
        } else if path == Path::new("snapdog/snapdog.toml") {
            let source =
                std::str::from_utf8(contents).context("snapdog.toml is not valid UTF-8")?;
            let config = crate::server_config::parse_config_toml(source)
                .context("snapdog.toml could not be parsed")?;
            crate::server_config::validate(&config)
                .context("snapdog.toml contains invalid settings")?;
        }
    }
    Ok(())
}

fn enforce_archive_limits(
    entry_count: usize,
    entry_bytes: usize,
    total_bytes: usize,
) -> Result<()> {
    anyhow::ensure!(
        entry_count <= MAX_ARCHIVE_ENTRIES,
        "archive has too many entries (max {MAX_ARCHIVE_ENTRIES})"
    );
    anyhow::ensure!(
        entry_bytes <= MAX_ARCHIVE_ENTRY_BYTES,
        "archive entry is too large (max {MAX_ARCHIVE_ENTRY_BYTES} bytes)"
    );
    let expanded = total_bytes
        .checked_add(entry_bytes)
        .context("expanded archive size overflow")?;
    anyhow::ensure!(
        expanded <= MAX_ARCHIVE_TOTAL_BYTES,
        "expanded archive is too large (max {MAX_ARCHIVE_TOTAL_BYTES} bytes)"
    );
    Ok(())
}

fn validate_upload_size(data: &[u8]) -> Result<()> {
    if data.len() > SETTINGS_UPLOAD_LIMIT_BYTES {
        bail!("archive too large (max {SETTINGS_UPLOAD_LIMIT_BYTES} bytes)");
    }
    Ok(())
}

fn normalize_archive_path(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            bail!("unsafe archive path: {}", path.display());
        };
        component
            .to_str()
            .context("archive path is not valid UTF-8")?;
        normalized.push(component);
    }
    anyhow::ensure!(!normalized.as_os_str().is_empty(), "empty archive path");
    Ok(normalized)
}

fn is_allowed_settings_file(path: &Path) -> bool {
    STATIC_SETTINGS_FILES
        .iter()
        .any(|allowed| path == Path::new(allowed))
        || is_allowed_wpa_file(path)
}

fn is_allowed_wpa_file(path: &Path) -> bool {
    if path.parent() != Some(Path::new("wpa_supplicant")) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(interface) = name
        .strip_prefix("wpa_supplicant-")
        .and_then(|name| name.strip_suffix(".conf"))
    else {
        return false;
    };
    !interface.is_empty()
        && interface
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.".contains(character))
}

fn is_allowed_settings_directory(path: &Path) -> bool {
    SETTINGS_DIRECTORIES
        .iter()
        .any(|allowed| path == Path::new(allowed))
}

fn ensure_real_data_root(data_dir: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(data_dir).with_context(|| {
        format!(
            "settings data directory is unavailable: {}",
            data_dir.display()
        )
    })?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "settings data directory must be a real directory: {}",
        data_dir.display()
    );
    Ok(())
}

fn verify_existing_parent_directories(data_dir: &Path, relative: &Path) -> Result<()> {
    ensure_real_data_root(data_dir)?;
    let Some(parent) = relative.parent() else {
        return Ok(());
    };
    let mut current = data_dir.to_path_buf();
    for component in parent.components() {
        let Component::Normal(component) = component else {
            bail!("unsafe settings path: {}", relative.display());
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("settings parent is unavailable: {}", current.display()))?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "settings parent must be a real directory: {}",
            current.display()
        );
    }
    Ok(())
}

fn ensure_directory_path(data_dir: &Path, relative: &Path) -> Result<()> {
    ensure_real_data_root(data_dir)?;
    let mut current_path = data_dir.to_path_buf();
    let mut current_relative = PathBuf::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            bail!("unsafe settings directory: {}", relative.display());
        };
        current_path.push(component);
        current_relative.push(component);
        anyhow::ensure!(
            is_allowed_settings_directory(&current_relative),
            "settings directory is not permitted: {}",
            current_relative.display()
        );
        match fs::symlink_metadata(&current_path) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "settings parent must be a real directory: {}",
                current_path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current_path).with_context(|| {
                    format!(
                        "failed to create settings directory: {}",
                        current_path.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect settings directory: {}",
                        current_path.display()
                    )
                });
            }
        }
        set_normalized_permissions(&current_path, normalized_directory_mode(&current_relative)?)?;
    }
    Ok(())
}

fn write_normalized_file(data_dir: &Path, relative: &Path, contents: &[u8]) -> Result<()> {
    write_file_with_mode(
        data_dir,
        relative,
        contents,
        normalized_file_mode(relative)?,
    )
}

fn write_file_with_mode(
    data_dir: &Path,
    relative: &Path,
    contents: &[u8],
    mode: u32,
) -> Result<()> {
    let parent = relative
        .parent()
        .context("settings file has no parent directory")?;
    if !parent.as_os_str().is_empty() {
        ensure_directory_path(data_dir, parent)?;
    }
    let destination = data_dir.join(relative);
    let destination_parent = destination
        .parent()
        .context("settings destination has no parent")?;
    let file_name = destination
        .file_name()
        .context("settings destination has no filename")?
        .to_string_lossy();
    let (temporary, mut file) = loop {
        let sequence = IMPORT_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = destination_parent.join(format!(
            ".{file_name}.settings-import.{}.{sequence}",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        match options.open(&temporary) {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to stage imported setting: {}",
                        destination.display()
                    )
                });
            }
        }
    };

    let write_result = (|| -> Result<()> {
        set_normalized_permissions(&temporary, mode)?;
        file.write_all(contents)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| {
            format!(
                "failed to write imported setting: {}",
                destination.display()
            )
        });
    }
    if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| {
            format!(
                "failed to activate imported setting: {}",
                destination.display()
            )
        });
    }
    File::open(destination_parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| {
            format!(
                "failed to sync imported settings directory: {}",
                destination_parent.display()
            )
        })?;
    Ok(())
}

fn normalized_file_mode(path: &Path) -> Result<u32> {
    anyhow::ensure!(
        is_allowed_settings_file(path),
        "settings file is not permitted"
    );
    if path == Path::new("snapdog/ctrl.toml")
        || path == Path::new("ssh/authorized_keys")
        || is_allowed_wpa_file(path)
    {
        Ok(0o600)
    } else if path == Path::new("snapdog/snapdog.toml") {
        Ok(0o640)
    } else {
        Ok(0o644)
    }
}

fn normalized_directory_mode(path: &Path) -> Result<u32> {
    anyhow::ensure!(
        is_allowed_settings_directory(path),
        "settings directory is not permitted"
    );
    if path == Path::new("snapdog") {
        Ok(0o2750)
    } else if matches!(path.to_str(), Some("wpa_supplicant" | "ssh")) {
        Ok(0o700)
    } else {
        Ok(0o755)
    }
}

fn set_normalized_permissions(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("failed to set safe permissions on {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "snapdog-settings-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct TestArchiveEntry {
        path: String,
        entry_type: tar::EntryType,
        contents: Vec<u8>,
        mode: u32,
    }

    impl TestArchiveEntry {
        fn file(path: impl Into<String>, contents: impl Into<Vec<u8>>) -> Self {
            Self {
                path: path.into(),
                entry_type: tar::EntryType::file(),
                contents: contents.into(),
                mode: 0o777,
            }
        }

        fn directory(path: impl Into<String>) -> Self {
            Self {
                path: path.into(),
                entry_type: tar::EntryType::dir(),
                contents: Vec::new(),
                mode: 0o777,
            }
        }

        fn special(path: impl Into<String>, entry_type: tar::EntryType) -> Self {
            Self {
                path: path.into(),
                entry_type,
                contents: Vec::new(),
                mode: 0o777,
            }
        }
    }

    fn test_archive(entries: Vec<TestArchiveEntry>) -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for entry in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(entry.entry_type);
            header.set_size(u64::try_from(entry.contents.len()).unwrap());
            header.set_mode(entry.mode);
            archive
                .append_data(&mut header, entry.path, Cursor::new(entry.contents))
                .unwrap();
        }
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap()
    }

    fn write_test_file(root: &Path, relative: &str, contents: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[tokio::test]
    async fn parallel_writer_is_not_clobbered_by_import_rollback() {
        let settings_guard = lock_settings_mutation().await;
        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old-host\n");
        let archive = test_archive(vec![TestArchiveEntry::file(
            "hostname",
            b"imported-host\n".to_vec(),
        )]);
        let transaction = begin_settings_import_into(&archive, data.path()).unwrap();
        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"imported-host\n"
        );

        let writer_path = data.path().join("hostname");
        let (writer_started_tx, writer_started_rx) = tokio::sync::oneshot::channel();
        let writer = tokio::spawn(async move {
            writer_started_tx.send(()).unwrap();
            let _guard = lock_settings_mutation().await;
            fs::write(writer_path, b"parallel-writer\n").unwrap();
        });
        writer_started_rx.await.unwrap();

        // The importer owns the same lease as every normal writer. Rollback
        // therefore completes before the queued writer can enter.
        assert!(SETTINGS_MUTATION_LOCK.try_lock().is_err());
        transaction.rollback().unwrap();
        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"old-host\n"
        );

        drop(settings_guard);
        writer.await.unwrap();
        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"parallel-writer\n"
        );
        assert!(!transaction_directory(data.path()).exists());
    }

    #[test]
    fn export_contains_committed_settings_but_no_server_transaction_or_state() {
        let data = TestDirectory::new();
        write_test_file(
            data.path(),
            "snapdog/ctrl.toml",
            b"[services]\nserver = true\n",
        );
        write_test_file(data.path(), "snapdog/snapdog.toml", b"[[zone]]\nname='A'\n");
        write_test_file(data.path(), "snapdog/server-operation.json", b"{}");
        write_test_file(data.path(), "snapdog/.snapdog.toml.candidate", b"candidate");
        write_test_file(data.path(), "snapdog/.snapdog.toml.previous", b"previous");
        write_test_file(data.path(), "snapdog/snapdog.toml.last-good", b"last good");
        write_test_file(data.path(), "snapdog/server-last-issue.json", b"last issue");
        write_test_file(data.path(), "snapdog/state/server_id", b"runtime");

        let archive = export_settings_from(data.path()).unwrap();
        let paths = read_validated_archive(&archive)
            .unwrap()
            .into_iter()
            .map(|entry| entry.path().to_path_buf())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec![
                PathBuf::from("snapdog/ctrl.toml"),
                PathBuf::from("snapdog/snapdog.toml")
            ]
        );
    }

    #[test]
    fn import_rejects_every_transient_server_path() {
        for path in [
            "snapdog/server-operation.json",
            "snapdog/.snapdog.toml.candidate",
            "snapdog/.snapdog.toml.previous",
            "snapdog/snapdog.toml.last-good",
            "snapdog/server-last-issue.json",
            "snapdog/state/server_id",
        ] {
            let archive = test_archive(vec![TestArchiveEntry::file(path, b"secret".to_vec())]);
            let error = read_validated_archive(&archive).unwrap_err();
            assert!(
                error.to_string().contains("not permitted"),
                "{path}: {error}"
            );
        }
    }

    #[test]
    fn import_rejects_links_devices_fifos_and_contiguous_entries() {
        for entry_type in [
            tar::EntryType::symlink(),
            tar::EntryType::hard_link(),
            tar::EntryType::character_special(),
            tar::EntryType::block_special(),
            tar::EntryType::fifo(),
            tar::EntryType::contiguous(),
        ] {
            let archive = test_archive(vec![TestArchiveEntry::special("hostname", entry_type)]);
            let error = read_validated_archive(&archive).unwrap_err();
            assert!(error.to_string().contains("unsupported archive entry type"));
        }
    }

    #[test]
    fn import_rejects_extension_headers_before_expanding_their_contents() {
        let archive = test_archive(vec![TestArchiveEntry {
            path: "././@LongLink".into(),
            entry_type: tar::EntryType::new(b'L'),
            contents: vec![b'a'; MAX_ARCHIVE_ENTRY_BYTES + 1],
            mode: 0o600,
        }]);

        let error = read_validated_archive(&archive).unwrap_err();
        assert!(error.to_string().contains("unsupported archive entry type"));
    }

    #[test]
    fn import_rejects_duplicate_normalized_paths() {
        let archive = test_archive(vec![
            TestArchiveEntry::file("hostname", b"first".to_vec()),
            TestArchiveEntry::file("hostname", b"second".to_vec()),
        ]);
        let error = read_validated_archive(&archive).unwrap_err();
        assert!(error.to_string().contains("duplicate archive path"));
    }

    #[test]
    fn import_rejects_known_ctrl_sections_with_wrong_shapes_or_types() {
        for source in [
            "services = 'enabled'\n",
            "[services]\nserver = 'yes'\n",
            "[auto-update]\nenabled = 1\n",
            "[softap]\ncountry = false\n",
            "[auth]\npassword_hash = 42\n",
        ] {
            let archive = test_archive(vec![TestArchiveEntry::file(
                "snapdog/ctrl.toml",
                source.as_bytes().to_vec(),
            )]);
            let error = read_validated_archive(&archive).unwrap_err();
            assert!(
                format!("{error:#}").contains("invalid controller settings"),
                "{source}: {error:#}"
            );
        }
    }

    #[test]
    fn boot_recovery_rolls_back_a_power_loss_during_partial_application() {
        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old-host\n");
        write_test_file(
            data.path(),
            "snapdog/ctrl.toml",
            b"[services]\nserver = true\n",
        );
        let archive = test_archive(vec![
            TestArchiveEntry::file("hostname", b"new-host\n".to_vec()),
            TestArchiveEntry::file(
                "snapdog/ctrl.toml",
                b"[services]\nserver = false\n".to_vec(),
            ),
        ]);
        let entries = read_validated_archive(&archive).unwrap();
        let snapshot = snapshot_import(&entries, data.path()).unwrap();
        let mut transaction = create_import_transaction(data.path(), &snapshot).unwrap();
        transaction.set_phase(ImportPhase::Applying).unwrap();
        write_normalized_file(data.path(), Path::new("hostname"), b"new-host\n").unwrap();

        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"new-host\n"
        );
        assert_eq!(
            fs::read(data.path().join("snapdog/ctrl.toml")).unwrap(),
            b"[services]\nserver = true\n"
        );

        // Simulate process loss: Drop cannot run, so boot recovery must consume
        // only the durable journal and backups.
        std::mem::forget(transaction);
        recover_interrupted_import_from(data.path()).unwrap();

        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"old-host\n"
        );
        assert_eq!(
            fs::read(data.path().join("snapdog/ctrl.toml")).unwrap(),
            b"[services]\nserver = true\n"
        );
        assert!(!transaction_directory(data.path()).exists());
    }

    #[test]
    fn boot_recovery_commits_only_a_fully_applied_import() {
        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old-host\n");
        write_test_file(
            data.path(),
            "snapdog/ctrl.toml",
            b"[services]\nserver = true\n",
        );
        let archive = test_archive(vec![
            TestArchiveEntry::file("hostname", b"new-host\n".to_vec()),
            TestArchiveEntry::file(
                "snapdog/ctrl.toml",
                b"[services]\nserver = false\n".to_vec(),
            ),
        ]);

        let transaction = begin_settings_import_into(&archive, data.path()).unwrap();
        transaction.leave_for_restart();
        assert!(transaction_directory(data.path()).exists());

        recover_interrupted_import_from(data.path()).unwrap();

        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"new-host\n"
        );
        assert_eq!(
            fs::read(data.path().join("snapdog/ctrl.toml")).unwrap(),
            b"[services]\nserver = false\n"
        );
        assert!(!transaction_directory(data.path()).exists());
    }

    #[test]
    fn reboot_failure_and_abandoned_import_both_restore_the_complete_snapshot() {
        let archive = test_archive(vec![
            TestArchiveEntry::file("hostname", b"new-host\n".to_vec()),
            TestArchiveEntry::file(
                "snapdog/ctrl.toml",
                b"[services]\nserver = false\n".to_vec(),
            ),
        ]);
        for explicit_rollback in [true, false] {
            let data = TestDirectory::new();
            write_test_file(data.path(), "hostname", b"old-host\n");
            write_test_file(
                data.path(),
                "snapdog/ctrl.toml",
                b"[services]\nserver = true\n",
            );

            let transaction = begin_settings_import_into(&archive, data.path()).unwrap();
            if explicit_rollback {
                transaction.rollback().unwrap();
            } else {
                drop(transaction);
            }

            assert_eq!(
                fs::read(data.path().join("hostname")).unwrap(),
                b"old-host\n"
            );
            assert_eq!(
                fs::read(data.path().join("snapdog/ctrl.toml")).unwrap(),
                b"[services]\nserver = true\n"
            );
            assert!(!transaction_directory(data.path()).exists());
        }
    }

    #[test]
    fn failed_rollback_remains_durable_and_can_be_retried_before_consumers_run() {
        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old-host\n");
        let archive = test_archive(vec![TestArchiveEntry::file(
            "hostname",
            b"imported-host\n".to_vec(),
        )]);
        let transaction = begin_settings_import_into(&archive, data.path()).unwrap();
        let backup_path = import_backup_path(data.path(), 0);
        let backup = fs::read(&backup_path).unwrap();

        // A missing durable backup makes this rollback attempt fail in a fully
        // deterministic way. The WAL must remain RollingBack, never look
        // committed and never be silently discarded.
        fs::remove_file(&backup_path).unwrap();
        let error = transaction.rollback().unwrap_err();
        assert!(format!("{error:#}").contains("failed to restore settings"));
        assert_eq!(
            read_import_journal(data.path()).unwrap().phase,
            ImportPhase::RollingBack
        );
        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"imported-host\n"
        );

        // Model repair of the transient storage problem. Runtime recovery uses
        // this exact durable retry path while consumers remain locked out.
        fs::write(&backup_path, backup).unwrap();
        retry_pending_import_rollback_from(data.path()).unwrap();
        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"old-host\n"
        );
        assert!(!transaction_directory(data.path()).exists());
    }

    #[test]
    fn runtime_rollback_retry_overrides_an_applied_phase_before_reconciliation() {
        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old-host\n");
        let archive = test_archive(vec![TestArchiveEntry::file(
            "hostname",
            b"imported-host\n".to_vec(),
        )]);
        let transaction = begin_settings_import_into(&archive, data.path()).unwrap();
        assert_eq!(
            read_import_journal(data.path()).unwrap().phase,
            ImportPhase::Applied
        );

        // Simulate losing the in-memory transaction after the runtime chose
        // rollback but before it could persist RollingBack. Boot recovery must
        // commit Applied, while this explicit same-runtime API must roll it back.
        std::mem::forget(transaction);
        retry_pending_import_rollback_from(data.path()).unwrap();

        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"old-host\n"
        );
        assert!(!transaction_directory(data.path()).exists());
    }

    #[cfg(unix)]
    #[test]
    fn durable_import_artifacts_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old-host\n");
        let archive = test_archive(vec![TestArchiveEntry::file(
            "hostname",
            b"new-host\n".to_vec(),
        )]);

        let transaction = begin_settings_import_into(&archive, data.path()).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(&transaction_directory(data.path())), 0o700);
        assert_eq!(mode(&import_journal_path(data.path())), 0o600);
        assert_eq!(mode(&import_backup_path(data.path(), 0)), 0o600);
        transaction.rollback().unwrap();
    }

    #[test]
    fn failed_multi_file_import_restores_every_previous_file() {
        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old-host\n");
        write_test_file(
            data.path(),
            "snapdog/ctrl.toml",
            b"[services]\nserver = true\n",
        );
        let archive = test_archive(vec![
            TestArchiveEntry::file("hostname", b"new-host\n".to_vec()),
            TestArchiveEntry::file(
                "snapdog/ctrl.toml",
                b"[services]\nserver = false\n".to_vec(),
            ),
        ]);
        let entries = read_validated_archive(&archive).unwrap();
        let error = import_validated_entries(&entries, data.path(), |path| {
            if path == Path::new("snapdog/ctrl.toml") {
                bail!("injected write failure");
            }
            Ok(())
        })
        .unwrap_err();

        assert!(error.to_string().contains("rolled back"));
        assert_eq!(
            fs::read(data.path().join("hostname")).unwrap(),
            b"old-host\n"
        );
        assert_eq!(
            fs::read(data.path().join("snapdog/ctrl.toml")).unwrap(),
            b"[services]\nserver = true\n"
        );
    }

    #[test]
    fn import_refuses_an_unbounded_existing_backup() {
        let data = TestDirectory::new();
        write_test_file(
            data.path(),
            "hostname",
            &vec![b'x'; MAX_ARCHIVE_ENTRY_BYTES + 1],
        );
        let archive = test_archive(vec![TestArchiveEntry::file("hostname", b"new\n".to_vec())]);
        let error = import_settings_into(&archive, data.path()).unwrap_err();
        assert!(error.to_string().contains("too large to back up safely"));
    }

    #[cfg(unix)]
    #[test]
    fn export_and_import_refuse_existing_hard_link_aliases() {
        let data = TestDirectory::new();
        write_test_file(data.path(), "hostname", b"old\n");
        fs::hard_link(
            data.path().join("hostname"),
            data.path().join("hostname-alias"),
        )
        .unwrap();

        assert!(export_settings_from(data.path()).is_err());
        let archive = test_archive(vec![TestArchiveEntry::file("hostname", b"new\n".to_vec())]);
        let error = import_settings_into(&archive, data.path()).unwrap_err();
        assert!(error.to_string().contains("regular file"));
        assert_eq!(fs::read(data.path().join("hostname")).unwrap(), b"old\n");
    }

    #[test]
    fn import_enforces_entry_count_individual_and_expanded_size_limits() {
        let too_many = (0..=MAX_ARCHIVE_ENTRIES)
            .map(|index| {
                TestArchiveEntry::file(
                    format!("wpa_supplicant/wpa_supplicant-wlan{index}.conf"),
                    Vec::new(),
                )
            })
            .collect();
        let error = read_validated_archive(&test_archive(too_many)).unwrap_err();
        assert!(error.to_string().contains("too many entries"));

        let oversized = test_archive(vec![TestArchiveEntry::file(
            "hostname",
            vec![0; MAX_ARCHIVE_ENTRY_BYTES + 1],
        )]);
        let error = read_validated_archive(&oversized).unwrap_err();
        assert!(error.to_string().contains("entry is too large"));

        let expanded = (0..=MAX_ARCHIVE_TOTAL_BYTES / MAX_ARCHIVE_ENTRY_BYTES)
            .map(|index| {
                TestArchiveEntry::file(
                    format!("wpa_supplicant/wpa_supplicant-wlan{index}.conf"),
                    vec![0; MAX_ARCHIVE_ENTRY_BYTES],
                )
            })
            .collect();
        let error = read_validated_archive(&test_archive(expanded)).unwrap_err();
        assert!(error.to_string().contains("expanded archive is too large"));
    }

    #[cfg(unix)]
    #[test]
    fn import_ignores_archive_modes_and_applies_private_normalized_modes() {
        use std::os::unix::fs::PermissionsExt;

        let data = TestDirectory::new();
        let archive = test_archive(vec![
            TestArchiveEntry::directory("snapdog"),
            TestArchiveEntry::directory("wpa_supplicant"),
            TestArchiveEntry::file("snapdog/ctrl.toml", b"[auth]\n".to_vec()),
            TestArchiveEntry::file(
                "snapdog/snapdog.toml",
                b"[[zone]]\nname = \"Imported\"\n".to_vec(),
            ),
            TestArchiveEntry::file(
                "wpa_supplicant/wpa_supplicant-wlan0.conf",
                b"network={}\n".to_vec(),
            ),
        ]);

        import_settings_into(&archive, data.path()).unwrap();

        assert_eq!(
            fs::metadata(data.path().join("snapdog"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o2750
        );
        assert_eq!(
            fs::metadata(data.path().join("snapdog/ctrl.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
        assert_eq!(
            fs::metadata(data.path().join("snapdog/snapdog.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o640
        );
        assert_eq!(
            fs::metadata(data.path().join("wpa_supplicant/wpa_supplicant-wlan0.conf"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn import_refuses_an_existing_symlinked_parent() {
        use std::os::unix::fs::symlink;

        let data = TestDirectory::new();
        let outside = TestDirectory::new();
        symlink(outside.path(), data.path().join("snapdog")).unwrap();
        let archive = test_archive(vec![TestArchiveEntry::file(
            "snapdog/ctrl.toml",
            b"[auth]\n".to_vec(),
        )]);

        let error = import_settings_into(&archive, data.path()).unwrap_err();
        assert!(error.to_string().contains("must be a real directory"));
        assert!(!outside.path().join("ctrl.toml").exists());
    }

    #[cfg(unix)]
    #[test]
    fn boot_recovery_never_follows_a_symlinked_data_root() {
        use std::os::unix::fs::symlink;

        let container = TestDirectory::new();
        let outside = TestDirectory::new();
        fs::create_dir(outside.path().join(IMPORT_TRANSACTION_DIRECTORY)).unwrap();
        let linked_data = container.path().join("data-link");
        symlink(outside.path(), &linked_data).unwrap();

        let error = recover_interrupted_import_from(&linked_data).unwrap_err();
        assert!(error.to_string().contains("must be a real directory"));
        assert!(transaction_directory(outside.path()).exists());
    }
}
