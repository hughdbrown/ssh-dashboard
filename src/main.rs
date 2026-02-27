use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use ssh_dashboard::config::Config;

#[derive(Parser)]
#[command(name = "ssh-dashboard", about = "TUI dashboard for managing SSH tunnels and long-running commands")]
struct Cli {
    /// Path to config file (default: ~/.ssh-dashboard/config.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(Config::default_config_path);
    let _config = Config::load(&config_path)?;
    Ok(())
}
