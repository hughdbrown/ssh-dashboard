use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tokio::sync::Mutex;

use ssh_dashboard::config::Config;
use ssh_dashboard::logging::Logger;
use ssh_dashboard::process;
use ssh_dashboard::state::AppState;
use ssh_dashboard::tui;

#[derive(Parser)]
#[command(name = "ssh-dashboard", about = "TUI dashboard for managing SSH tunnels and long-running commands")]
struct Cli {
    /// Path to config file (default: ~/.ssh-dashboard/config.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(Config::default_config_path);
    let config = Config::load(&config_path)?;

    // Initialize logger
    let log_path = config.log_path();
    let logger = Arc::new(Logger::new(&log_path)?);

    // Create shared application state
    let state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::new(&config)));

    // Start commands marked with startup = true
    for (i, cmd) in config.commands.iter().enumerate() {
        if cmd.startup {
            process::start_command(state.clone(), i, Some(logger.clone()));
        }
    }

    // Spawn signal handler for graceful shutdown
    let signal_state = state.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        process::shutdown_all(signal_state).await;
    });

    // Run the TUI event loop
    tui::run_app(state.clone(), Some(logger.clone())).await?;

    // Ensure all processes are cleaned up on exit
    process::shutdown_all(state).await;

    Ok(())
}
