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
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading config: {}", path.display()))?;
        let config: Config =
            toml::from_str(&content).with_context(|| format!("parsing config: {}", path.display()))?;
        Ok(config)
    }

    pub fn log_path(&self) -> PathBuf {
        if let Some(ref path) = self.log {
            if let Some(stripped) = path.to_str().and_then(|s| s.strip_prefix("~/")) {
                if let Some(home) = dirs::home_dir() {
                    return home.join(stripped);
                }
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
}
