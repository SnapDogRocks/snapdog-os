// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use thiserror::Error;

#[derive(Error, Debug)]
pub enum UpgradeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP client error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Authentication failed (check credentials)")]
    Unauthorized,

    #[error(
        "Target board compatibility mismatch: target is '{target}' but image compatible with '{image}'"
    )]
    IncompatibleBoard { target: String, image: String },

    #[error("Preflight health warning: target system reports critical warnings: {0:?}")]
    SystemUnhealthy(Vec<String>),

    #[error("Upload failed with status: {0}")]
    UploadFailed(reqwest::StatusCode),

    #[error("Upgrade failed: {0}")]
    Failed(String),

    #[error("Flashing request expired or challenge rejected")]
    ChallengeRejected,

    #[error("Operation timed out: {0}")]
    Timeout(String),
}

pub type Result<T> = std::result::Result<T, UpgradeError>;
