// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Transactional `SnapDog` server lifecycle and recovery state.
//!
//! The manager is the only code allowed to combine desired-state persistence,
//! configuration activation, systemd control, and readiness verification. This
//! keeps "enabled" (intent), "active" (systemd), and "healthy" (`SnapDog` HTTP)
//! separate and serializes every mutating operation.

use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, OnceLock, RwLock as StdRwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::server_config::{self, ServerConfig};
use crate::system;

const SERVICE_NAME: &str = "snapdog.service";
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
// Longer than snapdog.service's RestartSec=5: a process that dies immediately
// must not be accepted merely because systemd has not restarted it yet.
const STABILITY_WINDOW: Duration = Duration::from_secs(6);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_DIAGNOSTIC_LINES: usize = 40;
const MAX_DIAGNOSTIC_LINE_CHARS: usize = 500;

static OPERATION_LOCK: Mutex<()> = Mutex::const_new(());
static ISSUE_PERSISTENCE_LOCK: Mutex<()> = Mutex::const_new(());
static RUNTIME_MEMORY: RwLock<RuntimeMemory> = RwLock::const_new(RuntimeMemory::new());
static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static RUNTIME_GENERATION: AtomicU64 = AtomicU64::new(1);
static BROADCASTER: OnceLock<broadcast::Sender<String>> = OnceLock::new();
static KNOWN_SECRETS: LazyLock<StdRwLock<Vec<String>>> =
    LazyLock::new(|| StdRwLock::new(Vec::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupState {
    NeedsSetup,
    Configured,
    NeedsRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    Stopped,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Stopped,
    Starting,
    Running,
    Restarting,
    Stopping,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Unknown,
    Checking,
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigState {
    Missing,
    ValidUnverified,
    Valid,
    Invalid,
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Setup,
    Apply,
    Start,
    Stop,
    Restart,
    Retry,
    Recover,
    SettingsImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Validating,
    Staging,
    Activating,
    Starting,
    Restarting,
    Stopping,
    Verifying,
    RollingBack,
    Recovering,
    Importing,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerOperation {
    pub id: String,
    pub kind: OperationKind,
    pub phase: OperationPhase,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerIssue {
    pub code: String,
    pub stage: String,
    pub summary: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub systemd_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollback_succeeded: Option<bool>,
}

impl ServerIssue {
    fn new(code: &str, stage: &str, summary: &str, detail: impl AsRef<str>) -> Self {
        Self {
            code: code.to_string(),
            stage: stage.to_string(),
            summary: summary.to_string(),
            detail: redact_text(detail.as_ref()),
            field_path: None,
            line: None,
            column: None,
            exit_code: None,
            systemd_result: None,
            rollback_succeeded: None,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ServerState {
    pub setup_state: SetupState,
    pub desired_state: DesiredState,
    pub runtime_state: RuntimeState,
    pub health_state: HealthState,
    pub config_state: ConfigState,
    pub active_revision: Option<String>,
    pub last_good_revision: Option<String>,
    pub endpoint: Option<String>,
    pub operation: Option<ServerOperation>,
    pub issue: Option<ServerIssue>,
    /// Legacy compatibility projection.
    pub enabled: bool,
    /// Legacy compatibility projection: this means systemd currently reports
    /// the process active; callers needing readiness use `health_state`.
    pub running: bool,
}

#[derive(Clone, Serialize)]
pub struct ServerConfigEnvelope {
    pub state: ConfigState,
    pub revision: String,
    pub raw_toml: String,
    pub config: Option<ServerConfig>,
    pub issues: Vec<ServerIssue>,
}

#[derive(Clone, Serialize)]
pub struct ValidationResponse {
    pub valid: bool,
    pub issues: Vec<ServerIssue>,
    pub config: Option<ServerConfig>,
}

#[derive(Clone, Serialize)]
pub struct ServerDiagnostics {
    pub generated_at: String,
    pub state: ServerState,
    pub systemd: ServiceSnapshot,
    pub journal: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ServiceSnapshot {
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub result: String,
    pub exec_main_code: Option<i32>,
    pub exec_main_status: Option<i32>,
    pub restart_count: Option<u64>,
    pub invocation_id: String,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub enum ConfigPayload {
    Direct(ServerConfig),
    Wrapped(WrappedConfigPayload),
}

#[derive(Clone, Deserialize)]
pub struct WrappedConfigPayload {
    pub revision: String,
    #[serde(default)]
    pub raw_toml: String,
    #[serde(default)]
    pub config: Option<ServerConfig>,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub enum SetupPayload {
    Direct(ServerConfig),
    Wrapped(WrappedSetupPayload),
}

#[derive(Clone, Deserialize)]
pub struct WrappedSetupPayload {
    pub config: ServerConfig,
    #[serde(default = "default_true")]
    pub start: bool,
}

const fn default_true() -> bool {
    true
}

impl ConfigPayload {
    pub(crate) fn into_config(self) -> ServerConfig {
        match self {
            Self::Direct(config) => config,
            Self::Wrapped(payload) => {
                if let Some(mut config) = payload.config {
                    config.revision = payload.revision;
                    config.raw_toml = payload.raw_toml;
                    config
                } else {
                    ServerConfig {
                        revision: payload.revision,
                        raw_toml: payload.raw_toml,
                        raw_toml_changed: true,
                        ..server_config::default_server_config()
                    }
                }
            }
        }
    }
}

impl SetupPayload {
    pub(crate) fn into_parts(self) -> (ServerConfig, bool) {
        match self {
            Self::Direct(config) => (config, true),
            Self::Wrapped(payload) => (payload.config, payload.start),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerErrorKind {
    Conflict,
    Invalid,
    Runtime,
    Internal,
}

#[derive(Debug, Clone)]
pub struct ManagerError {
    pub kind: ManagerErrorKind,
    pub issue: ServerIssue,
}

/// Holds the global server-operation lease from quiesce through reboot. The
/// route acquires settings mutation and then ctrl.toml only after this guard,
/// establishing the sole lock order: server operation, settings, ctrl config.
#[must_use]
pub struct SettingsImportGuard {
    lease: OperationLease,
}

/// Owns both the visible operation and the serialization lock. If the request
/// future disappears at any await point, `Drop` transfers the still-held lock
/// to an independent recovery task. A cancelled HTTP request can therefore
/// never strand an operation phase or expose a half-applied desired/runtime
/// state to the next mutation.
struct OperationLease {
    operation_guard: Option<tokio::sync::MutexGuard<'static, ()>>,
    operation: ServerOperation,
    terminalized: bool,
    runtime: tokio::runtime::Handle,
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.issue.summary, self.issue.detail)
    }
}

impl std::error::Error for ManagerError {}

#[derive(Clone)]
struct RuntimeMemory {
    operation: Option<ServerOperation>,
    issue: Option<ServerIssue>,
    active_revision: Option<String>,
    health_state: HealthState,
}

impl RuntimeMemory {
    const fn new() -> Self {
        Self {
            operation: None,
            issue: None,
            active_revision: None,
            health_state: HealthState::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionJournal {
    operation_id: String,
    kind: OperationKind,
    phase: OperationPhase,
    previous_existed: bool,
    previous_revision: Option<String>,
    candidate_revision: String,
    desired_running: bool,
    #[serde(default)]
    previous_desired_running: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct PersistedOperationIssue {
    recorded_at: String,
    issue: ServerIssue,
}

struct PreparedCandidate {
    previous: Option<String>,
    previous_revision: String,
    candidate: String,
    candidate_revision: String,
    parsed: ServerConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryDecision {
    ActivationNotApplied,
    CandidateNeedsVerification,
    CandidateCommitted,
    RollbackCompleted,
    Conflict,
}

enum RecoveryOutcome {
    Continue,
    Reconciled(Option<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciliationTarget {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancellationRecovery {
    ReconcileDesired,
    CompleteExplicitStop,
    RestoreSettingsThenReconcile,
}

/// Brackets the external reads used to build a state snapshot. Server
/// operations advance the generation before publishing a transition and again
/// when terminalizing it. A state-derived disk update is therefore safe only
/// when the complete observation saw one unchanged generation and no
/// authoritative in-memory operation or issue.
#[derive(Debug, Clone, Copy)]
struct RuntimeObservation {
    generation: u64,
}

impl RuntimeObservation {
    fn begin() -> Self {
        Self {
            generation: RUNTIME_GENERATION.load(Ordering::Acquire),
        }
    }

    const fn permits_state_issue_mutation(
        self,
        current_generation: u64,
        memory: &RuntimeMemory,
    ) -> bool {
        self.generation == current_generation
            && memory.operation.is_none()
            && memory.issue.is_none()
    }
}

/// Register the WebSocket event bus once during application startup.
/// Every lifecycle phase emits the same invalidation event; consumers then read
/// `/api/server/state` for the authoritative snapshot.
pub fn set_broadcaster(sender: broadcast::Sender<String>) {
    let _ = BROADCASTER.set(sender);
}

fn broadcast_change() {
    if let Some(sender) = BROADCASTER.get() {
        let _ = sender.send("server_changed".to_string());
    }
}

pub fn initial_config() -> ServerConfig {
    let mut config = server_config::default_server_config();
    config.revision = server_config::config_revision("");
    config.raw_toml.clear();
    config
}

pub fn setup_required_error() -> ManagerError {
    ManagerError {
        kind: ManagerErrorKind::Invalid,
        issue: ServerIssue::new(
            "setup_required",
            "configuration",
            "SnapDog must be configured before it can start",
            "Complete the server setup wizard first",
        ),
    }
}

pub async fn config_envelope() -> ServerConfigEnvelope {
    inspect_config(false).await
}

#[allow(clippy::too_many_lines)]
pub async fn server_state() -> ServerState {
    // Polling state must stay cheap and bounded. The external SnapDog guard runs
    // only for explicit validation/mutation paths.
    let runtime_observation = RuntimeObservation::begin();
    let envelope = inspect_config(false).await;
    let (enabled, desired_issue) = match system::service_desired_state("server").await {
        Ok(enabled) => (enabled, None),
        Err(error) => (
            false,
            Some(ServerIssue::new(
                "desired_state_unreadable",
                "configuration",
                "SnapDog's start preference cannot be read",
                error.to_string(),
            )),
        ),
    };
    let snapshot_result = service_snapshot().await;
    let (snapshot, snapshot_issue) = match snapshot_result {
        Ok(snapshot) => (snapshot, None),
        Err(error) => (
            ServiceSnapshot::default(),
            Some(ServerIssue::new(
                "systemd_unavailable",
                "status",
                "Server status is unavailable",
                error.to_string(),
            )),
        ),
    };
    let running = snapshot.active_state == "active";
    let (memory, runtime_generation) = {
        let memory = RUNTIME_MEMORY.read().await;
        (memory.clone(), RUNTIME_GENERATION.load(Ordering::Acquire))
    };
    let state_issue_mutation_allowed =
        runtime_observation.permits_state_issue_mutation(runtime_generation, &memory);

    let (health_state, probe_issue, endpoint) = if memory.operation.is_some() {
        (memory.health_state, None, None)
    } else if running {
        if let Some(active_revision) = memory.active_revision.as_deref() {
            if let Some(config) = config_for_verified_revision(active_revision, &envelope).await {
                match probe_ready(&config).await {
                    Ok(()) => (HealthState::Healthy, None, config_endpoint(&config)),
                    Err(detail) => (
                        HealthState::Unhealthy,
                        Some(ServerIssue::new(
                            "readiness_failed",
                            "health_check",
                            "SnapDog is running but not ready",
                            detail,
                        )),
                        None,
                    ),
                }
            } else {
                (
                    HealthState::Unknown,
                    Some(ServerIssue::new(
                        "active_config_unavailable",
                        "health_check",
                        "The running SnapDog configuration cannot be identified",
                        format!("verified revision {active_revision} is unavailable"),
                    )),
                    None,
                )
            }
        } else {
            (
                HealthState::Unknown,
                Some(ServerIssue::new(
                    "restart_required",
                    "health_check",
                    "SnapDog must be restarted before its health can be verified",
                    "The controller has not verified which configuration the running process loaded",
                )),
                None,
            )
        }
    } else {
        (HealthState::Unknown, None, None)
    };

    let runtime_state = memory.operation.as_ref().map_or_else(
        || runtime_from_snapshot(&snapshot, enabled),
        |operation| runtime_from_operation(operation.phase),
    );
    let setup_state = match envelope.state {
        ConfigState::Missing => SetupState::NeedsSetup,
        ConfigState::Invalid | ConfigState::Unreadable => SetupState::NeedsRepair,
        ConfigState::Valid | ConfigState::ValidUnverified => SetupState::Configured,
    };
    let active_revision = memory.active_revision.clone();
    let revision_issue = active_revision.as_ref().and_then(|active| {
        (envelope.state != ConfigState::Missing && envelope.revision != *active).then(|| {
            ServerIssue::new(
                "config_changed_since_start",
                "configuration",
                "The saved configuration is not loaded yet",
                format!(
                    "running revision {active}; saved revision {}",
                    envelope.revision
                ),
            )
        })
    });
    let operation_visible = memory.operation.is_some();
    let service_issue = service_failure_issue(&snapshot, enabled, operation_visible);
    let mut persisted_issue = read_persisted_issue().await;
    if state_issue_mutation_allowed {
        if let Some(current_issue) = service_issue
            .as_ref()
            .filter(|issue| issue.code == "service_not_running")
        {
            if !persisted_issue
                .as_ref()
                .is_some_and(|persisted| same_terminal_runtime_issue(persisted, current_issue))
            {
                match persist_runtime_issue_if_unchanged(current_issue, runtime_generation).await {
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(%error, "failed to persist SnapDog's stopped runtime issue");
                    }
                }
            }
            if persisted_issue.is_some() {
                persisted_issue = Some(current_issue.clone());
            }
        } else if (!enabled || running)
            && persisted_issue
                .as_ref()
                .is_some_and(|issue| issue.code == "service_not_running")
        {
            match clear_runtime_issue_if_unchanged(runtime_generation).await {
                Ok(true) => persisted_issue = None,
                Ok(false) => {}
                Err(error) => {
                    tracing::error!(%error, "failed to clear SnapDog's resolved stopped runtime issue");
                }
            }
        }
    }
    let persisted_issue = persisted_issue.filter(|issue| {
        !(operation_visible && issue.code == "service_not_running") && memory.issue.is_none()
    });
    let issue = envelope
        .issues
        .first()
        .cloned()
        .or(memory.issue)
        .or(service_issue)
        .or(persisted_issue)
        .or(revision_issue)
        .or(probe_issue)
        .or(snapshot_issue)
        .or(desired_issue);

    ServerState {
        setup_state,
        desired_state: if enabled {
            DesiredState::Running
        } else {
            DesiredState::Stopped
        },
        runtime_state,
        health_state,
        config_state: envelope.state,
        active_revision,
        last_good_revision: read_revision(server_config::CONFIG_LAST_GOOD).await,
        endpoint,
        operation: memory.operation,
        issue,
        enabled,
        running,
    }
}

pub async fn diagnostics() -> ServerDiagnostics {
    let state = server_state().await;
    let systemd = service_snapshot().await.unwrap_or_default();
    ServerDiagnostics {
        generated_at: Utc::now().to_rfc3339(),
        state,
        systemd,
        journal: journal_excerpt().await,
    }
}

async fn config_for_verified_revision(
    revision: &str,
    envelope: &ServerConfigEnvelope,
) -> Option<ServerConfig> {
    if envelope.revision == revision {
        return envelope.config.clone();
    }
    let source = tokio::fs::read_to_string(server_config::CONFIG_LAST_GOOD)
        .await
        .ok()?;
    (server_config::config_revision(&source) == revision)
        .then(|| server_config::parse_config_toml(&source).ok())
        .flatten()
}

pub async fn validate_config(
    payload: ConfigPayload,
) -> std::result::Result<ValidationResponse, ManagerError> {
    let _guard = OPERATION_LOCK.lock().await;
    ensure_validation_does_not_touch_recovery().await?;
    let config = payload.into_config();
    match prepare_candidate(config).await {
        Ok(prepared) => {
            remove_file_if_present(server_config::CONFIG_CANDIDATE).await;
            let _ = prepared;
            Ok(successful_validation_response())
        }
        Err(error) => {
            remove_file_if_present(server_config::CONFIG_CANDIDATE).await;
            if error.kind == ManagerErrorKind::Invalid {
                Ok(ValidationResponse {
                    valid: false,
                    issues: vec![error.issue],
                    config: None,
                })
            } else {
                Err(error)
            }
        }
    }
}

const fn successful_validation_response() -> ValidationResponse {
    // Validation must not replace the editor's baseline revision/raw source with
    // the candidate revision. The caller keeps its original draft for Apply.
    ValidationResponse {
        valid: true,
        issues: Vec::new(),
        config: None,
    }
}

pub async fn apply_config(
    payload: ConfigPayload,
) -> std::result::Result<ServerState, ManagerError> {
    let lease = OperationLease::acquire(OperationKind::Apply, OperationPhase::Recovering).await;
    if let Err(error) = recover_before_mutation().await {
        return lease.finish(Err(error)).await;
    }
    let desired_running = match read_desired_state().await {
        Ok(desired_running) => desired_running,
        Err(error) => return lease.finish(Err(error)).await,
    };
    let result = apply_inner(
        payload.into_config(),
        desired_running,
        desired_running,
        lease.operation(),
    )
    .await;
    lease.finish(result).await
}

pub async fn setup(payload: SetupPayload) -> std::result::Result<ServerState, ManagerError> {
    let lease = OperationLease::acquire(OperationKind::Setup, OperationPhase::Recovering).await;
    if let Err(error) = recover_before_mutation().await {
        return lease.finish(Err(error)).await;
    }
    let (config, start) = payload.into_parts();
    let previous_desired = match read_desired_state().await {
        Ok(previous_desired) => previous_desired,
        Err(error) => return lease.finish(Err(error)).await,
    };
    let result = apply_inner(config, start, previous_desired, lease.operation()).await;
    if let Err(error) = &result
        && should_restore_setup_desired(&error.issue)
        && let Err(restore_error) = system::set_service_desired("server", previous_desired).await
    {
        let original = error.issue.detail.clone();
        return lease
            .finish(Err(manager_error(
                ManagerErrorKind::Internal,
                "desired_state_restore_failed",
                "recovery",
                "Setup failed and its previous start preference could not be restored",
                format!("setup error: {original}; restore error: {restore_error:#}"),
            )))
            .await;
    }
    lease.finish(result).await
}

pub async fn start() -> std::result::Result<ServerState, ManagerError> {
    run_action(OperationKind::Start, "start").await
}

pub async fn restart() -> std::result::Result<ServerState, ManagerError> {
    run_action(OperationKind::Restart, "restart").await
}

pub async fn retry() -> std::result::Result<ServerState, ManagerError> {
    run_action(OperationKind::Retry, "retry").await
}

pub async fn stop() -> std::result::Result<ServerState, ManagerError> {
    let lease = OperationLease::acquire(OperationKind::Stop, OperationPhase::Stopping).await;
    let result = match stop_inner().await {
        Ok(()) => recover_after_explicit_stop().await,
        Err(error) => Err(error),
    };
    let result = result.map(|()| None);
    lease.finish(result).await
}

pub async fn prepare_settings_import(
    imported_server_source: Option<&str>,
) -> std::result::Result<SettingsImportGuard, ManagerError> {
    let lease =
        OperationLease::acquire(OperationKind::SettingsImport, OperationPhase::Recovering).await;
    if let Err(error) = recover_before_mutation().await {
        let terminal_error = error.clone();
        let _ = lease.finish(Err(error)).await;
        return Err(terminal_error);
    }
    set_operation_phase(OperationPhase::Validating).await;
    let preparation: std::result::Result<(), ManagerError> = async {
        if let Some(source) = imported_server_source {
            validate_settings_import_server_source(source).await?;
        }
        set_operation_phase(OperationPhase::Stopping).await;
        system::control_service("server", "stop")
            .await
            .map_err(|error| {
                manager_error(
                    ManagerErrorKind::Runtime,
                    "settings_import_stop_failed",
                    "stopping",
                    "SnapDog could not be quiesced for settings import",
                    error,
                )
            })?;
        wait_stopped().await?;
        set_operation_phase(OperationPhase::Importing).await;
        Ok(())
    }
    .await;
    if let Err(error) = preparation {
        let terminal_error = error.clone();
        let _ = lease.finish(Err(error)).await;
        return Err(terminal_error);
    }
    Ok(SettingsImportGuard { lease })
}

impl SettingsImportGuard {
    /// Restore the now-current committed settings after a transactional import
    /// failure. The ctrl-config lease must be dropped before calling this.
    #[allow(
        clippy::significant_drop_tightening,
        reason = "the server operation lock must cover terminal issue persistence"
    )]
    pub async fn abort(self, import_error: impl std::fmt::Display) -> bool {
        let lease = self.lease;
        set_operation_phase(OperationPhase::Recovering).await;
        let reconciliation = reconcile_inner(lease.operation()).await;
        let (issue, recovered) = match reconciliation {
            Ok(revision) => {
                record_reconciled_revision(revision.as_deref()).await;
                let mut issue = ServerIssue::new(
                    "settings_import_failed",
                    "settings_import",
                    "Settings could not be imported",
                    import_error.to_string(),
                );
                issue.rollback_succeeded = Some(true);
                (issue, true)
            }
            Err(recovery_error) => {
                let mut issue = ServerIssue::new(
                    "settings_import_recovery_failed",
                    "settings_import",
                    "Settings import failed and the server state could not be restored",
                    format!(
                        "import: {import_error}; recovery: {}",
                        recovery_error.issue.detail
                    ),
                );
                issue.rollback_succeeded = Some(false);
                (issue, false)
            }
        };
        let _ = lease
            .finish(Err(ManagerError {
                kind: ManagerErrorKind::Runtime,
                issue,
            }))
            .await;
        recovered
    }

    /// A successful reboot command is the terminal boundary: deliberately keep
    /// both this operation lease and its visible phase until the process exits.
    ///
    /// This is intentionally synchronous. Once systemd accepted the reboot,
    /// there must be no cancellation point before ownership of the operation
    /// lease is terminalized for process exit.
    pub fn hold_until_process_exit(self) {
        retain_until_process_exit(self);
        drop(tokio::spawn(async {
            if let Err(error) = clear_persisted_issue().await {
                tracing::error!(%error, "failed to clear the resolved server issue before reboot");
            }
        }));
    }

    /// Publish an actionable, durable fail-closed state while the settings WAL
    /// is still rolling back. The operation lease remains held, so no server
    /// action can start `SnapDog` from a partially restored settings set.
    pub async fn mark_rollback_pending(
        &self,
        reboot_error: impl std::fmt::Display,
        rollback_error: impl std::fmt::Display,
    ) {
        set_operation_phase(OperationPhase::RollingBack).await;
        let mut issue = ServerIssue::new(
            "settings_import_rollback_pending",
            "rollback",
            "Previous settings are still being restored",
            format!(
                "reboot: {reboot_error}; rollback: {rollback_error}. SnapDog remains stopped while recovery is retried."
            ),
        );
        issue.rollback_succeeded = Some(false);
        {
            let mut memory = RUNTIME_MEMORY.write().await;
            RUNTIME_GENERATION.fetch_add(1, Ordering::Release);
            memory.issue = Some(issue.clone());
            memory.health_state = HealthState::Unhealthy;
        }
        if let Err(error) = persist_operation_issue(&issue).await {
            tracing::error!(%error, "failed to persist pending settings rollback issue");
        }
        broadcast_change();
    }
}

/// Synchronous type boundary used after an external reboot request has been
/// accepted. Callers cannot accidentally insert an `.await` between accepting
/// that irreversible boundary and retaining the lease.
const fn retain_until_process_exit<T>(value: T) {
    std::mem::forget(value);
}

impl OperationLease {
    async fn acquire(kind: OperationKind, phase: OperationPhase) -> Self {
        let operation_guard = OPERATION_LOCK.lock().await;
        let runtime = tokio::runtime::Handle::current();
        let operation = begin_operation(kind, phase).await;
        Self {
            operation_guard: Some(operation_guard),
            operation,
            terminalized: false,
            runtime,
        }
    }

    const fn operation(&self) -> &ServerOperation {
        &self.operation
    }

    async fn finish(
        mut self,
        result: std::result::Result<Option<String>, ManagerError>,
    ) -> std::result::Result<ServerState, ManagerError> {
        let terminal_result = finish_result(result).await;
        self.terminalized = true;
        terminal_result
    }
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        if self.terminalized {
            return;
        }
        let Some(operation_guard) = self.operation_guard.take() else {
            return;
        };
        let operation = self.operation.clone();
        let runtime = self.runtime.clone();
        drop(runtime.spawn(recover_cancelled_operation(operation, operation_guard)));
    }
}

async fn recover_cancelled_operation(
    mut operation: ServerOperation,
    operation_guard: tokio::sync::MutexGuard<'static, ()>,
) {
    let interrupted_phase = {
        let mut memory = RUNTIME_MEMORY.write().await;
        // Publish the epoch while the write lock excludes state snapshots. A
        // stale poll then skips persistence even before this transition becomes
        // visible through `RUNTIME_MEMORY`.
        RUNTIME_GENERATION.fetch_add(1, Ordering::Release);
        let phase = memory
            .operation
            .as_ref()
            .filter(|visible| visible.id == operation.id)
            .map_or(operation.phase, |visible| visible.phase);
        operation.phase = OperationPhase::Recovering;
        memory.operation = Some(operation.clone());
        memory.issue = None;
        memory.health_state = HealthState::Checking;
        drop(memory);
        phase
    };
    broadcast_change();

    let reconciliation = reconcile_cancelled_operation(&operation).await;
    let retain_operation_lease = reconciliation
        .as_ref()
        .is_err_and(cancellation_recovery_requires_process_exit);
    let terminal_error = match reconciliation {
        Ok(revision) => {
            record_reconciled_revision(revision.as_deref()).await;
            interrupted_operation_error(&operation, interrupted_phase, None)
        }
        Err(recovery_error) if retain_operation_lease => recovery_error,
        Err(recovery_error) => {
            interrupted_operation_error(&operation, interrupted_phase, Some(&recovery_error))
        }
    };
    let _ = finish_result(Err(terminal_error)).await;
    // Keep the exact lease acquired by the cancelled request until recovery,
    // issue persistence, and the final state transition are complete.
    if retain_operation_lease {
        // The settings + ctrl leases were retained by the recovery helper.
        // Retain the server-operation lease too: no action may start SnapDog
        // from a partially restored archive before process restart recovery.
        std::mem::forget(operation_guard);
    } else {
        drop(operation_guard);
    }
}

fn cancellation_recovery_requires_process_exit(error: &ManagerError) -> bool {
    error.issue.code == "settings_import_rollback_pending"
}

async fn record_reconciled_revision(revision: Option<&str>) {
    if let Some(revision) = revision {
        remember_verified_revision(revision).await;
    } else {
        clear_verified_revision().await;
    }
}

async fn reconcile_cancelled_operation(
    operation: &ServerOperation,
) -> std::result::Result<Option<String>, ManagerError> {
    match cancellation_recovery(operation.kind) {
        CancellationRecovery::ReconcileDesired => reconcile_inner(operation).await,
        CancellationRecovery::CompleteExplicitStop => {
            stop_inner().await?;
            recover_after_explicit_stop().await?;
            Ok(None)
        }
        CancellationRecovery::RestoreSettingsThenReconcile => {
            let settings_guard = crate::settings::lock_settings_mutation().await;
            let ctrl_guard = system::lock_ctrl_config_for_settings_import().await;
            if let Err(error) = crate::settings::retry_pending_import_rollback() {
                let mut issue = ServerIssue::new(
                    "settings_import_rollback_pending",
                    "rollback",
                    "Previous settings could not be restored safely",
                    format!(
                        "cancelled settings import recovery failed: {error:#}. SnapDog and settings changes remain locked until the controller restarts."
                    ),
                );
                issue.rollback_succeeded = Some(false);
                std::mem::forget(ctrl_guard);
                std::mem::forget(settings_guard);
                return Err(ManagerError {
                    kind: ManagerErrorKind::Runtime,
                    issue,
                });
            }
            drop(ctrl_guard);
            drop(settings_guard);
            reconcile_inner(operation).await
        }
    }
}

const fn cancellation_recovery(kind: OperationKind) -> CancellationRecovery {
    match kind {
        OperationKind::Stop => CancellationRecovery::CompleteExplicitStop,
        OperationKind::SettingsImport => CancellationRecovery::RestoreSettingsThenReconcile,
        OperationKind::Setup
        | OperationKind::Apply
        | OperationKind::Start
        | OperationKind::Restart
        | OperationKind::Retry
        | OperationKind::Recover => CancellationRecovery::ReconcileDesired,
    }
}

fn interrupted_operation_error(
    operation: &ServerOperation,
    interrupted_phase: OperationPhase,
    recovery_error: Option<&ManagerError>,
) -> ManagerError {
    let kind = operation_kind_name(operation.kind);
    let phase = operation_phase_name(interrupted_phase);
    let (code, summary, detail, recovered) = recovery_error.map_or_else(
        || {
            (
                "operation_interrupted_recovered",
                "The interrupted server operation was recovered",
                format!(
                    "The {kind} request ended during {phase}. SnapDog was restored to its saved start preference; retry the operation if it is still needed."
                ),
                true,
            )
        },
        |error| {
            (
                "operation_interrupted_recovery_failed",
                "The server operation was interrupted and recovery failed",
                format!(
                    "The {kind} request ended during {phase}; runtime recovery failed: {}. Open diagnostics, correct the reported problem, then choose Retry or Stop.",
                    error.issue.detail
                ),
                false,
            )
        },
    );
    let mut issue = ServerIssue::new(code, "recovery", summary, detail);
    issue.rollback_succeeded = Some(recovered);
    ManagerError {
        kind: ManagerErrorKind::Runtime,
        issue,
    }
}

const fn operation_kind_name(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Setup => "setup",
        OperationKind::Apply => "apply",
        OperationKind::Start => "start",
        OperationKind::Stop => "stop",
        OperationKind::Restart => "restart",
        OperationKind::Retry => "retry",
        OperationKind::Recover => "recovery",
        OperationKind::SettingsImport => "settings import",
    }
}

const fn operation_phase_name(phase: OperationPhase) -> &'static str {
    match phase {
        OperationPhase::Validating => "validation",
        OperationPhase::Staging => "staging",
        OperationPhase::Activating => "activation",
        OperationPhase::Starting => "startup",
        OperationPhase::Restarting => "restart",
        OperationPhase::Stopping => "shutdown",
        OperationPhase::Verifying => "verification",
        OperationPhase::RollingBack => "rollback",
        OperationPhase::Recovering => "recovery",
        OperationPhase::Importing => "settings import",
    }
}

pub async fn reconcile_at_boot() {
    let _guard = OPERATION_LOCK.lock().await;
    let operation = begin_operation(OperationKind::Recover, OperationPhase::Recovering).await;
    let result = reconcile_inner(&operation).await;
    if let Err(error) = finish_result(result).await {
        tracing::error!(error = %error, "SnapDog server boot reconciliation failed");
    }
}

async fn run_action(
    kind: OperationKind,
    action: &str,
) -> std::result::Result<ServerState, ManagerError> {
    let lease = OperationLease::acquire(kind, OperationPhase::Recovering).await;
    if let Err(error) = recover_before_mutation().await {
        return lease.finish(Err(error)).await;
    }
    let phase = if action == "restart" || action == "retry" {
        OperationPhase::Restarting
    } else {
        OperationPhase::Starting
    };
    set_operation_phase(phase).await;
    let result = start_inner(action, lease.operation()).await;
    lease.finish(result).await
}

#[allow(clippy::too_many_lines)]
async fn apply_inner(
    config: ServerConfig,
    start_after_save: bool,
    previous_desired_running: bool,
    operation: &ServerOperation,
) -> std::result::Result<Option<String>, ManagerError> {
    set_operation_phase(OperationPhase::Validating).await;
    let prepared = prepare_candidate(config).await?;

    set_operation_phase(OperationPhase::Staging).await;
    if let Some(previous) = &prepared.previous {
        server_config::durable_atomic_write(server_config::CONFIG_BACKUP, previous)
            .await
            .map_err(|error| {
                manager_error(
                    ManagerErrorKind::Internal,
                    "backup_failed",
                    "staging",
                    "Could not preserve the current configuration",
                    error,
                )
            })?;
    } else {
        remove_file_if_present(server_config::CONFIG_BACKUP).await;
    }

    let mut journal = TransactionJournal {
        operation_id: operation.id.clone(),
        kind: operation.kind,
        phase: OperationPhase::Staging,
        previous_existed: prepared.previous.is_some(),
        previous_revision: prepared
            .previous
            .as_ref()
            .map(|_| prepared.previous_revision.clone()),
        candidate_revision: prepared.candidate_revision.clone(),
        desired_running: start_after_save,
        previous_desired_running: Some(previous_desired_running),
    };
    write_journal(&journal).await?;

    // Desired state is transaction data too. The durable journal must exist
    // before changing it so a power cut cannot leave "running" with no config.
    if operation.kind == OperationKind::Setup
        && let Err(error) = system::set_service_desired("server", start_after_save).await
    {
        remove_file_if_present(server_config::CONFIG_CANDIDATE).await;
        remove_file_if_present(server_config::CONFIG_JOURNAL).await;
        return Err(manager_error(
            ManagerErrorKind::Internal,
            "desired_state_write_failed",
            "activation",
            "Could not save SnapDog's start preference",
            error,
        ));
    }

    // Close the revision-check/activation TOCTOU window as far as a normal file
    // API permits: re-read immediately before the same-directory atomic rename.
    let current = read_active_source().await.map_err(read_error)?;
    let current_revision = server_config::config_revision(current.as_deref().unwrap_or(""));
    if current_revision != prepared.previous_revision {
        remove_file_if_present(server_config::CONFIG_CANDIDATE).await;
        remove_file_if_present(server_config::CONFIG_JOURNAL).await;
        return Err(conflict_error());
    }

    set_operation_phase(OperationPhase::Activating).await;
    // Write-ahead boundary: recovery must see Activating before the active file
    // can possibly become the candidate.
    journal.phase = OperationPhase::Activating;
    write_journal(&journal).await?;
    tokio::fs::rename(server_config::CONFIG_CANDIDATE, server_config::CONFIG_PATH)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "activation_failed",
                "activation",
                "Could not activate the new configuration",
                error,
            )
        })?;
    sync_data_directory().await.map_err(|error| {
        manager_error(
            ManagerErrorKind::Internal,
            "activation_sync_failed",
            "activation",
            "Could not durably activate the new configuration",
            error,
        )
    })?;

    if !start_after_save {
        set_operation_phase(OperationPhase::Stopping).await;
        journal.phase = OperationPhase::Stopping;
        write_journal(&journal).await?;
        stop_inner().await?;
        remove_file_if_present(server_config::CONFIG_JOURNAL).await;
        return Ok(None);
    }

    let action = if prepared.previous.is_some() {
        "restart"
    } else {
        "start"
    };
    set_operation_phase(if action == "restart" {
        OperationPhase::Restarting
    } else {
        OperationPhase::Starting
    })
    .await;
    journal.phase = if action == "restart" {
        OperationPhase::Restarting
    } else {
        OperationPhase::Starting
    };
    write_journal(&journal).await?;

    if let Err(apply_error) = start_and_verify(action, &prepared.parsed, Some(&mut journal)).await {
        if prepared.previous.is_none()
            && tokio::fs::metadata(server_config::CONFIG_LAST_GOOD)
                .await
                .is_err()
        {
            remove_file_if_present(server_config::CONFIG_JOURNAL).await;
            return Err(apply_error);
        }

        set_operation_phase(OperationPhase::RollingBack).await;
        journal.phase = OperationPhase::RollingBack;
        write_journal(&journal).await?;
        let rollback_desired = if operation.kind == OperationKind::Setup {
            previous_desired_running
        } else {
            start_after_save
        };
        return rollback_after_failure(apply_error, &prepared, rollback_desired).await;
    }

    remember_verified_revision(&prepared.candidate_revision).await;
    server_config::durable_atomic_write(server_config::CONFIG_LAST_GOOD, &prepared.candidate)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "last_good_write_failed",
                "finalizing",
                "SnapDog started, but its recovery copy could not be saved",
                error,
            )
        })?;
    remove_file_if_present(server_config::CONFIG_JOURNAL).await;
    Ok(Some(prepared.candidate_revision))
}

async fn rollback_after_failure(
    apply_error: ManagerError,
    prepared: &PreparedCandidate,
    rollback_desired: bool,
) -> std::result::Result<Option<String>, ManagerError> {
    let rollback_source = match tokio::fs::read_to_string(server_config::CONFIG_LAST_GOOD).await {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => prepared.previous.clone(),
        Err(error) => {
            return Err(manager_error(
                ManagerErrorKind::Internal,
                "last_good_read_failed",
                "rollback",
                "The new configuration failed and recovery data could not be read",
                error,
            ));
        }
    };
    let Some(rollback_source) = rollback_source else {
        return Err(apply_error);
    };

    system::set_service_desired("server", rollback_desired)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "desired_state_restore_failed",
                "rollback",
                "The previous SnapDog start preference could not be restored",
                error,
            )
        })?;
    server_config::durable_atomic_write(server_config::CONFIG_PATH, &rollback_source)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "rollback_write_failed",
                "rollback",
                "The previous configuration could not be restored",
                error,
            )
        })?;
    let rollback_config = server_config::parse_config_toml(&rollback_source).map_err(|error| {
        manager_error(
            ManagerErrorKind::Internal,
            "rollback_config_invalid",
            "rollback",
            "The recovery configuration is invalid",
            error,
        )
    })?;
    let restored = if rollback_desired {
        start_and_verify("restart", &rollback_config, None).await
    } else {
        match system::control_service("server", "stop").await {
            Ok(()) => wait_stopped().await,
            Err(error) => Err(manager_error(
                ManagerErrorKind::Runtime,
                "rollback_stop_failed",
                "rollback",
                "The restored SnapDog service could not be stopped",
                error,
            )),
        }
    };
    match restored {
        Ok(()) => {
            if rollback_desired {
                remember_verified_revision(&server_config::config_revision(&rollback_source)).await;
            } else {
                clear_verified_revision().await;
            }
            remove_file_if_present(server_config::CONFIG_JOURNAL).await;
            let mut issue = apply_error.issue;
            issue.rollback_succeeded = Some(true);
            RUNTIME_MEMORY.write().await.issue = Some(issue.clone());
            Err(ManagerError {
                kind: ManagerErrorKind::Runtime,
                issue,
            })
        }
        Err(rollback_error) => {
            let mut issue = ServerIssue::new(
                "rollback_failed",
                "rollback",
                "SnapDog could not start and recovery also failed",
                format!(
                    "new configuration: {}; recovery configuration: {}",
                    apply_error.issue.detail, rollback_error.issue.detail
                ),
            );
            issue.rollback_succeeded = Some(false);
            Err(ManagerError {
                kind: ManagerErrorKind::Runtime,
                issue,
            })
        }
    }
}

async fn start_inner(
    action: &str,
    _operation: &ServerOperation,
) -> std::result::Result<Option<String>, ManagerError> {
    // Persist intent first. If validation or startup fails, the state API must
    // say "wanted running, failed" instead of silently falling back to stopped.
    system::set_service_desired("server", true)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "desired_state_write_failed",
                "activation",
                "Could not enable SnapDog",
                error,
            )
        })?;
    let envelope = inspect_config(true).await;
    if !matches!(
        envelope.state,
        ConfigState::Valid | ConfigState::ValidUnverified
    ) || !envelope.issues.is_empty()
    {
        let issue = envelope.issues.first().cloned().unwrap_or_else(|| {
            ServerIssue::new(
                "setup_required",
                "validation",
                "SnapDog needs a valid configuration before it can start",
                format!("configuration state: {:?}", envelope.state),
            )
        });
        return Err(ManagerError {
            kind: ManagerErrorKind::Invalid,
            issue,
        });
    }
    let config = envelope.config.ok_or_else(|| ManagerError {
        kind: ManagerErrorKind::Invalid,
        issue: ServerIssue::new(
            "setup_required",
            "validation",
            "SnapDog needs a valid configuration before it can start",
            "No parsed configuration is available",
        ),
    })?;
    set_operation_phase(if action == "start" {
        OperationPhase::Starting
    } else {
        OperationPhase::Restarting
    })
    .await;
    let systemd_action = if action == "retry" || action == "start" {
        let snapshot = service_snapshot().await.unwrap_or_default();
        if snapshot.active_state == "active" {
            "restart"
        } else {
            "start"
        }
    } else {
        action
    };
    start_and_verify(systemd_action, &config, None).await?;
    let source = read_active_source()
        .await
        .map_err(read_error)?
        .ok_or_else(|| ManagerError {
            kind: ManagerErrorKind::Internal,
            issue: ServerIssue::new(
                "config_disappeared",
                "finalizing",
                "The active configuration disappeared after startup",
                "active config is missing",
            ),
        })?;
    let revision = server_config::config_revision(&source);
    remember_verified_revision(&revision).await;
    server_config::durable_atomic_write(server_config::CONFIG_LAST_GOOD, &source)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "last_good_write_failed",
                "finalizing",
                "SnapDog started, but its recovery copy could not be saved",
                error,
            )
        })?;
    Ok(Some(revision))
}

