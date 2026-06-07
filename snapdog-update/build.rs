// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

fn main() {
    println!("cargo:rerun-if-env-changed=SNAPDOG_UPDATE_VERSION");

    let version = std::env::var("SNAPDOG_UPDATE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=SNAPDOG_UPDATE_VERSION={version}");
}
