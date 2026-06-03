// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use super::TuningConfig;
use anyhow::Result;

pub struct GenericTuningDriver;

impl GenericTuningDriver {
    pub const fn new() -> Self {
        Self
    }

    #[allow(clippy::unused_async)]
    pub async fn get_config(&self) -> Result<TuningConfig> {
        // Fallback returns a safe default configuration (all features disabled)
        Ok(TuningConfig {
            rf_kill_wifi: false,
            rf_kill_bluetooth: false,
            disable_onboard_audio: false,
            exclusive_audio_core: false,
        })
    }

    #[allow(clippy::unused_async)]
    pub async fn set_config(&self, _config: &TuningConfig) -> Result<()> {
        // Fallback is a no-op on non-supported boards
        tracing::warn!("Hardware tuning is not supported on this platform.");
        Ok(())
    }
}
