// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

mod auth;
#[cfg(not(debug_assertions))]
mod auto_update;
#[cfg_attr(debug_assertions, allow(dead_code))]
mod captive_dns;
mod config_txt;
mod mdns;
#[cfg(debug_assertions)]
mod mock;
#[cfg(not(debug_assertions))]
mod mpris_client;
#[cfg_attr(debug_assertions, allow(dead_code))]
mod network;
mod rauc;
#[cfg_attr(debug_assertions, allow(dead_code, unused_imports))]
mod routes;
mod server_config;
mod settings;
#[cfg_attr(debug_assertions, allow(dead_code))]
mod system;
mod tuning;
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

    // OTA rollback is handled out-of-process: rauc-mark-good.service runs
    // rauc-commit (mark-good + boot-handler commit) once the ctrl is up, which
    // commits a healthy tryboot trial or leaves a failed one to auto-revert on the
    // next normal boot. Nothing to do here.

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
        // Ensure ethernet has a network config (write DHCP default if missing)
        if tokio::fs::metadata(network::ETH_NETWORK_PATH)
            .await
            .is_err()
        {
            let _ = network::configure_ethernet(None).await;
        }
        let _ = network::configure_resolved().await;

        let softap = system::get_softap_config().await;

        // WiFi already configured: nothing has started the supplicant on this
        // path yet, so bring the client up and we're done (no AP).
        if network::is_wifi_configured().await {
            if let Err(e) = network::start_wifi_client().await {
                tracing::error!("Failed to start WiFi client: {e}");
            }
            return;
        }

        if !softap.enabled {
            return;
        }

        // No WiFi configured. Give a wired link a chance to obtain a REAL IP
        // before deciding — carrier-up-but-no-DHCP must NOT suppress the setup AP,
        // or the device is unreachable with no way in.
        let wait_for_ip_secs: u64 = 30;
        for _ in 0..(wait_for_ip_secs / 3) {
            if system::has_connectivity().await {
                tracing::info!("Functional network present — not starting setup AP");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }

        tracing::info!("No functional network after {wait_for_ip_secs}s — starting setup AP");
        if let Err(e) = network::start_ap(&softap.password, &softap.country).await {
            tracing::error!("Failed to start AP: {e}");
            return;
        }
        // Log the passphrase to the console so a first-join is possible out-of-band
        // (the AP password is never exposed on the LAN, see get_softap_view).
        tracing::warn!(
            "Setup AP '{}' is up — join with passphrase: {}",
            network::ap_ssid().await,
            softap.password
        );
        // Auto-close the AP once real connectivity appears (idempotent stop_ap;
        // connect_wifi's deferred teardown may beat us to it).
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if system::has_connectivity().await {
                tracing::info!("Network connected — stopping setup AP");
                let _ = network::stop_ap().await;
                break;
            }
        }
    });

    // Reconcile the previous auto-update: confirm the pending bundle booted, or
    // mark it bad if the bootloader rolled us back, so a broken bundle is never
    // reinstalled in a loop.
    system::reconcile_pending_update().await;

    // Preflight health check
    let health_warnings = system::preflight_check().await;
    let has_critical = health_warnings.iter().any(|w| w.severity == "critical");
    let health_state = routes::HealthState(std::sync::Arc::new(health_warnings));

    if has_critical {
        tracing::error!("Critical health issue detected — running in degraded mode (no services)");
    } else {
        // Migrate devices flashed before the HAT-EEPROM fix. OTA can't rewrite the
        // shared /boot partition, so a stale config.txt may still carry
        // force_eeprom_read=0, which blocks the firmware's HAT EEPROM read. Strip it
        // here (a persistent fix) and reboot to apply it. Note: during a RAUC tryboot
        // trial the firmware boots from tryboot.txt, not this just-cleaned config.txt,
        // so the EEPROM read (and the DAC auto-detect below) lands on the first
        // committed boot after the trial rather than on this reboot; on a normal boot
        // it takes effect immediately.
        match config_txt::reconcile_eeprom_settings().await {
            Ok(true) => {
                tracing::info!(
                    "config.txt: removed EEPROM-disabling lines — rebooting so the firmware reads the HAT EEPROM"
                );
                system::reboot().await;
                return Router::new();
            }
            Ok(false) => {}
            Err(e) => tracing::warn!("config.txt EEPROM reconcile failed: {e}"),
        }

        // Auto-detect DAC on first boot: if EEPROM detected and no overlay set → apply + reboot
        if system::auto_apply_dac_overlay().await {
            tracing::info!("DAC detected and configured — rebooting to activate");
            // Use the tryboot-aware reboot so a DAC-detect reboot during an OTA
            // trial re-enters the trial (rather than reverting a good update).
            system::reboot().await;
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
    let ws_sender = ws::WsSender(tx.clone());

    // Start MPRIS2 poller if client is enabled
    let now_playing = if system::is_service_enabled("client").await {
        let (np, _handle) = mpris_client::start(tx);
        np
    } else {
        std::sync::Arc::new(tokio::sync::Mutex::new(mpris_client::NowPlaying::default()))
    };

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
        .layer(axum::Extension(now_playing))
        .layer(TraceLayer::new_for_http())
}
