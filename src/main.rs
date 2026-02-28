mod agent;
mod cli;
mod config;
mod error;
mod provider;
mod proxy;
mod tui;

use clap::{Parser, Subcommand};
use config::{ensure_config_initialized, load_config};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use crate::error::Result;

#[derive(Parser, Debug)]
#[command(
    name = "vibemate",
    version,
    about = "Your vibe coding companion",
    long_about = "A CLI for logging into supported agents, checking quota usage, running a local proxy, and viewing a terminal dashboard."
)]
struct Cli {
    #[arg(
        long,
        value_name = "PATH",
        default_value = "~/.vibemate/config.toml",
        help = "Path to the config file"
    )]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(
        about = "Authenticate with an agent provider",
        long_about = "Start an OAuth login flow for the selected agent and save the token locally."
    )]
    Login {
        #[arg(
            value_name = "AGENT",
            help = "Agent ID to log in (for example: codex, claude)"
        )]
        agent: String,
    },
    #[command(
        about = "Show usage and quota information",
        long_about = "Fetch usage data for logged-in agents. Use --json for normalized JSON output, or --raw for upstream provider payloads."
    )]
    Usage {
        #[arg(
            long,
            conflicts_with = "raw",
            help = "Print normalized usage data as pretty JSON"
        )]
        json: bool,
        #[arg(
            long,
            conflicts_with = "json",
            help = "Print raw provider usage payloads as pretty JSON"
        )]
        raw: bool,
    },
    #[command(
        about = "Run the local proxy server",
        long_about = "Start the Vibemate proxy server using the configured host and port."
    )]
    Proxy,
    #[command(
        about = "Launch the interactive terminal dashboard",
        long_about = "Start the proxy and open the TUI dashboard with logs and usage panels."
    )]
    Dashboard,
    #[command(
        about = "Inspect or initialize the config file",
        long_about = "Print the current config file content, or create one with defaults using --init."
    )]
    Config {
        #[arg(long, help = "Create a default config file at --config path")]
        init: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let Cli {
        config: config_path,
        command,
    } = Cli::parse();

    let is_dashboard = matches!(command, Commands::Dashboard);
    init_tracing(is_dashboard);

    if !matches!(command, Commands::Config { init: true }) {
        ensure_config_initialized(&config_path)?;
    }

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

    result
}

fn init_tracing(dashboard_mode: bool) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    if dashboard_mode {
        // In dashboard/TUI mode, discard log output so it doesn't corrupt the screen.
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::sink)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    } else {
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    }
}
