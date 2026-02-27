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
#[command(name = "vibemate", version, about = "AI model proxy and usage dashboard")]
struct Cli {
    #[arg(long, default_value = "~/.vibemate/config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Login { agent: String },
    Usage,
    Proxy,
    Dashboard,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let config = load_config(&cli.config)?;

    let result: Result<()> = match cli.command {
        Commands::Login { agent } => cli::login::run(&agent, &config).await,
        Commands::Usage => cli::usage::run(&config).await,
        Commands::Proxy => cli::proxy::run(&config).await,
        Commands::Dashboard => cli::dashboard::run(&config).await,
    };

    result.map_err(anyhow::Error::from)
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = FmtSubscriber::builder().with_env_filter(env_filter).finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}
