// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../.git/HEAD");

    let git_version = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || env!("CARGO_PKG_VERSION").to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );
    println!("cargo:rustc-env=SNAPDOG_UPDATE_VERSION={git_version}");
}
