// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use crate::error::{Result, UpgradeError};
use crate::progress::ProgressUi;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug, Clone)]
pub struct Reporter {
    inner: Arc<ReporterInner>,
}

#[derive(Debug)]
struct ReporterInner {
    format: OutputFormat,
    progress_enabled: bool,
    interactive: bool,
    output_lock: Mutex<()>,
}

#[derive(Serialize)]
struct Event<'a> {
    kind: &'a str,
    phase: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes_sent: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    percent_basis_points: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'a str>,
}

impl<'a> Event<'a> {
    const fn new(kind: &'a str, phase: &'a str, message: &'a str) -> Self {
        Self {
            kind,
            phase,
            message,
            bytes_sent: None,
            total_bytes: None,
            percent_basis_points: None,
            challenge: None,
            expires_in_seconds: None,
            exit_code: None,
            error_code: None,
            hint: None,
        }
    }
}

impl Reporter {
    pub fn new(format: OutputFormat, no_progress: bool, non_interactive: bool) -> Self {
        let progress_enabled =
            format == OutputFormat::Human && !no_progress && io::stderr().is_terminal();
        let interactive =
            format == OutputFormat::Human && !non_interactive && io::stdin().is_terminal();
        Self {
            inner: Arc::new(ReporterInner {
                format,
                progress_enabled,
                interactive,
                output_lock: Mutex::new(()),
            }),
        }
    }

    pub fn format(&self) -> OutputFormat {
        self.inner.format
    }

    pub fn interactive(&self) -> bool {
        self.inner.interactive
    }

    pub fn status(&self, phase: &'static str, message: impl AsRef<str>) {
        let message = message.as_ref();
        if self.inner.format == OutputFormat::Json {
            self.emit_json(&Event::new("status", phase, message));
        } else {
            eprintln!("{message}");
        }
    }

    pub fn success(&self, phase: &'static str, message: impl AsRef<str>) {
        let message = message.as_ref();
        if self.inner.format == OutputFormat::Json {
            let mut event = Event::new("success", phase, message);
            event.exit_code = Some(0);
            self.emit_json(&event);
        } else {
            eprintln!("{message}");
        }
    }

    pub fn error(&self, error: &UpgradeError) {
        if self.inner.format == OutputFormat::Json {
            let message = error.to_string();
            let mut event = Event::new("error", "error", &message);
            event.exit_code = Some(1);
            event.error_code = Some(error.code());
            event.hint = error.hint();
            self.emit_json(&event);
        } else {
            eprintln!("error: {error}");
            if let Some(hint) = error.hint() {
                eprintln!("hint: {hint}");
            }
        }
    }

    pub fn raw_flash_challenge(&self, challenge: &str, expires_in_seconds: u64) {
        if self.inner.format == OutputFormat::Json {
            let mut event = Event::new(
                "confirmation_required",
                "raw_flash",
                "Raw flash upload is pending explicit confirmation",
            );
            event.challenge = Some(challenge);
            event.expires_in_seconds = Some(expires_in_seconds);
            event.exit_code = Some(2);
            event.hint =
                Some("Rerun with --raw --confirm-raw-flash <challenge> before it expires.");
            self.emit_json(&event);
        } else {
            eprintln!();
            eprintln!("Raw flash upload is pending confirmation.");
            eprintln!("This will overwrite the inactive root partition.");
            eprintln!("Challenge: {challenge}");
            eprintln!("Expires in: {expires_in_seconds}s");
            eprintln!();
            eprintln!("To confirm from another shell:");
            eprintln!("  snapdog-update --url <device-url> --raw --confirm-raw-flash {challenge}");
            eprintln!();
        }
    }

    pub fn upload_progress(&self, total_bytes: u64, message: &'static str) -> UploadProgress {
        self.status("upload", message);
        UploadProgress {
            ui: ProgressUi::new_upload(total_bytes, message, self.inner.progress_enabled),
            reporter: self.clone(),
            total_bytes,
            last_reported: Mutex::new(0),
        }
    }

    pub fn spinner(&self, phase: &'static str, message: &'static str) -> SpinnerProgress {
        self.status(phase, message);
        SpinnerProgress {
            ui: ProgressUi::new_spinner(message, self.inner.progress_enabled),
            reporter: self.clone(),
            phase,
        }
    }

    pub async fn prompt_raw_flash_confirmation(&self, challenge: &str) -> Result<Option<String>> {
        if !self.interactive() {
            return Ok(None);
        }

        let challenge = challenge.to_string();
        tokio::task::spawn_blocking(move || {
            eprint!("Type challenge {challenge} to confirm raw flash, or press Enter to stop: ");
            io::stderr().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let trimmed = input.trim().to_string();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        })
        .await
        .map_err(|_| UpgradeError::Input("failed to read raw flash confirmation".to_string()))?
    }

    fn progress(&self, phase: &'static str, bytes_sent: u64, total_bytes: u64) {
        if self.inner.format != OutputFormat::Json {
            return;
        }

        let percent_basis_points = if total_bytes == 0 {
            10_000
        } else {
            u64::try_from((u128::from(bytes_sent) * 10_000) / u128::from(total_bytes))
                .unwrap_or(10_000)
        };

        let mut event = Event::new("progress", phase, "Upload progress");
        event.bytes_sent = Some(bytes_sent);
        event.total_bytes = Some(total_bytes);
        event.percent_basis_points = Some(percent_basis_points);
        self.emit_json(&event);
    }

    fn emit_json(&self, event: &Event<'_>) {
        let Ok(_guard) = self.inner.output_lock.lock() else {
            return;
        };
        let mut stdout = io::stdout().lock();
        if serde_json::to_writer(&mut stdout, event).is_ok() {
            let _ = writeln!(stdout);
        }
    }
}

pub struct UploadProgress {
    ui: ProgressUi,
    reporter: Reporter,
    total_bytes: u64,
    last_reported: Mutex<u64>,
}

impl UploadProgress {
    pub fn set_position(&self, pos: u64) {
        self.ui.set_position(pos);

        let min_delta = (self.total_bytes / 100).max(1_048_576);
        let Ok(mut last_reported) = self.last_reported.lock() else {
            return;
        };
        if pos >= self.total_bytes || pos.saturating_sub(*last_reported) >= min_delta {
            *last_reported = pos;
            self.reporter.progress("upload", pos, self.total_bytes);
        }
    }

    pub fn finish_success(&self, message: &str) {
        self.ui.finish_success(message);
        self.reporter.success("upload", message);
    }

    pub fn finish_failure(&self, message: &str) {
        self.ui.finish_failure(message);
        self.reporter.status("upload", message);
    }
}

pub struct SpinnerProgress {
    ui: ProgressUi,
    reporter: Reporter,
    phase: &'static str,
}

impl SpinnerProgress {
    pub fn update_message(&self, message: String) {
        self.ui.update_message(message.clone());
        self.reporter.status(self.phase, message);
    }

    pub fn finish_success(&self, message: &str) {
        self.ui.finish_success(message);
        self.reporter.success(self.phase, message);
    }

    pub fn finish_failure(&self, message: &str) {
        self.ui.finish_failure(message);
        self.reporter.status(self.phase, message);
    }
}