async fn stop_inner() -> std::result::Result<(), ManagerError> {
    system::set_service_desired("server", false)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "desired_state_write_failed",
                "stopping",
                "Could not disable SnapDog",
                error,
            )
        })?;
    system::control_service("server", "stop")
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Runtime,
                "stop_failed",
                "stopping",
                "SnapDog could not be stopped",
                error,
            )
        })?;
    wait_stopped().await
}

async fn reconcile_inner(
    operation: &ServerOperation,
) -> std::result::Result<Option<String>, ManagerError> {
    if let RecoveryOutcome::Reconciled(revision) = recover_interrupted_transaction().await? {
        return Ok(revision);
    }
    // A candidate without a journal can only be residue from a cancelled
    // validation/staging await. It is never safe to activate and must not leak
    // into a later request.
    remove_file_if_present(server_config::CONFIG_CANDIDATE).await;
    match reconciliation_target(read_desired_state().await?) {
        ReconciliationTarget::Running => start_inner("start", operation).await,
        ReconciliationTarget::Stopped => {
            stop_inner().await?;
            Ok(None)
        }
    }
}

const fn reconciliation_target(desired_running: bool) -> ReconciliationTarget {
    if desired_running {
        ReconciliationTarget::Running
    } else {
        ReconciliationTarget::Stopped
    }
}

