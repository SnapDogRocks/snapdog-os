// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use thiserror::Error;

#[derive(Error, Debug)]
pub enum UpgradeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP client error: {}", describe_http_error(.0))]
    Http(#[from] reqwest::Error),

    #[error("invalid SnapDog URL: {0}")]
    InvalidBaseUrl(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("{input} is required in non-interactive mode")]
    NonInteractiveInputRequired {
        input: &'static str,
        hint: &'static str,
    },

    #[error("authentication token returned by target cannot be sent as an HTTP header")]
    InvalidAuthToken,

    #[error("Authentication failed (check credentials)")]
    Unauthorized,

    #[error(
        "Target board compatibility mismatch: target is '{target}' but bundle is built for '{bundle}'"
    )]
    IncompatibleBoard { target: String, bundle: String },

    #[error("Preflight health warning: target system reports critical warnings: {0:?}")]
    SystemUnhealthy(Vec<String>),

    #[error("{action} failed with status {status}: {body}")]
    HttpStatus {
        action: &'static str,
        status: reqwest::StatusCode,
        body: String,
    },

    #[error("unsupported firmware file '{path}'")]
    UnsupportedFirmwareFile { path: String },

    #[error("Upgrade failed: {0}")]
    Failed(String),

    #[error("Operation timed out: {0}")]
    Timeout(String),
}

pub type Result<T> = std::result::Result<T, UpgradeError>;

/// reqwest's Display for a transport failure is opaque — e.g. "error sending
/// request for url (X)" with no hint at the real cause. Walk the error's source
/// chain and append the deepest cause (typically the OS error, like
/// "No route to host (os error 65)") so the user sees what actually failed.
fn describe_http_error(err: &reqwest::Error) -> String {
    let mut msg = err.to_string();
    let mut deepest: Option<String> = None;
    let mut source = std::error::Error::source(err);
    while let Some(s) = source {
        deepest = Some(s.to_string());
        source = s.source();
    }
    if let Some(cause) = deepest
        && !msg.contains(cause.as_str())
    {
        use std::fmt::Write as _;
        let _ = write!(msg, ": {cause}");
    }
    msg
}

impl UpgradeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io_error",
            Self::Http(_) | Self::HttpStatus { .. } => "http_error",
            Self::InvalidBaseUrl(_) => "invalid_base_url",
            Self::InvalidArgument(_) => "invalid_argument",
            Self::NonInteractiveInputRequired { .. } => "input_required",
            Self::InvalidAuthToken => "invalid_auth_token",
            Self::Unauthorized => "unauthorized",
            Self::IncompatibleBoard { .. } => "incompatible_board",
            Self::SystemUnhealthy(_) => "system_unhealthy",
            Self::UnsupportedFirmwareFile { .. } => "unsupported_firmware_file",
            Self::Failed(_) => "upgrade_failed",
            Self::Timeout(_) => "timeout",
        }
    }

    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::Http(e) if e.is_connect() => Some(
                "Could not reach the target. Check the URL and that the device is online. If you \
                 recently changed VPN/Wi-Fi, a stale neighbor-cache entry can cause \"no route to \
                 host\" — flush it (macOS: `sudo arp -d <device-ip>`) or bounce the interface.",
            ),
            Self::Http(e) if e.is_timeout() => {
                Some("The target did not respond in time. Check device power and connectivity.")
            }
            Self::InvalidBaseUrl(_) => Some("Use an absolute URL such as http://snapdog.local."),
            Self::NonInteractiveInputRequired { hint, .. } => Some(hint),
            Self::Unauthorized => {
                Some("Pass --password, set SNAPDOG_PASSWORD, or login through the control UI.")
            }
            Self::InvalidAuthToken => {
                Some("Retry authentication; if this repeats, update the target control service.")
            }
            Self::IncompatibleBoard { .. } => {
                Some("Download a signed RAUC bundle built for the target board model.")
            }
            Self::SystemUnhealthy(_) => {
                Some("Resolve critical health warnings on the target before updating.")
            }
            Self::UnsupportedFirmwareFile { .. } => Some("Use a signed .raucb firmware bundle."),
            Self::Timeout(_) => Some(
                "Check device power, network connectivity, and whether the update is still running.",
            ),
            _ => None,
        }
    }
}
