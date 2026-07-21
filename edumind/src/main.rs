use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use edumind::{
    GatewayBootstrap, GatewayMode,
    config::load_from_path,
    gateway::{AppState, bind_listener, serve},
    infra::{EduMindError, Result},
};

/// Starts the EduMind gateway bootstrap process.
#[derive(Debug, Parser)]
#[command(name = "edumind", version, about)]
struct Cli {
    /// Select the gateway lifecycle owner.
    #[arg(long, value_enum, default_value_t = CliMode::Standalone)]
    mode: CliMode,
    /// Load and validate a YAML configuration file before bootstrapping.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Validate the selected configuration and exit without starting the gateway.
    #[arg(long, requires = "config")]
    check_config: bool,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum CliMode {
    #[default]
    Standalone,
    Embedded,
}

impl From<CliMode> for GatewayMode {
    fn from(value: CliMode) -> Self {
        match value {
            CliMode::Standalone => Self::Standalone,
            CliMode::Embedded => Self::Embedded,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(path) = cli.config {
        let config = load_from_path(&path)?;
        if cli.check_config {
            println!("Configuration {} is valid.", path.display());
            return Ok(());
        }
        let listener = bind_listener(&config.gateway).await?;
        let address = listener.local_addr().map_err(|error| {
            EduMindError::Gateway(format!("failed to read listener address: {error}"))
        })?;
        println!(
            "EduMind gateway for {} listening on {}.",
            config.meta.name, address
        );
        return serve(listener, AppState::new(config)?).await;
    }
    let bootstrap = GatewayBootstrap::local(cli.mode.into());

    println!(
        "EduMind Phase 1 gateway bootstrap ready on {} ({:?} mode).",
        bootstrap.bind_addr, bootstrap.mode
    );
    Ok(())
}