#[allow(clippy::too_many_lines)]
async fn recover_interrupted_transaction() -> std::result::Result<RecoveryOutcome, ManagerError> {
    ensure_transaction_store_safe().await?;
    let journal_content = match tokio::fs::read_to_string(server_config::CONFIG_JOURNAL).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RecoveryOutcome::Continue);
        }
        Err(error) => {
            return Err(manager_error(
                ManagerErrorKind::Internal,
                "transaction_journal_unreadable",
                "recovery",
                "An interrupted SnapDog update could not be inspected",
                error,
            ));
        }
    };
    let mut journal: TransactionJournal =
        serde_json::from_str(&journal_content).map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "transaction_journal_invalid",
                "recovery",
                "The interrupted SnapDog transaction record is invalid",
                error,
            )
        })?;
    let active_source = read_active_source().await.map_err(read_error)?;
    let last_good_source = read_optional_source(server_config::CONFIG_LAST_GOOD).await?;
    let decision = decide_recovery(
        &journal,
        active_source.as_deref(),
        last_good_source.as_deref(),
    );

    match decision {
        RecoveryDecision::ActivationNotApplied => {
            restore_desired_after_rollback(&journal).await?;
            finish_recovery_artifacts().await;
            Ok(RecoveryOutcome::Continue)
        }
        RecoveryDecision::CandidateCommitted => {
            system::set_service_desired("server", journal.desired_running)
                .await
                .map_err(|error| {
                    manager_error(
                        ManagerErrorKind::Internal,
                        "desired_state_restore_failed",
                        "recovery",
                        "The committed configuration's start preference could not be restored",
                        error,
                    )
                })?;
            finish_recovery_artifacts().await;
            // Normal reconciliation still verifies a desired-running service.
            Ok(RecoveryOutcome::Continue)
        }
        RecoveryDecision::RollbackCompleted => {
            let source = active_source.expect("decision guarantees a restored config");
            verify_recovered_rollback(&journal, &source, None).await
        }
        RecoveryDecision::CandidateNeedsVerification => {
            let source = active_source.expect("decision guarantees an active candidate");
            let candidate = validate_recovered_candidate(&source).await;
            let candidate = match candidate {
                Ok(candidate) => candidate,
                Err(validation_error) => {
                    return recover_failed_candidate(&journal, last_good_source, validation_error)
                        .await;
                }
            };

            system::set_service_desired("server", journal.desired_running)
                .await
                .map_err(|error| {
                    manager_error(
                        ManagerErrorKind::Internal,
                        "desired_state_restore_failed",
                        "recovery",
                        "The recovered candidate's start preference could not be restored",
                        error,
                    )
                })?;
            if !journal.desired_running {
                stop_inner().await?;
                finish_recovery_artifacts().await;
                return Ok(RecoveryOutcome::Reconciled(None));
            }

            let snapshot = service_snapshot().await.unwrap_or_default();
            let action = if snapshot.active_state == "active" {
                "restart"
            } else {
                "start"
            };
            if let Err(start_error) = start_and_verify(action, &candidate, Some(&mut journal)).await
            {
                return recover_failed_candidate(&journal, last_good_source, start_error).await;
            }
            remember_verified_revision(&journal.candidate_revision).await;
            server_config::durable_atomic_write(server_config::CONFIG_LAST_GOOD, &source)
                .await
                .map_err(|error| {
                    manager_error(
                        ManagerErrorKind::Internal,
                        "last_good_write_failed",
                        "recovery",
                        "The recovered configuration started but could not be committed",
                        error,
                    )
                })?;
            finish_recovery_artifacts().await;
            Ok(RecoveryOutcome::Reconciled(Some(
                journal.candidate_revision,
            )))
        }
        RecoveryDecision::Conflict => Err(ManagerError {
            kind: ManagerErrorKind::Conflict,
            issue: ServerIssue::new(
                "recovery_revision_conflict",
                "recovery",
                "An interrupted SnapDog update conflicts with the active configuration",
                format!(
                    "operation {} expected revision {} or {:?}",
                    journal.operation_id, journal.candidate_revision, journal.previous_revision
                ),
            ),
        }),
    }
}

