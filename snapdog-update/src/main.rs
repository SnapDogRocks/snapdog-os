use clap::{Parser, ValueEnum};
use snapdog_update::error::UpgradeError;
use snapdog_update::output::{OutputFormat, Reporter};
use snapdog_update::update::UpgradeManager;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputArg {
    Human,
    Json,
}

impl From<OutputArg> for OutputFormat {
    fn from(value: OutputArg) -> Self {
        match value {
            OutputArg::Human => Self::Human,
            OutputArg::Json => Self::Json,
        }
    }
}

#[derive(Parser)]
#[command(
    name = "snapdog-update",
    version = env!("SNAPDOG_UPDATE_VERSION"),
    author = "Fabian Schmieder",
    about = "Firmware update client for SnapDog OS"
)]
struct Args {
    /// Target `SnapDog` base URL (e.g., <http://192.168.1.100> or <https://play.schmieder.eu>)
    #[arg(short, long, env = "SNAPDOG_URL")]
    url: String,

    /// Path to a signed RAUC firmware bundle (.raucb)
    #[arg(short, long)]
    file: PathBuf,

    /// Password for target control interface (optional if auth disabled)
    #[arg(short, long, env = "SNAPDOG_PASSWORD")]
    password: Option<String>,

    /// Disable interactive prompts; missing required input becomes an error
    #[arg(long)]
    non_interactive: bool,

    /// Disable progress bars even when stderr is a terminal
    #[arg(long)]
    no_progress: bool,

    /// Output format for humans or automation
    #[arg(long, value_enum, default_value_t = OutputArg::Human)]
    output: OutputArg,

    /// Total upgrade timeout in minutes
    #[arg(long, default_value_t = 30)]
    timeout_mins: u64,

    /// Progress check poll interval in seconds
    #[arg(long, default_value_t = 2)]
    poll_secs: u64,
}

#[tokio::main]
async fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into());
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let args = Args::parse();
    let reporter = Reporter::new(
        args.output.into(),
        args.no_progress,
        args.non_interactive || matches!(args.output, OutputArg::Json),
    );

    if !args.file.exists() {
        let err = UpgradeError::InvalidArgument(format!(
            "local file does not exist: {}",
            args.file.display()
        ));
        reporter.error(&err);
        return ExitCode::from(1);
    }

    let Some(timeout_secs) = args.timeout_mins.checked_mul(60) else {
        let err = UpgradeError::InvalidArgument("--timeout-mins is too large".to_string());
        reporter.error(&err);
        return ExitCode::from(1);
    };

    let mut manager = match UpgradeManager::new(
        &args.url,
        &args.file,
        Duration::from_secs(timeout_secs),
        Duration::from_secs(args.poll_secs),
        reporter.clone(),
    ) {
        Ok(manager) => manager,
        Err(error) => {
            reporter.error(&error);
            return ExitCode::from(1);
        }
    };

    match manager.run(args.password.as_deref()).await {
        Ok(()) => {
            reporter.success("complete", "Upgrade sequence completed successfully.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            reporter.error(&error);
            ExitCode::from(1)
        }
    }
}
