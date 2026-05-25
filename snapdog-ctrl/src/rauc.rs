//! RAUC D-Bus client for OTA updates.
//!
//! Communicates with the RAUC service via `de.pengutronix.rauc.Installer` interface.
#![allow(dead_code)]

use std::collections::HashMap;

use zbus::Connection;
use zbus::proxy;

/// RAUC D-Bus proxy (auto-generated interface).
#[proxy(
    interface = "de.pengutronix.rauc.Installer",
    default_service = "de.pengutronix.rauc",
    default_path = "/"
)]
trait RaucInstaller {
    fn install_bundle(
        &self,
        source: &str,
        args: HashMap<String, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
    fn mark(&self, state: &str, slot_identifier: &str) -> zbus::Result<(String, String)>;
    fn get_slot_status(
        &self,
    ) -> zbus::Result<Vec<(String, HashMap<String, zbus::zvariant::OwnedValue>)>>;
    fn get_primary(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn operation(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn last_error(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn progress(&self) -> zbus::Result<(i32, String, i32)>;

    #[zbus(property)]
    fn compatible(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn boot_slot(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn completed(&self, result: i32) -> zbus::Result<()>;
}

/// High-level RAUC client.
pub struct Rauc {
    proxy: RaucInstallerProxy<'static>,
}

#[derive(Debug, serde::Serialize)]
pub struct SlotStatus {
    pub name: String,
    pub class: String,
    pub device: String,
    pub state: String,
    pub version: String,
    pub booted: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct InstallProgress {
    pub percentage: i32,
    pub message: String,
}

impl Rauc {
    /// Connect to the RAUC D-Bus service.
    pub async fn connect() -> anyhow::Result<Self> {
        let connection = Connection::system().await?;
        let proxy = RaucInstallerProxy::new(&connection).await?;
        Ok(Self { proxy })
    }

    /// Install a bundle from a local path or URL.
    pub async fn install(&self, source: &str) -> anyhow::Result<()> {
        self.proxy.install_bundle(source, HashMap::new()).await?;
        Ok(())
    }

    /// Mark a slot good/bad/active.
    pub async fn mark(&self, state: &str, slot: &str) -> anyhow::Result<String> {
        let (name, msg) = self.proxy.mark(state, slot).await?;
        tracing::info!("RAUC mark {state} {slot}: {name} — {msg}");
        Ok(msg)
    }

    /// Mark the booted slot as good.
    pub async fn mark_good(&self) -> anyhow::Result<String> {
        self.mark("good", "booted").await
    }

    /// Get current operation (idle/installing).
    pub async fn operation(&self) -> anyhow::Result<String> {
        Ok(self.proxy.operation().await?)
    }

    /// Get last error message.
    pub async fn last_error(&self) -> anyhow::Result<String> {
        Ok(self.proxy.last_error().await?)
    }

    /// Get installation progress.
    pub async fn progress(&self) -> anyhow::Result<InstallProgress> {
        let (pct, msg, _depth) = self.proxy.progress().await?;
        Ok(InstallProgress {
            percentage: pct,
            message: msg,
        })
    }

    /// Get the primary slot name.
    pub async fn primary(&self) -> anyhow::Result<String> {
        Ok(self.proxy.get_primary().await?)
    }

    /// Get status of all slots.
    pub async fn slot_status(&self) -> anyhow::Result<Vec<SlotStatus>> {
        let boot_slot = self.proxy.boot_slot().await.unwrap_or_default();
        let raw = self.proxy.get_slot_status().await?;
        let mut slots = Vec::new();

        for (name, props) in raw {
            let get_str = |k: &str| -> String {
                props
                    .get(k)
                    .and_then(|v| <String as TryFrom<_>>::try_from(v.clone()).ok())
                    .unwrap_or_default()
            };

            let class = name.split('.').next().unwrap_or("").to_string();
            let device = get_str("device");
            let state = get_str("state");
            let version = get_str("bundle.version");
            let booted = get_str("bootname") == boot_slot || name == boot_slot;

            slots.push(SlotStatus {
                name,
                class,
                device,
                state,
                version,
                booted,
            });
        }

        Ok(slots)
    }

    /// Get system compatible string.
    pub async fn compatible(&self) -> anyhow::Result<String> {
        Ok(self.proxy.compatible().await?)
    }
}