fn decide_recovery(
    journal: &TransactionJournal,
    active_source: Option<&str>,
    last_good_source: Option<&str>,
) -> RecoveryDecision {
    let active_revision = active_source.map(server_config::config_revision);
    let last_good_revision = last_good_source.map(server_config::config_revision);
    if active_revision.as_deref() == Some(journal.candidate_revision.as_str()) {
        return if last_good_revision.as_deref() == Some(journal.candidate_revision.as_str()) {
            RecoveryDecision::CandidateCommitted
        } else {
            RecoveryDecision::CandidateNeedsVerification
        };
    }
    let active_is_previous = if journal.previous_existed {
        active_revision.as_deref() == journal.previous_revision.as_deref()
    } else {
        active_source.is_none()
    };
    if journal.phase == OperationPhase::RollingBack
        && (active_is_previous
            || (last_good_revision.is_some()
                && active_revision == last_good_revision
                && last_good_revision.as_deref() != Some(journal.candidate_revision.as_str())))
    {
        return RecoveryDecision::RollbackCompleted;
    }
    if active_is_previous {
        RecoveryDecision::ActivationNotApplied
    } else {
        RecoveryDecision::Conflict
    }
}

async fn validate_recovered_candidate(
    source: &str,
) -> std::result::Result<ServerConfig, ManagerError> {
    let config = server_config::parse_config_toml(source).map_err(validation_error)?;
    preflight_config(&config).await?;
    server_config::validate_with_server(server_config::CONFIG_PATH)
        .await
        .map_err(config_guard_error)?;
    Ok(config)
}

async fn recover_failed_candidate(
    journal: &TransactionJournal,
    last_good_source: Option<String>,
    candidate_error: ManagerError,
) -> std::result::Result<RecoveryOutcome, ManagerError> {
    let fallback = if let Some(last_good) = last_good_source {
        Some(last_good)
    } else if journal.previous_existed {
        read_optional_source(server_config::CONFIG_BACKUP).await?
    } else {
        None
    };
    let Some(fallback) = fallback else {
        let mut candidate_error = candidate_error;
        candidate_error.issue.rollback_succeeded = Some(false);
        // First setup has no fallback by definition. Preserve the failed active
        // config and desired intent for repair, but release the WAL so Validate,
        // Apply, and Stop can actually fix the device without a controller reboot.
        system::set_service_desired("server", journal.desired_running)
            .await
            .map_err(|error| {
                manager_error(
                    ManagerErrorKind::Internal,
                    "desired_state_restore_failed",
                    "recovery",
                    "The failed first setup's start preference could not be restored",
                    error,
                )
            })?;
        if !journal.desired_running {
            system::control_service("server", "stop")
                .await
                .map_err(|error| {
                    manager_error(
                        ManagerErrorKind::Runtime,
                        "recovery_stop_failed",
                        "recovery",
                        "The failed first setup could not be stopped",
                        error,
                    )
                })?;
            wait_stopped().await?;
        }
        RUNTIME_MEMORY.write().await.issue = Some(candidate_error.issue);
        finish_recovery_artifacts().await;
        return Ok(RecoveryOutcome::Reconciled(None));
    };
    let mut rollback_journal = journal.clone();
    rollback_journal.phase = OperationPhase::RollingBack;
    write_journal(&rollback_journal).await?;
    server_config::durable_atomic_write(server_config::CONFIG_PATH, &fallback)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "recovery_write_failed",
                "recovery",
                "The last known-good configuration could not be restored",
                error,
            )
        })?;
    verify_recovered_rollback(&rollback_journal, &fallback, Some(candidate_error)).await
}

async fn verify_recovered_rollback(
    journal: &TransactionJournal,
    source: &str,
    original_error: Option<ManagerError>,
) -> std::result::Result<RecoveryOutcome, ManagerError> {
    restore_desired_after_rollback(journal).await?;
    let config = match validate_recovered_candidate(source).await {
        Ok(config) => config,
        Err(error) => return Err(rollback_verification_error(original_error, &error)),
    };
    let desired_running = desired_after_rollback(journal);
    let verification = if desired_running {
        // Keep the journal in RollingBack while verifying. A crash then resumes
        // fallback verification instead of mistaking it for the new candidate.
        start_and_verify("restart", &config, None).await
    } else {
        match system::control_service("server", "stop").await {
            Ok(()) => wait_stopped().await,
            Err(error) => Err(manager_error(
                ManagerErrorKind::Runtime,
                "rollback_stop_failed",
                "rollback",
                "The restored SnapDog service could not be stopped",
                error,
            )),
        }
    };
    if let Err(error) = verification {
        return Err(rollback_verification_error(original_error, &error));
    }
    let revision = server_config::config_revision(source);
    if desired_running {
        remember_verified_revision(&revision).await;
    } else {
        clear_verified_revision().await;
    }
    if desired_running {
        server_config::durable_atomic_write(server_config::CONFIG_LAST_GOOD, source)
            .await
            .map_err(|error| {
                manager_error(
                    ManagerErrorKind::Internal,
                    "last_good_write_failed",
                    "recovery",
                    "The restored configuration started but could not be committed",
                    error,
                )
            })?;
    }
    let mut issue = original_error.map_or_else(
        || {
            ServerIssue::new(
                "interrupted_rollback_recovered",
                "recovery",
                "An interrupted configuration update was rolled back",
                format!("recovered operation {}", journal.operation_id),
            )
        },
        |error| error.issue,
    );
    issue.rollback_succeeded = Some(true);
    RUNTIME_MEMORY.write().await.issue = Some(issue);
    finish_recovery_artifacts().await;
    Ok(RecoveryOutcome::Reconciled(
        desired_running.then_some(revision),
    ))
}

fn rollback_verification_error(
    original_error: Option<ManagerError>,
    rollback_error: &ManagerError,
) -> ManagerError {
    let mut issue = ServerIssue::new(
        "rollback_failed",
        "rollback",
        "SnapDog recovery could not restore a working service",
        original_error.map_or_else(
            || format!("recovery configuration: {}", rollback_error.issue.detail),
            |original| {
                format!(
                    "new configuration: {}; recovery configuration: {}",
                    original.issue.detail, rollback_error.issue.detail
                )
            },
        ),
    );
    issue.rollback_succeeded = Some(false);
    ManagerError {
        kind: ManagerErrorKind::Runtime,
        issue,
    }
}

async fn restore_desired_after_rollback(
    journal: &TransactionJournal,
) -> std::result::Result<(), ManagerError> {
    system::set_service_desired("server", desired_after_rollback(journal))
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "desired_state_restore_failed",
                "recovery",
                "The previous SnapDog start preference could not be restored",
                error,
            )
        })
}

fn desired_after_rollback(journal: &TransactionJournal) -> bool {
    if journal.kind == OperationKind::Setup {
        journal
            .previous_desired_running
            .unwrap_or(journal.desired_running)
    } else {
        journal.desired_running
    }
}

fn should_restore_setup_desired(issue: &ServerIssue) -> bool {
    !matches!(
        issue.stage.as_str(),
        "startup" | "stopping" | "verification" | "rollback" | "finalizing"
    )
}

async fn finish_recovery_artifacts() {
    remove_file_if_present(server_config::CONFIG_CANDIDATE).await;
    remove_file_if_present(server_config::CONFIG_JOURNAL).await;
}

async fn read_optional_source(path: &str) -> std::result::Result<Option<String>, ManagerError> {
    match tokio::fs::read_to_string(path).await {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(manager_error(
            ManagerErrorKind::Internal,
            "recovery_source_unreadable",
            "recovery",
            "SnapDog recovery data could not be read",
            error,
        )),
    }
}

async fn validate_settings_import_server_source(
    source: &str,
) -> std::result::Result<(), ManagerError> {
    ensure_transaction_store_safe().await?;
    let config = server_config::parse_config_toml(source).map_err(validation_error)?;
    server_config::validate(&config).map_err(validation_error)?;
    preflight_config(&config).await?;
    let sequence = OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let validation_path = format!(
        "/data/snapdog/.settings-import-validation.{}.{sequence}.toml",
        std::process::id()
    );
    // The guard is a root child of ctrl; the DynamicUser service never needs
    // access to this short-lived copy of imported credentials.
    system::atomic_write_with_mode(&validation_path, source, 0o600)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "settings_import_validation_stage_failed",
                "validation",
                "The imported server configuration could not be staged for validation",
                error,
            )
        })?;
    let validation = server_config::validate_with_server(&validation_path)
        .await
        .map_err(config_guard_error);
    let cleanup = async {
        tokio::fs::remove_file(&validation_path).await?;
        sync_data_directory().await
    }
    .await;
    match (validation, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(validation_error), Ok(())) => Err(validation_error),
        (validation, Err(cleanup_error)) => Err(manager_error(
            ManagerErrorKind::Internal,
            "settings_import_validation_cleanup_failed",
            "validation",
            "The imported server validation file could not be removed",
            validation.map_or_else(
                |error| {
                    format!(
                        "validation: {}; cleanup: {cleanup_error}",
                        error.issue.detail
                    )
                },
                |()| cleanup_error.to_string(),
            ),
        )),
    }
}

