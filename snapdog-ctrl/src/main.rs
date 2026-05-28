// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

mod auth;
#[cfg(not(debug_assertions))]
mod auto_update;
mod config_txt;
mod mdns;
#[cfg(debug_assertions)]
#[cfg(debug_assertions)]
mod mock;
#[cfg_attr(debug_assertions, allow(dead_code))]
mod network;
mod rauc;
#[cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
mod routes;
mod server_config;
#[cfg_attr(debug_assertions, allow(dead_code))]
mod system;
mod ws;

use axum::Router;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    #[cfg(target_os = "linux")]
    {
        let journald = tracing_journald::layer().ok();
        if journald.is_some() {
            tracing_subscriber::registry()
                .with(filter)
                .with(journald)
                .init();
        } else {
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    let app = build_app().await;

    let port = std::env::var("SNAPDOG_SETUP_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(80);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;

    // Log real interface addresses
    if let Ok(addrs) = tokio::net::lookup_host(format!("0.0.0.0:{port}")).await {
        let _ = addrs; // lookup_host on 0.0.0.0 doesn't help, use system interfaces
    }
    let interfaces: Vec<String> = std::net::UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|s| s.connect("1.1.1.1:80").ok().map(|()| s))
        .and_then(|s| s.local_addr().ok())
        .map(|a| vec![format!("http://{}:{port}", a.ip())])
        .unwrap_or_default();

    if interfaces.is_empty() {
        tracing::info!("snapdog-ctrl listening on port {port}");
    } else {
        tracing::info!("snapdog-ctrl listening on {}", interfaces.join(", "));
    }

    // Mark boot as successful (clears OTA rollback counter)
    #[cfg(target_os = "linux")]
    {
        if tokio::fs::metadata("/boot/boot-attempts").await.is_ok() {
            let _ = tokio::process::Command::new("mount")
                .args(["-o", "remount,rw", "/boot"])
                .output()
                .await;

            if tokio::fs::remove_file("/boot/boot-attempts").await.is_ok() {
                tracing::info!("Boot marked successful (OTA rollback counter cleared)");
            }

            let _ = tokio::process::Command::new("mount")
                .args(["-o", "remount,ro", "/boot"])
                .output()
                .await;
        }
    }

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(debug_assertions)]
async fn build_app() -> Router {
    let mock = mock::MockState::new();
    tracing::info!("🔶 Running in MOCK mode (debug build)");

    let auth_state = auth::AuthState::load().await;
    let health_state = routes::HealthState(std::sync::Arc::new(vec![]));

    let (tx, _rx) = tokio::sync::broadcast::channel::<String>(100);
    let ws_sender = ws::WsSender(tx);

    Router::new()
        .nest("/api", routes::api_mock(mock))
        .fallback(routes::static_files)
        .layer(axum::middleware::from_fn({
            let auth = auth_state.clone();
            move |req, next| {
                let auth = auth.clone();
                async move { auth::require_auth_ext(auth, req, next).await }
            }
        }))
        .layer(axum::Extension(health_state))
        .layer(axum::Extension(auth_state))
        .layer(axum::Extension(ws_sender))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}

#[cfg(not(debug_assertions))]
async fn build_app() -> Router {
    // Start setup AP only if no network interface is configured
    tokio::spawn(async {
        let _ = network::configure_resolved().await;

        let softap = system::get_softap_config().await;
        if !softap.enabled {
            return;
        }

        let wifi_configured = network::is_wifi_configured().await;
        let eth_has_link = has_network_link().await;

        if !wifi_configured && !eth_has_link {
            tracing::info!("No network configured — starting setup AP (SSID: SnapDog-Setup)");
            if let Err(e) = network::start_ap(&softap.password).await {
                tracing::error!("Failed to start AP: {e}");
                return;
            }
            // Auto-close AP when a network interface comes up
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if network::is_wifi_configured().await || has_network_link().await {
                    tracing::info!("Network connected — stopping setup AP");
                    let _ = network::stop_ap().await;
                    break;
                }
            }
        }
    });

    // Preflight health check
    let health_warnings = system::preflight_check().await;
    let has_critical = health_warnings.iter().any(|w| w.severity == "critical");
    let health_state = routes::HealthState(std::sync::Arc::new(health_warnings));

    if has_critical {
        tracing::error!("Critical health issue detected — running in degraded mode (no services)");
    } else {
        // Auto-detect DAC on first boot: if EEPROM detected and no overlay set → apply + reboot
        if system::auto_apply_dac_overlay().await {
            tracing::info!("DAC detected and configured — rebooting to activate");
            let _ = tokio::process::Command::new("systemctl")
                .arg("reboot")
                .status()
                .await;
            return Router::new(); // unreachable, but satisfies return type
        }

        // Apply service config (start/stop ssh, client, server based on ctrl.toml)
        tokio::spawn(async {
            system::apply_service_config().await;
        });

        // Start auto-update scheduler
        auto_update::spawn();
    }

    let auth_state = auth::AuthState::load().await;

    let (tx, _rx) = tokio::sync::broadcast::channel::<String>(100);
    let ws_sender = ws::WsSender(tx);

    Router::new()
        .nest("/api", routes::api())
        .merge(routes::captive_portal_routes())
        .fallback(routes::static_files)
        .layer(axum::middleware::from_fn({
            let hs = health_state.clone();
            move |req, next| {
                let hs = hs.clone();
                async move { routes::degraded_mode_guard(hs, req, next).await }
            }
        }))
        .layer(axum::Extension(health_state))
        .layer(axum::middleware::from_fn({
            let auth = auth_state.clone();
            move |req, next| {
                let auth = auth.clone();
                async move { auth::require_auth_ext(auth, req, next).await }
            }
        }))
        .layer(axum::Extension(auth_state))
        .layer(axum::Extension(ws_sender))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}

/// Check if any ethernet interface has carrier (link detected).
#[cfg(not(debug_assertions))]
async fn has_network_link() -> bool {
    for iface in &["eth0", "end0"] {
        let path = format!("/sys/class/net/{iface}/carrier");
        if let Ok(val) = tokio::fs::read_to_string(&path).await {
            if val.trim() == "1" {
                return true;
            }
        }
    }
    false
}
