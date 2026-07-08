// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=webui/src");
    println!("cargo:rerun-if-changed=webui/messages");
    println!("cargo:rerun-if-changed=webui/public");
    println!("cargo:rerun-if-changed=webui/package.json");
    println!("cargo:rerun-if-changed=webui/next.config.ts");
    println!("cargo:rerun-if-changed=../.git/HEAD");

    // Version, aligned with the release-please flow. release-please owns the
    // authoritative version in Cargo.toml (CARGO_PKG_VERSION) and tags each ctrl
    // release `snapdog-ctrl-v<x.y.z>`. We `git describe` restricted to that tag
    // prefix so dev builds get a `<x.y.z>-<n>-g<sha>` suffix off the *ctrl* tag —
    // NOT the repo's OS-level `v<x.y.z>` tags, which a bare `--tags` would pick by
    // commit distance and mislabel ctrl with the OS version. The `snapdog-ctrl-v`
    // prefix is stripped for display; fall back to CARGO_PKG_VERSION when git is
    // unavailable (e.g. the release build container) or no matching tag exists.
    let git_version = Command::new("git")
        .args([
            "describe",
            "--tags",
            "--match",
            "snapdog-ctrl-v*",
            "--dirty",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|v| !v.is_empty())
        .map_or_else(
            || env!("CARGO_PKG_VERSION").to_string(),
            |v| v.trim_start_matches("snapdog-ctrl-v").to_string(),
        );
    println!("cargo:rustc-env=SNAPDOG_CTRL_VERSION={git_version}");

    let webui_dir = std::path::Path::new("webui");

    // Install deps if needed
    if !webui_dir.join("node_modules").exists() {
        let status = Command::new("npm")
            .args(["ci", "--prefer-offline"])
            .current_dir(webui_dir)
            .status()
            .expect("failed to run npm ci — is Node.js installed?");
        assert!(status.success(), "npm ci failed");
    }

    // Build static export
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(webui_dir)
        .status()
        .expect("failed to run npm run build");
    assert!(status.success(), "webui build failed");
}