async fn prepare_candidate(
    config: ServerConfig,
) -> std::result::Result<PreparedCandidate, ManagerError> {
    ensure_transaction_store_safe().await?;
    let previous = read_active_source().await.map_err(read_error)?;
    let source = previous.as_deref().unwrap_or("");
    let previous_revision = server_config::config_revision(source);
    if config.revision.is_empty() || config.revision != previous_revision {
        return Err(conflict_error());
    }

    if !server_config::uses_advanced_toml(&config) {
        server_config::validate(&config).map_err(validation_error)?;
    }
    let candidate = server_config::render_candidate(source, &config).map_err(validation_error)?;
    let parsed = server_config::parse_config_toml(&candidate).map_err(validation_error)?;
    preflight_config(&parsed).await?;
    server_config::durable_atomic_write(server_config::CONFIG_CANDIDATE, &candidate)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "candidate_write_failed",
                "staging",
                "The new configuration could not be staged",
                error,
            )
        })?;
    if let Err(error) = server_config::validate_with_server(server_config::CONFIG_CANDIDATE).await {
        remove_file_if_present(server_config::CONFIG_CANDIDATE).await;
        return Err(config_guard_error(error));
    }

    Ok(PreparedCandidate {
        previous,
        previous_revision,
        candidate_revision: server_config::config_revision(&candidate),
        candidate,
        parsed,
    })
}

async fn ensure_transaction_store_safe() -> std::result::Result<(), ManagerError> {
    let directory = Path::new("/data/snapdog");
    let metadata = tokio::fs::symlink_metadata(directory)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "config_directory_unavailable",
                "environment",
                "The SnapDog configuration directory is unavailable",
                error,
            )
        })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(preflight_error(
            "config_directory_unsafe",
            "The SnapDog configuration directory is unsafe",
            "/data/snapdog must be a real directory",
            "system.state_dir",
        ));
    }
    for path in [
        server_config::CONFIG_PATH,
        server_config::CONFIG_BACKUP,
        server_config::CONFIG_CANDIDATE,
        server_config::CONFIG_LAST_GOOD,
        server_config::CONFIG_JOURNAL,
        server_config::LAST_OPERATION_ISSUE,
    ] {
        match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.uid() != 0 {
                        return Err(preflight_error(
                            "transaction_file_owner_invalid",
                            "A SnapDog configuration file has an unsafe owner",
                            path,
                            "system.state_dir",
                        ));
                    }
                }
            }
            Ok(_) => {
                return Err(preflight_error(
                    "transaction_file_unsafe",
                    "A SnapDog configuration file is not a regular file",
                    path,
                    "system.state_dir",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(manager_error(
                    ManagerErrorKind::Internal,
                    "transaction_file_unreadable",
                    "environment",
                    "A SnapDog configuration file cannot be inspected",
                    error,
                ));
            }
        }
    }
    Ok(())
}

async fn preflight_config(config: &ServerConfig) -> std::result::Result<(), ManagerError> {
    register_config_secrets(config);
    if !state_dir_is_managed(&config.system.state_dir) {
        return Err(preflight_error(
            "state_directory_unsupported",
            "SnapDog's state directory is outside the writable server area",
            "Use /data/snapdog/state or one of its subdirectories",
            "system.state_dir",
        ));
    }
    inspect_managed_state_path(&config.system.state_dir, ManagedPathKind::WritableDirectory)
        .await
        .map_err(|error| {
            preflight_error(
                "state_directory_unavailable",
                "SnapDog's state directory is not safely writable",
                &error.to_string(),
                "system.state_dir",
            )
        })?;
    if let Some(log_file) = config.system.log_file.as_deref() {
        inspect_managed_state_path(log_file, ManagedPathKind::WritableFile)
            .await
            .map_err(|error| {
                preflight_error(
                    "log_file_unavailable",
                    "SnapDog's log file is outside its writable state area",
                    &error.to_string(),
                    "system.log_file",
                )
            })?;
    }
    if let Some(subsonic) = config.subsonic.as_ref() {
        inspect_managed_state_path(&subsonic.cache.path, ManagedPathKind::WritableDirectory)
            .await
            .map_err(|error| {
                preflight_error(
                    "cache_directory_unavailable",
                    "The Subsonic cache is outside SnapDog's writable state area",
                    &error.to_string(),
                    "subsonic.cache.path",
                )
            })?;
    }

    let Some((certificate, private_key)) = configured_tls_paths(config)? else {
        return Ok(());
    };
    for (path, field_path, label) in [
        (certificate, "http.tls_cert", "TLS certificate"),
        (private_key, "http.tls_key", "TLS private key"),
    ] {
        inspect_managed_state_path(path, ManagedPathKind::ReadableFile)
            .await
            .map_err(|error| {
                preflight_error(
                    "tls_file_unreadable",
                    &format!("{label} is not readable by SnapDog"),
                    &error.to_string(),
                    field_path,
                )
            })?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManagedPathKind {
    WritableDirectory,
    WritableFile,
    ReadableFile,
}

async fn inspect_managed_state_path(value: &str, kind: ManagedPathKind) -> Result<()> {
    const ROOT: &str = "/data/snapdog/state";
    anyhow::ensure!(state_dir_is_managed(value), "{value} is outside {ROOT}");
    let root = Path::new(ROOT);
    for protected in [Path::new("/data"), Path::new("/data/snapdog"), root] {
        let metadata = tokio::fs::symlink_metadata(protected)
            .await
            .with_context(|| format!("{} is missing", protected.display()))?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "{} must be a real directory",
            protected.display()
        );
    }

    #[cfg(unix)]
    let root_group = {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = tokio::fs::symlink_metadata(root).await?;
        anyhow::ensure!(metadata.uid() == 0, "{ROOT} must be owned by root");
        anyhow::ensure!(
            metadata.permissions().mode() & 0o2070 == 0o2070,
            "{ROOT} must be setgid and group-writable"
        );
        metadata.gid()
    };

    let requested = Path::new(value);
    let relative = requested
        .strip_prefix(root)
        .context("state path escaped its managed root")?;
    let components = relative.components().collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            anyhow::bail!("state path contains an unsafe component");
        };
        current.push(component);
        let is_last = index + 1 == components.len();
        match tokio::fs::symlink_metadata(&current).await {
            Ok(metadata) => {
                anyhow::ensure!(
                    !metadata.file_type().is_symlink(),
                    "{} must not be a symlink",
                    current.display()
                );
                let expects_directory = !is_last || kind == ManagedPathKind::WritableDirectory;
                anyhow::ensure!(
                    if expects_directory {
                        metadata.is_dir()
                    } else {
                        metadata.is_file()
                    },
                    "{} has the wrong file type",
                    current.display()
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::{MetadataExt, PermissionsExt};
                    let mode = metadata.permissions().mode();
                    anyhow::ensure!(
                        metadata.gid() == root_group,
                        "{} has the wrong service group",
                        current.display()
                    );
                    if metadata.is_dir() {
                        anyhow::ensure!(
                            mode & 0o2070 == 0o2070,
                            "{} is not setgid and group-writable",
                            current.display()
                        );
                    } else if kind == ManagedPathKind::ReadableFile {
                        anyhow::ensure!(
                            managed_file_permissions_allow(kind, true, mode),
                            "{} is not readable by the SnapDog service group",
                            current.display()
                        );
                    } else {
                        anyhow::ensure!(
                            managed_file_permissions_allow(kind, true, mode),
                            "{} is not readable and writable by the SnapDog service group",
                            current.display()
                        );
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                anyhow::ensure!(
                    kind == ManagedPathKind::WritableDirectory
                        || (kind == ManagedPathKind::WritableFile && is_last),
                    "{} does not exist",
                    current.display()
                );
                // No root mutation in the group-writable state tree: SnapDog may
                // create this missing tail itself under the inspected setgid dir.
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

const fn managed_file_permissions_allow(
    kind: ManagedPathKind,
    service_group_matches: bool,
    mode: u32,
) -> bool {
    service_group_matches
        && match kind {
            ManagedPathKind::ReadableFile => mode & 0o040 != 0,
            ManagedPathKind::WritableFile => mode & 0o060 == 0o060,
            ManagedPathKind::WritableDirectory => false,
        }
}

fn state_dir_is_managed(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        && path.starts_with(Path::new("/data/snapdog/state"))
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
}

#[allow(clippy::result_large_err)]
fn configured_tls_paths(
    config: &ServerConfig,
) -> std::result::Result<Option<(&str, &str)>, ManagerError> {
    match (
        config.http.tls_cert.as_deref(),
        config.http.tls_key.as_deref(),
    ) {
        (None, None) => Ok(None),
        (Some(certificate), Some(private_key))
            if !certificate.trim().is_empty() && !private_key.trim().is_empty() =>
        {
            Ok(Some((certificate, private_key)))
        }
        _ => Err(preflight_error(
            "tls_pair_incomplete",
            "TLS certificate and private key must be configured together",
            "Set both TLS paths, or clear both",
            "http.tls_cert",
        )),
    }
}

fn preflight_error(code: &str, summary: &str, detail: &str, field_path: &str) -> ManagerError {
    let mut issue = ServerIssue::new(code, "environment", summary, detail);
    issue.field_path = Some(field_path.to_string());
    ManagerError {
        kind: ManagerErrorKind::Invalid,
        issue,
    }
}

async fn start_and_verify(
    action: &str,
    config: &ServerConfig,
    journal: Option<&mut TransactionJournal>,
) -> std::result::Result<(), ManagerError> {
    // An explicit user operation is also explicit permission to clear systemd's
    // bounded start-limit. Without this, Retry can fail instantly even after the
    // underlying configuration problem was corrected.
    system::control_service("server", "reset-failed")
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Runtime,
                "systemd_reset_failed",
                "startup",
                "systemd could not reset SnapDog's failed state",
                error,
            )
        })?;
    system::control_service("server", action)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Runtime,
                "systemd_job_failed",
                "startup",
                "systemd could not start SnapDog",
                error,
            )
        })?;
    set_operation_phase(OperationPhase::Verifying).await;
    if let Some(journal) = journal {
        journal.phase = OperationPhase::Verifying;
        write_journal(journal).await?;
    }
    RUNTIME_MEMORY.write().await.health_state = HealthState::Checking;

    let deadline = tokio::time::Instant::now() + VERIFY_TIMEOUT;
    let mut last_detail = String::from("SnapDog has not reported ready");
    loop {
        let first = service_snapshot().await.map_err(|error| {
            manager_error(
                ManagerErrorKind::Runtime,
                "systemd_status_failed",
                "verification",
                "SnapDog status could not be verified",
                error,
            )
        })?;
        if first.active_state == "active" {
            match probe_ready(config).await {
                Ok(()) => {
                    tokio::time::sleep(STABILITY_WINDOW).await;
                    let second = service_snapshot().await.map_err(|error| {
                        manager_error(
                            ManagerErrorKind::Runtime,
                            "systemd_status_failed",
                            "verification",
                            "SnapDog stability could not be verified",
                            error,
                        )
                    })?;
                    if second.active_state == "active"
                        && stable_invocation(&first, &second)
                        && probe_ready(config).await.is_ok()
                    {
                        RUNTIME_MEMORY.write().await.health_state = HealthState::Healthy;
                        return Ok(());
                    }
                    last_detail = format!(
                        "service changed while stabilizing ({} / {}, restarts {:?} -> {:?})",
                        first.active_state,
                        second.active_state,
                        first.restart_count,
                        second.restart_count
                    );
                }
                Err(detail) => last_detail = detail,
            }
        } else if first.active_state == "failed" {
            last_detail = format!(
                "systemd state failed (result {}, exit status {:?})",
                first.result, first.exec_main_status
            );
        }
        if tokio::time::Instant::now() >= deadline {
            RUNTIME_MEMORY.write().await.health_state = HealthState::Unhealthy;
            let snapshot = service_snapshot().await.unwrap_or_default();
            let mut issue = ServerIssue::new(
                "readiness_timeout",
                "verification",
                "SnapDog did not become ready",
                last_detail,
            );
            issue.exit_code = snapshot.exec_main_status;
            issue.systemd_result = nonempty(snapshot.result);
            return Err(ManagerError {
                kind: ManagerErrorKind::Runtime,
                issue,
            });
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn wait_stopped() -> std::result::Result<(), ManagerError> {
    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    loop {
        let snapshot = service_snapshot().await.map_err(|error| {
            manager_error(
                ManagerErrorKind::Runtime,
                "systemd_status_failed",
                "stopping",
                "SnapDog stop could not be verified",
                error,
            )
        })?;
        if snapshot.active_state == "inactive" || snapshot.active_state == "failed" {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ManagerError {
                kind: ManagerErrorKind::Runtime,
                issue: ServerIssue::new(
                    "stop_timeout",
                    "stopping",
                    "SnapDog did not stop",
                    format!(
                        "systemd state remained {}/{}",
                        snapshot.active_state, snapshot.sub_state
                    ),
                ),
            });
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[allow(clippy::too_many_lines)]
async fn inspect_config(verify_with_server: bool) -> ServerConfigEnvelope {
    if let Ok(metadata) = tokio::fs::symlink_metadata(server_config::CONFIG_PATH).await
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return ServerConfigEnvelope {
            state: ConfigState::Unreadable,
            revision: String::new(),
            raw_toml: String::new(),
            config: None,
            issues: vec![ServerIssue::new(
                "config_file_unsafe",
                "configuration",
                "The SnapDog configuration is not a regular file",
                server_config::CONFIG_PATH,
            )],
        };
    }
    let source = match tokio::fs::read_to_string(server_config::CONFIG_PATH).await {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ServerConfigEnvelope {
                state: ConfigState::Missing,
                revision: server_config::config_revision(""),
                raw_toml: String::new(),
                config: Some(initial_config()),
                issues: Vec::new(),
            };
        }
        Err(error) => {
            return ServerConfigEnvelope {
                state: ConfigState::Unreadable,
                revision: String::new(),
                raw_toml: String::new(),
                config: None,
                issues: vec![ServerIssue::new(
                    "config_unreadable",
                    "configuration",
                    "The SnapDog configuration cannot be read",
                    error.to_string(),
                )],
            };
        }
    };
    let revision = server_config::config_revision(&source);
    if source.trim().is_empty() {
        return ServerConfigEnvelope {
            state: ConfigState::Invalid,
            revision,
            raw_toml: source,
            config: None,
            issues: vec![ServerIssue::new(
                "config_empty",
                "configuration",
                "The SnapDog configuration is empty",
                "Add at least one zone or run setup again",
            )],
        };
    }

    let document = source.parse::<toml_edit::DocumentMut>();
    let document = match document {
        Ok(document) => document,
        Err(error) => {
            let (line, column) = error
                .span()
                .map_or((0, 0), |span| line_column(&source, span.start));
            let mut issue = ServerIssue::new(
                "config_syntax_error",
                "configuration",
                "The SnapDog configuration contains invalid TOML",
                error.to_string(),
            );
            issue.line = (line > 0).then_some(line);
            issue.column = (column > 0).then_some(column);
            return ServerConfigEnvelope {
                state: ConfigState::Invalid,
                revision,
                raw_toml: source,
                config: None,
                issues: vec![issue],
            };
        }
    };
    drop(document);

    let config = match server_config::parse_config_toml(&source) {
        Ok(config) => config,
        Err(error) => {
            return ServerConfigEnvelope {
                state: ConfigState::Invalid,
                revision,
                raw_toml: source,
                config: None,
                issues: vec![validation_error(error).issue],
            };
        }
    };
    register_config_secrets(&config);
    if let Err(error) = server_config::validate(&config) {
        return ServerConfigEnvelope {
            state: ConfigState::Invalid,
            revision,
            raw_toml: source,
            config: Some(config),
            issues: vec![validation_error(error).issue],
        };
    }

    if verify_with_server {
        match server_config::validate_with_server(server_config::CONFIG_PATH).await {
            Ok(()) => {
                let verified = revision_is_runtime_verified(&revision).await;
                ServerConfigEnvelope {
                    state: if verified {
                        ConfigState::Valid
                    } else {
                        ConfigState::ValidUnverified
                    },
                    revision,
                    raw_toml: source,
                    config: Some(config),
                    issues: Vec::new(),
                }
            }
            Err(error) => match config_guard_error(error) {
                ManagerError {
                    kind: ManagerErrorKind::Invalid,
                    issue,
                } => ServerConfigEnvelope {
                    state: ConfigState::Invalid,
                    revision,
                    raw_toml: source,
                    config: Some(config),
                    issues: vec![issue],
                },
                infrastructure_error => ServerConfigEnvelope {
                    state: ConfigState::ValidUnverified,
                    revision,
                    raw_toml: source,
                    config: Some(config),
                    issues: vec![infrastructure_error.issue],
                },
            },
        }
    } else {
        let verified = revision_is_runtime_verified(&revision).await;
        ServerConfigEnvelope {
            state: if verified {
                ConfigState::Valid
            } else {
                ConfigState::ValidUnverified
            },
            revision,
            raw_toml: source,
            config: Some(config),
            issues: Vec::new(),
        }
    }
}

async fn revision_is_runtime_verified(revision: &str) -> bool {
    let last_good = read_revision(server_config::CONFIG_LAST_GOOD).await;
    let active = RUNTIME_MEMORY.read().await.active_revision.clone();
    last_good.as_deref() == Some(revision) || active.as_deref() == Some(revision)
}

async fn probe_ready(config: &ServerConfig) -> std::result::Result<(), String> {
    let scheme = if config.http.tls_cert.is_some() {
        "https"
    } else {
        "http"
    };
    let url = format!(
        "{scheme}://{}:{}/health/ready",
        readiness_host(&config.http.bind),
        config.http.port
    );
    let output = tokio::time::timeout(
        PROBE_TIMEOUT + Duration::from_millis(250),
        tokio::process::Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--fail",
                "--insecure",
                "--max-time",
                "2",
                "--noproxy",
                "*",
                "--write-out",
                "\n%{http_code}",
            ])
            .arg(&url)
            .output(),
    )
    .await
    .map_err(|_| format!("readiness request to {url} timed out"))?
    .map_err(|error| redact_text(&format!("could not execute readiness check: {error}")))?;
    if !output.status.success() {
        return Err(redact_text(&format!(
            "readiness request to {url} failed: {}",
            server_config::command_error(&output)
        )));
    }
    let Some(separator) = output.stdout.iter().rposition(|byte| *byte == b'\n') else {
        return Err("readiness response did not include an HTTP status".to_string());
    };
    let body = &output.stdout[..separator];
    let status = &output.stdout[separator + 1..];
    let status = std::str::from_utf8(status)
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok());
    if !status.is_some_and(|status| (200..300).contains(&status)) || body != b"ready" {
        return Err("/health/ready did not return 2xx with the exact body 'ready'".to_string());
    }
    Ok(())
}

