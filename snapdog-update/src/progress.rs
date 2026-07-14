// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub struct ProgressUi {
    pb: ProgressBar,
}

impl ProgressUi {
    pub fn new_upload(total_bytes: u64, message: &'static str, enabled: bool) -> Self {
        let pb = if enabled {
            ProgressBar::new(total_bytes)
        } else {
            ProgressBar::hidden()
        };
        if enabled
            && let Ok(style) = ProgressStyle::default_bar()
                .template("{spinner:.green} {msg} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            {
                pb.set_style(style.progress_chars("#>-"));
            }
        pb.set_message(message);
        Self { pb }
    }

    pub fn new_spinner(message: &'static str, enabled: bool) -> Self {
        let pb = if enabled {
            ProgressBar::new_spinner()
        } else {
            ProgressBar::hidden()
        };
        if enabled {
            pb.enable_steady_tick(Duration::from_millis(80));
            if let Ok(style) = ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg} [{elapsed_precise}]")
            {
                pb.set_style(style);
            }
        }
        pb.set_message(message);
        Self { pb }
    }

    pub fn set_position(&self, pos: u64) {
        self.pb.set_position(pos);
    }

    pub fn update_message(&self, msg: String) {
        self.pb.set_message(msg);
    }

    pub fn finish_success(&self, msg: &str) {
        self.pb.finish_with_message(msg.to_string());
    }

    pub fn finish_failure(&self, msg: &str) {
        self.pb.finish_with_message(msg.to_string());
    }
}
