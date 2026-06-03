// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub mod generic;
pub mod rpi;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct TuningConfig {
    pub rf_kill_wifi: bool,
    pub rf_kill_bluetooth: bool,
    pub disable_onboard_audio: bool,
    pub exclusive_audio_core: bool,
}

pub enum ActiveTuningDriver {
    Rpi(rpi::RpiTuningDriver),
    Generic(generic::GenericTuningDriver),
}

impl ActiveTuningDriver {
    pub async fn get_config(&self) -> Result<TuningConfig> {
        match self {
            Self::Rpi(d) => d.get_config().await,
            Self::Generic(d) => d.get_config().await,
        }
    }

    pub async fn set_config(&self, config: &TuningConfig) -> Result<()> {
        match self {
            Self::Rpi(d) => d.set_config(config).await,
            Self::Generic(d) => d.set_config(config).await,
        }
    }
}

/// Dynamically returns the active hardware tuning driver based on board detection.
pub async fn get_active_driver() -> ActiveTuningDriver {
    let board = crate::system::detect_board().await;
    if board.contains("pi") {
        ActiveTuningDriver::Rpi(rpi::RpiTuningDriver::new())
    } else {
        ActiveTuningDriver::Generic(generic::GenericTuningDriver::new())
    }
}