fn readiness_host(bind: &str) -> String {
    match bind.parse::<std::net::IpAddr>() {
        Ok(address) if address.is_unspecified() => "127.0.0.1".to_string(),
        Ok(std::net::IpAddr::V6(address)) => format!("[{address}]"),
        Ok(address) => address.to_string(),
        Err(_) => "127.0.0.1".to_string(),
    }
}

fn config_endpoint(config: &ServerConfig) -> Option<String> {
    server_config::sanitized_public_base_url(&config.http.base_url)
}

async fn service_snapshot() -> Result<ServiceSnapshot> {
    let mut command = tokio::process::Command::new("systemctl");
    command
        .args([
            "show",
            SERVICE_NAME,
            "--no-pager",
            "--property=LoadState",
            "--property=ActiveState",
            "--property=SubState",
            "--property=Result",
            "--property=ExecMainCode",
            "--property=ExecMainStatus",
            "--property=NRestarts",
            "--property=InvocationID",
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .context("systemd status query timed out")?
        .context("failed to query systemd")?;
    anyhow::ensure!(
        output.status.success(),
        "systemctl show failed: {}",
        server_config::command_error(&output)
    );
    Ok(parse_service_snapshot(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_service_snapshot(output: &str) -> ServiceSnapshot {
    let mut snapshot = ServiceSnapshot::default();
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "LoadState" => snapshot.load_state = value.to_string(),
            "ActiveState" => snapshot.active_state = value.to_string(),
            "SubState" => snapshot.sub_state = value.to_string(),
            "Result" => snapshot.result = value.to_string(),
            "ExecMainCode" => snapshot.exec_main_code = value.parse().ok(),
            "ExecMainStatus" => snapshot.exec_main_status = value.parse().ok(),
            "NRestarts" => snapshot.restart_count = value.parse().ok(),
            "InvocationID" => snapshot.invocation_id = value.to_string(),
            _ => {}
        }
    }
    snapshot
}

fn runtime_from_snapshot(snapshot: &ServiceSnapshot, desired_running: bool) -> RuntimeState {
    match snapshot.active_state.as_str() {
        "active" => RuntimeState::Running,
        "activating" => RuntimeState::Starting,
        "deactivating" => RuntimeState::Stopping,
        "failed" => RuntimeState::Failed,
        "inactive" if desired_running => RuntimeState::Failed,
        "inactive" => RuntimeState::Stopped,
        _ => RuntimeState::Unknown,
    }
}

const fn runtime_from_operation(phase: OperationPhase) -> RuntimeState {
    match phase {
        OperationPhase::Starting => RuntimeState::Starting,
        OperationPhase::Restarting
        | OperationPhase::Validating
        | OperationPhase::Staging
        | OperationPhase::Activating
        | OperationPhase::Verifying
        | OperationPhase::RollingBack
        | OperationPhase::Recovering => RuntimeState::Restarting,
        OperationPhase::Stopping => RuntimeState::Stopping,
        OperationPhase::Importing => RuntimeState::Stopped,
    }
}

fn stable_invocation(first: &ServiceSnapshot, second: &ServiceSnapshot) -> bool {
    (first.invocation_id.is_empty() || first.invocation_id == second.invocation_id)
        && first.restart_count == second.restart_count
}

fn service_failure_issue(
    snapshot: &ServiceSnapshot,
    desired_running: bool,
    operation_visible: bool,
) -> Option<ServerIssue> {
    if !desired_running || operation_visible {
        return None;
    }
    let result = if snapshot.result.is_empty() {
        "unknown"
    } else {
        &snapshot.result
    };
    let mut issue = match snapshot.active_state.as_str() {
        "inactive" => ServerIssue::new(
            "service_not_running",
            "runtime",
            "SnapDog is enabled but not running",
            format!(
                "systemd reports inactive/{}, result {result}. Choose Retry to start SnapDog again, or Stop to keep it disabled.",
                snapshot.sub_state
            ),
        ),
        "failed" => ServerIssue::new(
            "service_failed",
            "startup",
            "SnapDog could not start",
            format!(
                "systemd reports failed/{}, result {result}. Open diagnostics, correct the reported problem, then choose Retry or Stop.",
                snapshot.sub_state
            ),
        ),
        _ => return None,
    };
    issue.exit_code = snapshot.exec_main_status;
    issue.systemd_result = nonempty(snapshot.result.clone());
    Some(issue)
}

fn same_terminal_runtime_issue(first: &ServerIssue, second: &ServerIssue) -> bool {
    first.code == second.code
        && first.detail == second.detail
        && first.exit_code == second.exit_code
        && first.systemd_result == second.systemd_result
}

async fn journal_excerpt() -> Vec<String> {
    let line_limit = MAX_DIAGNOSTIC_LINES.to_string();
    let mut command = tokio::process::Command::new("journalctl");
    command
        .args([
            "--unit",
            SERVICE_NAME,
            "--boot",
            "--no-pager",
            "--output=cat",
            "--lines",
            &line_limit,
        ])
        .kill_on_drop(true);
    match tokio::time::timeout(COMMAND_TIMEOUT, command.output()).await {
        Ok(Ok(output)) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .take(MAX_DIAGNOSTIC_LINES)
            .map(redact_text)
            .collect(),
        Ok(Ok(output)) => vec![redact_text(&format!(
            "journalctl failed: {}",
            server_config::command_error(&output)
        ))],
        Ok(Err(error)) => vec![redact_text(&format!("journalctl unavailable: {error}"))],
        Err(_) => vec!["journalctl timed out".to_string()],
    }
}

fn redact_text(value: &str) -> String {
    value
        .lines()
        .take(MAX_DIAGNOSTIC_LINES)
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            // Logs and parser errors render keys using different separators
            // (`mqtt.password`, `mqtt password`, `username=...`, etc.). Treat
            // every non-alphanumeric byte as a separator before matching so a
            // formatting change cannot accidentally expose credentials.
            let normalized = lower
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character
                    } else {
                        ' '
                    }
                })
                .collect::<String>();
            let words = normalized.split_whitespace().collect::<Vec<_>>();
            let contains_known_secret = KNOWN_SECRETS.read().is_ok_and(|secrets| {
                secrets
                    .iter()
                    .any(|secret| !secret.is_empty() && line.contains(secret))
            });
            let contains_uri_userinfo = lower.split("://").skip(1).any(|remainder| {
                remainder
                    .split(['/', '?', '#'])
                    .next()
                    .is_some_and(|authority| authority.contains('@'))
            });
            let sensitive = lower.contains("password")
                || lower.contains("username")
                || lower.contains("apikey")
                || words.windows(2).any(|pair| pair == ["api", "key"])
                || words.contains(&"authorization")
                || words.contains(&"bearer")
                || words.contains(&"psk")
                || words.contains(&"token")
                || contains_uri_userinfo
                || contains_known_secret;
            if sensitive {
                "[redacted sensitive diagnostic line]".to_string()
            } else {
                line.chars().take(MAX_DIAGNOSTIC_LINE_CHARS).collect()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn register_config_secrets(config: &ServerConfig) {
    let mut discovered = config.http.api_keys.clone();
    discovered.extend(config.snapcast.encryption_psk.iter().cloned());
    discovered.extend(config.airplay.password.iter().cloned());
    if let Some(subsonic) = config.subsonic.as_ref() {
        discovered.push(subsonic.password.clone());
    }
    if let Some(mqtt) = config.mqtt.as_ref() {
        discovered.extend(mqtt.password.iter().cloned());
    }
    discovered.retain(|secret| !secret.is_empty());
    if let Ok(mut known) = KNOWN_SECRETS.write() {
        for secret in discovered {
            if !known.contains(&secret) {
                known.push(secret);
            }
        }
        // The configuration has a small, bounded number of credential fields.
        // Keep a hard cap so repeated rejected drafts cannot grow this forever.
        if known.len() > 128 {
            let excess = known.len() - 128;
            known.drain(..excess);
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn validation_error(error: anyhow::Error) -> ManagerError {
    let detail = format!("{error:#}");
    let mut issue = ServerIssue::new(
        "config_invalid",
        "validation",
        "The SnapDog configuration is invalid",
        &detail,
    );
    issue.field_path = validation_field_path(&detail).map(str::to_string);
    ManagerError {
        kind: ManagerErrorKind::Invalid,
        issue,
    }
}

fn config_guard_error(error: server_config::ConfigGuardError) -> ManagerError {
    match error {
        server_config::ConfigGuardError::Rejected(detail) => validation_error(anyhow::anyhow!(
            "SnapDog rejected the configuration: {detail}"
        )),
        server_config::ConfigGuardError::TimedOut => manager_error(
            ManagerErrorKind::Runtime,
            "validator_timeout",
            "validation",
            "SnapDog's configuration validator timed out",
            "The local validation process did not finish within five seconds",
        ),
        server_config::ConfigGuardError::Unavailable(error) => manager_error(
            ManagerErrorKind::Internal,
            "validator_unavailable",
            "validation",
            "SnapDog's configuration validator is unavailable",
            error,
        ),
    }
}

fn validation_field_path(detail: &str) -> Option<&'static str> {
    let detail = detail.to_ascii_lowercase();
    if detail.contains("sample rate") || detail.contains("sample_rate") {
        Some("audio.sample_rate")
    } else if detail.contains("bit depth") || detail.contains("bit_depth") {
        Some("audio.bit_depth")
    } else if detail.contains("base_url") || detail.contains("base url") {
        Some("http.base_url")
    } else if detail.contains("codec") {
        Some("snapcast.codec")
    } else if detail.contains("channel") {
        Some("audio.channels")
    } else if detail.contains("tls") {
        Some("http.tls_cert")
    } else if detail.contains("streaming port") {
        Some("snapcast.streaming_port")
    } else if detail.contains("json-rpc") && detail.contains("port") {
        Some("snapcast.jsonrpc_tcp_port")
    } else if detail.contains("http") && detail.contains("port") {
        Some("http.port")
    } else if detail.contains("server name") {
        Some("name")
    } else if detail.contains("subsonic") && detail.contains("url") {
        Some("subsonic.url")
    } else if detail.contains("subsonic") && detail.contains("format") {
        Some("subsonic.format")
    } else if detail.contains("spotify") && detail.contains("bitrate") {
        Some("spotify.bitrate")
    } else if detail.contains("spotify") && detail.contains("name") {
        Some("spotify.name")
    } else if detail.contains("mqtt") && detail.contains("broker") {
        Some("mqtt.broker")
    } else if detail.contains("snapcast") && detail.contains("port") {
        Some("snapcast.streaming_port")
    } else if detail.contains("knx") {
        Some("knx")
    } else if detail.contains("zone") {
        Some("zones")
    } else if detail.contains("client") {
        Some("clients")
    } else if detail.contains("radio") {
        Some("radio")
    } else {
        None
    }
}

fn conflict_error() -> ManagerError {
    ManagerError {
        kind: ManagerErrorKind::Conflict,
        issue: ServerIssue::new(
            "config_revision_conflict",
            "validation",
            "The configuration changed while it was being edited",
            "Reload the current configuration before saving again",
        ),
    }
}

fn read_error(error: anyhow::Error) -> ManagerError {
    manager_error(
        ManagerErrorKind::Internal,
        "config_read_failed",
        "configuration",
        "The active configuration could not be read",
        error,
    )
}

async fn read_desired_state() -> std::result::Result<bool, ManagerError> {
    system::service_desired_state("server")
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "desired_state_unreadable",
                "configuration",
                "SnapDog's start preference cannot be read",
                error,
            )
        })
}

async fn recover_before_mutation() -> std::result::Result<(), ManagerError> {
    match tokio::fs::symlink_metadata(server_config::CONFIG_JOURNAL).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(_) => {}
        Err(error) => {
            return Err(manager_error(
                ManagerErrorKind::Internal,
                "transaction_journal_unreadable",
                "recovery",
                "Recovery state cannot be inspected",
                error,
            ));
        }
    }
    let _ = recover_interrupted_transaction().await?;
    match tokio::fs::symlink_metadata(server_config::CONFIG_JOURNAL).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(ManagerError {
            kind: ManagerErrorKind::Conflict,
            issue: ServerIssue::new(
                "recovery_pending",
                "recovery",
                "SnapDog still needs recovery before it can be changed",
                "The recovery journal was preserved to prevent data loss",
            ),
        }),
        Err(error) => Err(manager_error(
            ManagerErrorKind::Internal,
            "transaction_journal_unreadable",
            "recovery",
            "Recovery state cannot be inspected",
            error,
        )),
    }
}

