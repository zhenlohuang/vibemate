mod cli;
mod config;
mod error;
mod oauth;
mod provider;
mod proxy;
mod tui;

use clap::{Parser, Subcommand};
use config::load_config;
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use crate::error::Result;

#[derive(Parser, Debug)]
#[command(name = "vibemate", version, about = "Your Vibe Coding mate")]
struct Cli {
    #[arg(long, default_value = "~/.vibemate/config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Login {
        agent: String,
    },
    Usage {
        #[arg(long, conflicts_with = "raw")]
        json: bool,
        #[arg(long, conflicts_with = "json")]
        raw: bool,
    },
    Proxy,
    Dashboard,
    Config {
        #[arg(long)]
        init: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let Cli {
        config: config_path,
        command,
    } = Cli::parse();

    let result: Result<()> = match command {
        Commands::Config { init } => cli::config::run(init, &config_path),
        command => {
            let config = load_config(&config_path)?;
            match command {
                Commands::Login { agent } => cli::login::run(&agent, &config).await,
                Commands::Usage { json, raw } => {
                    cli::usage::run(&config, cli::usage::UsageOptions { json, raw }).await
                }
                Commands::Proxy => cli::proxy::run(&config).await,
                Commands::Dashboard => cli::dashboard::run(&config).await,
                Commands::Config { .. } => unreachable!("handled above"),
            }
        }
    };

    result.map_err(anyhow::Error::from)
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(env_filter)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}
