use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct CommandConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub startup: bool,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub commands: Vec<CommandConfig>,
    pub log: Option<PathBuf>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading config: {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("parsing config: {}", path.display()))?;
        Ok(config)
    }

    pub fn log_path(&self) -> PathBuf {
        if let Some(ref path) = self.log {
            if let Some(stripped) = path.to_str().and_then(|s| s.strip_prefix("~/"))
                && let Some(home) = dirs::home_dir()
            {
                return home.join(stripped);
            }
            path.clone()
        } else {
            Self::default_log_path()
        }
    }

    pub fn default_config_dir() -> PathBuf {
        dirs::home_dir()
            .expect("could not determine home directory")
            .join(".ssh-dashboard")
    }

    pub fn default_config_path() -> PathBuf {
        Self::default_config_dir().join("config.toml")
    }

    fn default_log_path() -> PathBuf {
        Self::default_config_dir().join("history.log")
    }

    /// Ensure the config directory exists. If the config file doesn't exist,
    /// write an example config and return an informational message.
    pub fn ensure_config_dir() -> Result<Option<String>> {
        let dir = Self::default_config_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating config directory: {}", dir.display()))?;

        let config_path = Self::default_config_path();
        if !config_path.exists() {
            let example = r#"# SSH Dashboard configuration
# Edit this file to add your commands.

# log = "~/.ssh-dashboard/history.log"

# [[commands]]
# name = "my-tunnel"
# command = "ssh -N -L 8080:127.0.0.1:8080 user@host"
# startup = true
"#;
            std::fs::write(&config_path, example)
                .with_context(|| format!("writing example config: {}", config_path.display()))?;
            Ok(Some(format!(
                "Created example config at {}. Edit it to add your commands.",
                config_path.display()
            )))
        } else {
            Ok(None)
        }
    }
}