async fn ensure_validation_does_not_touch_recovery() -> std::result::Result<(), ManagerError> {
    match tokio::fs::symlink_metadata(server_config::CONFIG_JOURNAL).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(ManagerError {
            kind: ManagerErrorKind::Conflict,
            issue: ServerIssue::new(
                "recovery_pending",
                "recovery",
                "Validation is paused while SnapDog recovery data is pending",
                "Validation does not modify or execute pending recovery state",
            ),
        }),
        Err(error) => Err(manager_error(
            ManagerErrorKind::Internal,
            "transaction_journal_unreadable",
            "recovery",
            "Recovery state cannot be inspected",
            error,
        )),
    }
}

async fn recover_after_explicit_stop() -> std::result::Result<(), ManagerError> {
    let content = match tokio::fs::read_to_string(server_config::CONFIG_JOURNAL).await {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(content) => content,
        Err(error) => {
            return Err(manager_error(
                ManagerErrorKind::Internal,
                "transaction_journal_unreadable",
                "recovery",
                "Pending recovery state cannot be inspected after stopping SnapDog",
                error,
            ));
        }
    };
    let mut journal: TransactionJournal = serde_json::from_str(&content).map_err(|error| {
        manager_error(
            ManagerErrorKind::Internal,
            "transaction_journal_invalid",
            "recovery",
            "Pending recovery state is invalid",
            error,
        )
    })?;
    journal.desired_running = false;
    journal.previous_desired_running = Some(false);
    write_journal(&journal).await?;
    let _ = recover_interrupted_transaction().await?;
    Ok(())
}

fn manager_error(
    kind: ManagerErrorKind,
    code: &str,
    stage: &str,
    summary: &str,
    error: impl std::fmt::Display,
) -> ManagerError {
    ManagerError {
        kind,
        issue: ServerIssue::new(code, stage, summary, error.to_string()),
    }
}

async fn begin_operation(kind: OperationKind, phase: OperationPhase) -> ServerOperation {
    let sequence = OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let operation = ServerOperation {
        id: format!("server-{}-{sequence}", Utc::now().timestamp_millis()),
        kind,
        phase,
        started_at: Utc::now().to_rfc3339(),
    };
    {
        let mut memory = RUNTIME_MEMORY.write().await;
        RUNTIME_GENERATION.fetch_add(1, Ordering::Release);
        memory.operation = Some(operation.clone());
        memory.issue = None;
        memory.health_state = HealthState::Checking;
        drop(memory);
    }
    broadcast_change();
    operation
}

async fn set_operation_phase(phase: OperationPhase) {
    if let Some(operation) = RUNTIME_MEMORY.write().await.operation.as_mut() {
        operation.phase = phase;
    }
    broadcast_change();
}

async fn remember_verified_revision(revision: &str) {
    let mut memory = RUNTIME_MEMORY.write().await;
    memory.active_revision = Some(revision.to_string());
    memory.health_state = HealthState::Healthy;
}

async fn clear_verified_revision() {
    let mut memory = RUNTIME_MEMORY.write().await;
    memory.active_revision = None;
    memory.health_state = HealthState::Unknown;
}

async fn finish_result(
    result: std::result::Result<Option<String>, ManagerError>,
) -> std::result::Result<ServerState, ManagerError> {
    let (issue_to_persist, clear_previous_issue) = {
        let mut memory = RUNTIME_MEMORY.write().await;
        RUNTIME_GENERATION.fetch_add(1, Ordering::Release);
        let operation_kind = memory.operation.as_ref().map(|operation| operation.kind);
        let preserve_recovery_issue = memory
            .operation
            .as_ref()
            .is_some_and(|operation| operation.kind == OperationKind::Recover)
            && memory.issue.is_some();
        memory.operation = None;
        match &result {
            Ok(active_revision) => {
                memory.active_revision.clone_from(active_revision);
                if !preserve_recovery_issue {
                    memory.issue = None;
                }
                memory.health_state = if active_revision.is_some() {
                    HealthState::Healthy
                } else {
                    HealthState::Unknown
                };
            }
            Err(error) => {
                let operation_preserves_active = matches!(
                    error.issue.stage.as_str(),
                    "validation" | "staging" | "activation" | "stopping" | "finalizing"
                ) || error.issue.rollback_succeeded == Some(true);
                if !operation_preserves_active {
                    memory.active_revision = None;
                }
                memory.issue = Some(error.issue.clone());
                memory.health_state =
                    if operation_preserves_active && memory.active_revision.is_some() {
                        HealthState::Healthy
                    } else {
                        HealthState::Unhealthy
                    };
            }
        }
        let issue_to_persist = match &result {
            Err(error) => Some(error.issue.clone()),
            Ok(_) if preserve_recovery_issue => memory.issue.clone(),
            Ok(_) => None,
        };
        let clear_previous_issue = result.is_ok()
            && operation_kind.is_some_and(|kind| kind != OperationKind::Recover)
            && issue_to_persist.is_none();
        drop(memory);
        (issue_to_persist, clear_previous_issue)
    };
    if let Some(issue) = issue_to_persist {
        if let Err(error) = persist_operation_issue(&issue).await {
            tracing::error!(%error, "failed to persist SnapDog's terminal operation issue");
        }
    } else if clear_previous_issue && let Err(error) = clear_persisted_issue().await {
        tracing::error!(%error, "failed to clear SnapDog's resolved operation issue");
    }
    broadcast_change();
    match result {
        Ok(_) => Ok(server_state().await),
        Err(error) => Err(error),
    }
}

async fn read_active_source() -> Result<Option<String>> {
    match tokio::fs::read_to_string(server_config::CONFIG_PATH).await {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("failed to read active snapdog.toml"),
    }
}

async fn read_revision(path: &str) -> Option<String> {
    tokio::fs::read_to_string(path)
        .await
        .ok()
        .map(|content| server_config::config_revision(&content))
}

async fn read_persisted_issue() -> Option<ServerIssue> {
    let content = tokio::fs::read_to_string(server_config::LAST_OPERATION_ISSUE)
        .await
        .ok()?;
    parse_persisted_issue(&content).ok()
}

fn parse_persisted_issue(content: &str) -> Result<ServerIssue> {
    let mut persisted: PersistedOperationIssue = serde_json::from_str(content)?;
    persisted.issue.summary = redact_text(&persisted.issue.summary);
    persisted.issue.detail = redact_text(&persisted.issue.detail);
    Ok(persisted.issue)
}

async fn persist_operation_issue(issue: &ServerIssue) -> Result<()> {
    let _guard = ISSUE_PERSISTENCE_LOCK.lock().await;
    persist_operation_issue_unlocked(issue).await
}

async fn persist_operation_issue_unlocked(issue: &ServerIssue) -> Result<()> {
    let content = serialize_persisted_issue(issue)?;
    server_config::durable_atomic_write(server_config::LAST_OPERATION_ISSUE, &content).await
}

async fn persist_runtime_issue_if_unchanged(
    issue: &ServerIssue,
    expected_generation: u64,
) -> Result<bool> {
    let _guard = ISSUE_PERSISTENCE_LOCK.lock().await;
    if RUNTIME_GENERATION.load(Ordering::Acquire) != expected_generation {
        return Ok(false);
    }
    persist_operation_issue_unlocked(issue).await?;
    Ok(true)
}

fn serialize_persisted_issue(issue: &ServerIssue) -> Result<String> {
    let mut issue = issue.clone();
    issue.summary = redact_text(&issue.summary);
    issue.detail = redact_text(&issue.detail);
    Ok(serde_json::to_string_pretty(&PersistedOperationIssue {
        recorded_at: Utc::now().to_rfc3339(),
        issue,
    })?)
}

async fn clear_persisted_issue() -> Result<()> {
    let _guard = ISSUE_PERSISTENCE_LOCK.lock().await;
    clear_persisted_issue_unlocked().await
}

async fn clear_runtime_issue_if_unchanged(expected_generation: u64) -> Result<bool> {
    let _guard = ISSUE_PERSISTENCE_LOCK.lock().await;
    if RUNTIME_GENERATION.load(Ordering::Acquire) != expected_generation {
        return Ok(false);
    }
    clear_persisted_issue_unlocked().await?;
    Ok(true)
}

