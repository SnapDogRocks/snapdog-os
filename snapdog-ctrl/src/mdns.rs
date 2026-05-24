/// mDNS service discovery abstraction.
///
/// Default backend: `astro-dnssd` (requires Avahi on Linux, native on macOS).
/// Alternative: `mdns-sd` (pure Rust, no system dependencies).
///
/// Select via Cargo features: `mdns-astro` (default) or `mdns-sd`.
use crate::routes::DiscoveredServer;

const SERVICE_TYPE: &str = "_snapdog._tcp";
const BROWSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Browse the local network for `SnapDog` servers.
pub async fn browse_servers() -> Vec<DiscoveredServer> {
    #[cfg(feature = "mdns-astro")]
    {
        browse_astro().await
    }
    #[cfg(feature = "mdns-sd")]
    {
        browse_mdns_sd().await
    }
}

#[cfg(feature = "mdns-astro")]
async fn browse_astro() -> Vec<DiscoveredServer> {
    use astro_dnssd::{BrowseError, ServiceBrowserBuilder, ServiceEventType};

    let browser = match ServiceBrowserBuilder::new(SERVICE_TYPE).browse() {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("mDNS browse failed: {e}");
            return vec![];
        }
    };

    let deadline = std::time::Instant::now() + BROWSE_TIMEOUT;
    let mut servers = Vec::new();

    tokio::task::spawn_blocking(move || {
        while std::time::Instant::now() < deadline {
            match browser.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(svc) if svc.event_type == ServiceEventType::Added && svc.port > 0 => {
                    servers.push(DiscoveredServer {
                        name: svc.name,
                        host: svc.hostname,
                        port: svc.port,
                    });
                }
                Ok(_) | Err(BrowseError::Timeout) => {}
                Err(BrowseError::IoError(e))
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => break,
            }
        }
        servers
    })
    .await
    .unwrap_or_default()
}

#[cfg(feature = "mdns-sd")]
async fn browse_mdns_sd() -> Vec<DiscoveredServer> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let mdns = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("mDNS daemon failed: {e}");
            return vec![];
        }
    };

    let service_type = &format!("{SERVICE_TYPE}.local.");
    let receiver = match mdns.browse(service_type) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("mDNS browse failed: {e}");
            let _ = mdns.shutdown();
            return vec![];
        }
    };

    let mut servers = Vec::new();
    let deadline = tokio::time::Instant::now() + BROWSE_TIMEOUT;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(
            remaining,
            tokio::task::spawn_blocking({
                let receiver = receiver.clone();
                move || receiver.recv_timeout(std::time::Duration::from_millis(500))
            }),
        )
        .await
        {
            Ok(Ok(Ok(ServiceEvent::ServiceResolved(info)))) => {
                let name = info
                    .get_fullname()
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let host = info
                    .get_addresses()
                    .iter()
                    .next()
                    .map(std::string::ToString::to_string)
                    .unwrap_or_default();
                let port = info.get_port();
                if !host.is_empty() {
                    servers.push(DiscoveredServer { name, host, port });
                }
            }
            Ok(Ok(Ok(_))) => {}
            _ => break,
        }
    }

    let _ = mdns.shutdown();
    servers
}
