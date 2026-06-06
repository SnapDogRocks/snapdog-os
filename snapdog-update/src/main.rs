use clap::Parser;
use snapdog_update::update::UpgradeManager;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(
    name = "snapdog-update",
    version = "0.1.0",
    author = "Fabian Schmieder",
    about = "Development upgrade client tool for SnapDog OS"
)]
struct Args {
    /// Target `SnapDog` base URL (e.g., <http://192.168.1.100> or <https://play.schmieder.eu>)
    #[arg(short, long, env = "SNAPDOG_URL")]
    url: String,

    /// Path to firmware bundle (.raucb) or raw system image (.img/.img.gz)
    #[arg(short, long)]
    file: PathBuf,

    /// Password for target control interface (optional if auth disabled)
    #[arg(short, long, env = "SNAPDOG_PASSWORD")]
    password: Option<String>,

    /// Bypass RAUC installation and force write raw disk image partition
    #[arg(long)]
    raw: bool,

    /// Total upgrade timeout in minutes
    #[arg(long, default_value_t = 30)]
    timeout_mins: u64,

    /// Progress check poll interval in seconds
    #[arg(long, default_value_t = 2)]
    poll_secs: u64,
}

#[tokio::main]
async fn main() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let args = Args::parse();

    if !args.file.exists() {
        tracing::error!("Local file does not exist: {}", args.file.display());
        std::process::exit(1);
    }

    let mut manager = UpgradeManager::new(
        &args.url,
        &args.file,
        args.raw,
        Duration::from_secs(args.timeout_mins * 60),
        Duration::from_secs(args.poll_secs),
    );

    match manager.run(args.password.as_deref()).await {
        Ok(()) => {
            tracing::info!("Upgrade sequence completed successfully!");
        }
        Err(e) => {
            tracing::error!("Upgrade sequence failed: {}", e);
            std::process::exit(1);
        }
    }
}