async fn clear_persisted_issue_unlocked() -> Result<()> {
    match tokio::fs::remove_file(server_config::LAST_OPERATION_ISSUE).await {
        Ok(()) => sync_data_directory().await,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn write_journal(journal: &TransactionJournal) -> std::result::Result<(), ManagerError> {
    let content = serde_json::to_string_pretty(journal).map_err(|error| {
        manager_error(
            ManagerErrorKind::Internal,
            "transaction_journal_encode_failed",
            "staging",
            "The configuration transaction could not be recorded",
            error,
        )
    })?;
    server_config::durable_atomic_write(server_config::CONFIG_JOURNAL, &content)
        .await
        .map_err(|error| {
            manager_error(
                ManagerErrorKind::Internal,
                "transaction_journal_write_failed",
                "staging",
                "The configuration transaction could not be recorded",
                error,
            )
        })
}

async fn sync_data_directory() -> Result<()> {
    #[cfg(unix)]
    tokio::fs::File::open("/data/snapdog")
        .await?
        .sync_all()
        .await?;
    Ok(())
}

async fn remove_file_if_present(path: &str) {
    match tokio::fs::remove_file(path).await {
        Ok(()) =>
        {
            #[cfg(unix)]
            if let Some(parent) = Path::new(path).parent() {
                let sync_result = match tokio::fs::File::open(parent).await {
                    Ok(directory) => directory.sync_all().await,
                    Err(error) => Err(error),
                };
                if let Err(error) = sync_result {
                    tracing::warn!(%error, %path, "failed to sync removed server transaction artifact");
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(%error, %path, "failed to remove server transaction artifact");
        }
    }
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit_once('\n').map_or_else(
        || prefix.chars().count() + 1,
        |(_, tail)| tail.chars().count() + 1,
    );
    (line, column)
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recovery_journal(kind: OperationKind, phase: OperationPhase) -> TransactionJournal {
        TransactionJournal {
            operation_id: "test-operation".to_string(),
            kind,
            phase,
            previous_existed: true,
            previous_revision: Some(server_config::config_revision("previous")),
            candidate_revision: server_config::config_revision("candidate"),
            desired_running: true,
            previous_desired_running: Some(false),
        }
    }

    #[test]
    fn parses_systemd_status_needed_for_truthful_runtime_state() {
        let snapshot = parse_service_snapshot(
            "LoadState=loaded\nActiveState=failed\nSubState=failed\nResult=exit-code\n\
             ExecMainCode=1\nExecMainStatus=78\nNRestarts=3\nInvocationID=abc\n",
        );
        assert_eq!(snapshot.active_state, "failed");
        assert_eq!(snapshot.result, "exit-code");
        assert_eq!(snapshot.exec_main_status, Some(78));
        assert_eq!(snapshot.restart_count, Some(3));
        assert_eq!(runtime_from_snapshot(&snapshot, true), RuntimeState::Failed);
    }

    #[test]
    fn desired_running_inactive_success_is_a_persistent_actionable_failure() {
        let snapshot = parse_service_snapshot(
            "LoadState=loaded\nActiveState=inactive\nSubState=dead\nResult=success\n\
             ExecMainCode=0\nExecMainStatus=0\nNRestarts=0\nInvocationID=\n",
        );
        assert_eq!(runtime_from_snapshot(&snapshot, true), RuntimeState::Failed);
        assert_eq!(
            runtime_from_snapshot(&snapshot, false),
            RuntimeState::Stopped
        );

        let issue = service_failure_issue(&snapshot, true, false).unwrap();
        assert_eq!(issue.code, "service_not_running");
        assert!(issue.detail.contains("Choose Retry"));
        assert!(issue.detail.contains("or Stop"));
        let encoded = serialize_persisted_issue(&issue).unwrap();
        let restored = parse_persisted_issue(&encoded).unwrap();
        assert!(same_terminal_runtime_issue(&issue, &restored));

        assert!(service_failure_issue(&snapshot, false, false).is_none());
        assert!(service_failure_issue(&snapshot, true, true).is_none());
    }

    #[test]
    fn state_issue_mutation_requires_a_stable_epoch_without_an_authoritative_issue() {
        use std::sync::{Arc, Barrier};

        let generation = Arc::new(AtomicU64::new(41));
        let observation = RuntimeObservation {
            generation: generation.load(Ordering::Acquire),
        };
        let barrier = Arc::new(Barrier::new(2));
        let writer_generation = Arc::clone(&generation);
        let writer_barrier = Arc::clone(&barrier);
        let writer = std::thread::spawn(move || {
            writer_barrier.wait();
            writer_generation.fetch_add(2, Ordering::Release);
        });

        // Model a complete operation between the poll's external systemd read
        // and its RuntimeMemory snapshot. Joining makes the interleaving fully
        // deterministic while retaining the real atomic hand-off.
        barrier.wait();
        writer.join().unwrap();
        let current_generation = generation.load(Ordering::Acquire);
        let mut memory = RuntimeMemory::new();
        assert!(!observation.permits_state_issue_mutation(current_generation, &memory));

        let current_observation = RuntimeObservation {
            generation: current_generation,
        };
        assert!(current_observation.permits_state_issue_mutation(current_generation, &memory));

        memory.issue = Some(ServerIssue::new(
            "systemd_job_failed",
            "startup",
            "SnapDog could not start",
            "the authoritative terminal detail",
        ));
        assert!(!current_observation.permits_state_issue_mutation(current_generation, &memory));

        memory.issue = None;
        memory.operation = Some(ServerOperation {
            id: "server-test-operation".to_string(),
            kind: OperationKind::Start,
            phase: OperationPhase::Starting,
            started_at: "2026-01-01T00:00:00Z".to_string(),
        });
        assert!(!current_observation.permits_state_issue_mutation(current_generation, &memory));
    }

    #[test]
    fn every_mutation_phase_has_an_actionable_cancellation_terminal() {
        for (kind, phase) in [
            (OperationKind::Setup, OperationPhase::Validating),
            (OperationKind::Apply, OperationPhase::Staging),
            (OperationKind::Apply, OperationPhase::Activating),
            (OperationKind::Start, OperationPhase::Starting),
            (OperationKind::Restart, OperationPhase::Restarting),
            (OperationKind::Retry, OperationPhase::Verifying),
            (OperationKind::Stop, OperationPhase::Stopping),
            (OperationKind::SettingsImport, OperationPhase::Importing),
            (OperationKind::Apply, OperationPhase::RollingBack),
        ] {
            let operation = ServerOperation {
                id: "cancelled-test-operation".to_string(),
                kind,
                phase,
                started_at: "2026-01-01T00:00:00Z".to_string(),
            };
            let terminal = interrupted_operation_error(&operation, phase, None);
            assert_eq!(terminal.issue.code, "operation_interrupted_recovered");
            assert_eq!(terminal.issue.rollback_succeeded, Some(true));
            assert!(terminal.issue.detail.contains(operation_kind_name(kind)));
            assert!(terminal.issue.detail.contains(operation_phase_name(phase)));
            assert!(terminal.issue.detail.contains("retry"));
        }

        let operation = ServerOperation {
            id: "failed-recovery-test-operation".to_string(),
            kind: OperationKind::SettingsImport,
            phase: OperationPhase::Recovering,
            started_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let recovery_error = manager_error(
            ManagerErrorKind::Runtime,
            "test_recovery_failure",
            "recovery",
            "Recovery failed",
            "systemd unavailable",
        );
        let terminal = interrupted_operation_error(
            &operation,
            OperationPhase::Importing,
            Some(&recovery_error),
        );
        assert_eq!(terminal.issue.code, "operation_interrupted_recovery_failed");
        assert_eq!(terminal.issue.rollback_succeeded, Some(false));
        assert!(terminal.issue.detail.contains("Retry or Stop"));
    }

    #[test]
    fn accepted_reboot_retains_its_lease_without_a_drop_window() {
        struct DropProbe(std::sync::Arc<std::sync::atomic::AtomicBool>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        retain_until_process_exit(DropProbe(std::sync::Arc::clone(&dropped)));
        assert!(!dropped.load(Ordering::Acquire));
    }

    #[test]
    fn incomplete_settings_rollback_requires_process_exit_lease_retention() {
        let pending = manager_error(
            ManagerErrorKind::Runtime,
            "settings_import_rollback_pending",
            "rollback",
            "Settings rollback is pending",
            "simulated durable rollback failure",
        );
        assert!(cancellation_recovery_requires_process_exit(&pending));

        let ordinary = manager_error(
            ManagerErrorKind::Runtime,
            "systemd_status_failed",
            "recovery",
            "Status failed",
            "simulated systemd failure",
        );
        assert!(!cancellation_recovery_requires_process_exit(&ordinary));
    }

    #[test]
    fn cancellation_reconciliation_follows_the_persisted_desired_state() {
        assert_eq!(reconciliation_target(true), ReconciliationTarget::Running);
        assert_eq!(reconciliation_target(false), ReconciliationTarget::Stopped);
        assert_eq!(
            cancellation_recovery(OperationKind::Stop),
            CancellationRecovery::CompleteExplicitStop
        );
        assert_eq!(
            cancellation_recovery(OperationKind::SettingsImport),
            CancellationRecovery::RestoreSettingsThenReconcile
        );
        for kind in [
            OperationKind::Setup,
            OperationKind::Apply,
            OperationKind::Start,
            OperationKind::Restart,
            OperationKind::Retry,
            OperationKind::Recover,
        ] {
            assert_eq!(
                cancellation_recovery(kind),
                CancellationRecovery::ReconcileDesired
            );
        }
    }

    #[test]
    fn readiness_host_never_probes_a_wildcard_address() {
        assert_eq!(readiness_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(readiness_host("::"), "127.0.0.1");
        assert_eq!(readiness_host("::1"), "[::1]");
        assert_eq!(readiness_host("192.168.1.10"), "192.168.1.10");
    }

    #[test]
    fn diagnostics_redact_credentials_and_limit_line_length() {
        assert_eq!(
            redact_text("Authorization: Bearer secret"),
            "[redacted sensitive diagnostic line]"
        );
        assert_eq!(
            redact_text("mqtt.password = supersecret"),
            "[redacted sensitive diagnostic line]"
        );
        assert_eq!(
            redact_text("subsonic username=user"),
            "[redacted sensitive diagnostic line]"
        );
        for sensitive in [
            "X-Api-Key: top-secret",
            "apikey=top-secret",
            "psk=top-secret",
            "request http://user:password@example.test/failed",
            "request https://example.test/callback?token=top-secret",
        ] {
            assert_eq!(
                redact_text(sensitive),
                "[redacted sensitive diagnostic line]",
                "{sensitive}"
            );
        }
        let mut config = server_config::default_server_config();
        config.http.api_keys = vec!["opaque-value-without-a-label".to_string()];
        register_config_secrets(&config);
        assert_eq!(
            redact_text("guard echoed opaque-value-without-a-label"),
            "[redacted sensitive diagnostic line]"
        );
        assert_eq!(
            redact_text(&"x".repeat(800)).len(),
            MAX_DIAGNOSTIC_LINE_CHARS
        );
    }

    #[test]
    fn state_endpoint_never_exposes_credential_bearing_urls() {
        let mut config = server_config::default_server_config();
        config.http.base_url = "https://user:secret@example.test?token=secret".to_string();
        assert!(config_endpoint(&config).is_none());
        config.http.base_url = "https://example.test/snapdog".to_string();
        assert_eq!(
            config_endpoint(&config).as_deref(),
            Some("https://example.test/snapdog")
        );
    }

    #[test]
    fn toml_offsets_are_reported_as_one_based_line_and_column() {
        assert_eq!(line_column("first\nsecond\n", 8), (2, 3));
    }

    #[test]
    fn streaming_port_conflict_targets_the_editable_streaming_field() {
        assert_eq!(
            validation_field_path("HTTP and Snapcast streaming ports must be different"),
            Some("snapcast.streaming_port")
        );
    }

    #[test]
    fn public_base_url_validation_targets_the_editable_http_field() {
        for detail in [
            "invalid base_url: expected an absolute URL",
            "Public base URL must not contain credentials",
        ] {
            assert_eq!(validation_field_path(detail), Some("http.base_url"));
        }
    }

    #[test]
    fn transaction_paths_never_target_the_read_only_etc_symlink() {
        for path in [
            server_config::CONFIG_PATH,
            server_config::CONFIG_BACKUP,
            server_config::CONFIG_CANDIDATE,
            server_config::CONFIG_LAST_GOOD,
            server_config::CONFIG_JOURNAL,
            server_config::LAST_OPERATION_ISSUE,
        ] {
            assert!(path.starts_with("/data/snapdog/"), "{path}");
        }
    }

    #[test]
    fn transaction_file_modes_keep_secrets_private_and_candidate_readable() {
        assert_eq!(
            server_config::server_file_mode(server_config::CONFIG_PATH),
            0o640
        );
        assert_eq!(
            server_config::server_file_mode(server_config::CONFIG_CANDIDATE),
            0o640
        );
        for path in [
            server_config::CONFIG_BACKUP,
            server_config::CONFIG_LAST_GOOD,
            server_config::CONFIG_JOURNAL,
            server_config::LAST_OPERATION_ISSUE,
        ] {
            assert_eq!(server_config::server_file_mode(path), 0o600, "{path}");
        }
    }

    #[test]
    fn terminal_issue_survives_restart_in_redacted_form() {
        let mut config = server_config::default_server_config();
        config.http.api_keys = vec!["persisted-secret-value".to_string()];
        register_config_secrets(&config);
        let issue = ServerIssue::new(
            "readiness_timeout",
            "verification",
            "SnapDog did not become ready",
            "process echoed persisted-secret-value",
        );
        let encoded = serialize_persisted_issue(&issue).unwrap();
        assert!(!encoded.contains("persisted-secret-value"));
        let restored = parse_persisted_issue(&encoded).unwrap();
        assert_eq!(restored.code, "readiness_timeout");
        assert_eq!(restored.detail, "[redacted sensitive diagnostic line]");
        assert_eq!(
            server_config::server_file_mode(server_config::LAST_OPERATION_ISSUE),
            0o600
        );
    }

    #[test]
    fn environment_preflight_confines_state_and_requires_a_tls_pair() {
        assert!(state_dir_is_managed("/data/snapdog/state"));
        assert!(state_dir_is_managed("/data/snapdog/state/instance-a"));
        assert!(!state_dir_is_managed("/data/snapdog/state-other"));
        assert!(!state_dir_is_managed("/data/snapdog/state/../private"));
        assert!(!state_dir_is_managed("/tmp/snapdog"));

        let mut config = server_config::default_server_config();
        assert!(configured_tls_paths(&config).unwrap().is_none());
        config.http.tls_cert = Some("/data/snapdog/tls/server.crt".to_string());
        assert!(configured_tls_paths(&config).is_err());
        config.http.tls_key = Some("/data/snapdog/tls/server.key".to_string());
        assert_eq!(
            configured_tls_paths(&config).unwrap(),
            Some((
                "/data/snapdog/tls/server.crt",
                "/data/snapdog/tls/server.key"
            ))
        );
        assert!(!managed_file_permissions_allow(
            ManagedPathKind::ReadableFile,
            true,
            0o600
        ));
        assert!(managed_file_permissions_allow(
            ManagedPathKind::ReadableFile,
            true,
            0o640
        ));
        assert!(!managed_file_permissions_allow(
            ManagedPathKind::ReadableFile,
            false,
            0o644
        ));
    }

    #[test]
    fn recovery_uses_revisions_instead_of_trusting_the_last_recorded_phase() {
        let journal = recovery_journal(OperationKind::Setup, OperationPhase::Staging);
        assert_eq!(
            decide_recovery(&journal, Some("previous"), None),
            RecoveryDecision::ActivationNotApplied
        );
        // This is the old crash window: even a stale Staging phase cannot make
        // recovery discard a candidate that is already active.
        assert_eq!(
            decide_recovery(&journal, Some("candidate"), None),
            RecoveryDecision::CandidateNeedsVerification
        );
        assert_eq!(
            decide_recovery(&journal, Some("candidate"), Some("candidate")),
            RecoveryDecision::CandidateCommitted
        );
        assert_eq!(
            decide_recovery(&journal, Some("unrelated"), None),
            RecoveryDecision::Conflict
        );
        let rolling_back = recovery_journal(OperationKind::Apply, OperationPhase::RollingBack);
        assert_eq!(
            decide_recovery(&rolling_back, Some("last-good"), Some("last-good")),
            RecoveryDecision::RollbackCompleted
        );
    }

    #[test]
    fn rollback_restores_setup_intent_but_preserves_apply_intent() {
        let setup = recovery_journal(OperationKind::Setup, OperationPhase::Activating);
        assert!(!desired_after_rollback(&setup));
        let apply = recovery_journal(OperationKind::Apply, OperationPhase::Activating);
        assert!(desired_after_rollback(&apply));
    }

    #[test]
    fn verifying_is_a_durable_journal_phase() {
        let journal = recovery_journal(OperationKind::Apply, OperationPhase::Verifying);
        let encoded = serde_json::to_string(&journal).unwrap();
        assert!(encoded.contains("\"phase\":\"verifying\""));
    }

    #[test]
    fn validation_never_rebases_the_followup_apply_revision() {
        let response = successful_validation_response();
        assert!(response.valid);
        assert!(response.config.is_none());
    }
}
