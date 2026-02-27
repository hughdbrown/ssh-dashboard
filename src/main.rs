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
#[command(
    name = "ssh-dashboard",
    about = "TUI dashboard for managing SSH tunnels and long-running commands"
)]
struct Cli {
    /// Path to config file (default: ~/.ssh-dashboard/config.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Ensure config directory exists; create example config if needed
    if cli.config.is_none()
        && let Some(msg) = Config::ensure_config_dir()?
    {
        eprintln!("{msg}");
    }

    let config_path = cli.config.unwrap_or_else(Config::default_config_path);
    let config = Config::load(&config_path)?;

    // Initialize logger
    let log_path = config.log_path();
    let logger = Arc::new(Logger::new(&log_path)?);

    // Create shared application state
    let state: Arc<Mutex<AppState>> = Arc::new(Mutex::new(AppState::new(&config)));

    // Start interactive startup commands before the TUI so the user can enter passwords
    for (i, cmd) in config.commands.iter().enumerate() {
        if cmd.startup && cmd.interactive {
            println!("\n--- Starting interactive command ---");
            println!("  Name: {}", cmd.name);
            println!("  Command: {}", cmd.command);
            println!();

            let instance_id = {
                let mut st = state.lock().await;
                st.create_instance(i)
            };

            match process::spawn_interactive_command(&cmd.command) {
                Ok(child) => {
                    println!("Process started. Enter password/passphrase if prompted.");
                    println!("Press Enter to continue...");
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;

                    process::start_instance_with_child(
                        state.clone(),
                        instance_id,
                        child,
                        Some(logger.clone()),
                    );
                }
                Err(e) => {
                    eprintln!("Failed to start {}: {e}", cmd.name);
                    let mut st = state.lock().await;
                    if let Some(idx) = st.find_instance(instance_id) {
                        st.remove_instance(idx);
                    }
                }
            }
        }
    }

    // Start non-interactive startup commands
    for (i, cmd) in config.commands.iter().enumerate() {
        if cmd.startup && !cmd.interactive {
            process::start_instance(state.clone(), i, Some(logger.clone()));
        }
    }

    // Spawn signal handler for graceful shutdown (handles both Ctrl-C and SIGTERM)
    let signal_state = state.clone();
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
        process::shutdown_all(signal_state).await;
    });

    // Run the TUI event loop
    tui::run_app(state.clone(), Some(logger.clone())).await?;

    // Ensure all processes are cleaned up on exit
    process::shutdown_all(state).await;

    Ok(())
}
