use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::config::{CommandConfig, Config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandStatus {
    Stopped,
    Running,
    Parked,
}

impl CommandStatus {
    /// Sort order: Running=0, Stopped=1, Parked=2
    fn sort_key(&self) -> u8 {
        match self {
            CommandStatus::Running => 0,
            CommandStatus::Stopped => 1,
            CommandStatus::Parked => 2,
        }
    }
}

impl std::fmt::Display for CommandStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandStatus::Stopped => write!(f, "Stopped"),
            CommandStatus::Running => write!(f, "Running"),
            CommandStatus::Parked => write!(f, "Parked"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandState {
    pub config: CommandConfig,
    pub status: CommandStatus,
    pub pid: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub restart_count: u32,
    pub recent_failures: Vec<DateTime<Utc>>,
}

impl CommandState {
    pub fn new(config: CommandConfig) -> Self {
        Self {
            config,
            status: CommandStatus::Stopped,
            pid: None,
            started_at: None,
            restart_count: 0,
            recent_failures: Vec::new(),
        }
    }
}

pub struct AppState {
    pub commands: Vec<CommandState>,
    pub selected: usize,
    pub shutdown: bool,
    /// Circuit breaker window in seconds (default 30, lower for tests)
    pub failure_window_secs: i64,
}

impl AppState {
    pub fn new(config: &Config) -> Self {
        let commands = config
            .commands
            .iter()
            .map(|c| CommandState::new(c.clone()))
            .collect();
        Self {
            commands,
            selected: 0,
            shutdown: false,
            failure_window_secs: 30,
        }
    }

    /// Returns indices sorted by status: Running first, then Stopped, then Parked.
    pub fn sorted_indices(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.commands.len()).collect();
        indices.sort_by_key(|&i| self.commands[i].status.sort_key());
        indices
    }
}

pub type SharedState = Arc<Mutex<AppState>>;
