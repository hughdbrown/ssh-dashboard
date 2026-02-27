use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use crate::config::{CommandConfig, Config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceStatus {
    Running,
    Parked,
}

impl std::fmt::Display for InstanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceStatus::Running => write!(f, "Running"),
            InstanceStatus::Parked => write!(f, "Parked"),
        }
    }
}

/// A running (or parked) instance of a command.
#[derive(Debug, Clone)]
pub struct Instance {
    /// Index into AppState.available (which command config this came from)
    pub config_index: usize,
    pub status: InstanceStatus,
    pub pid: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub restart_count: u32,
    pub recent_failures: Vec<DateTime<Utc>>,
}

/// An available command from config. Always displayed in the "Available" section.
#[derive(Debug, Clone)]
pub struct AvailableCommand {
    pub config: CommandConfig,
}

/// Which section the cursor is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Running,
    Available,
}

pub struct AppState {
    /// Available command templates (from config)
    pub available: Vec<AvailableCommand>,
    /// Running/parked instances
    pub instances: Vec<Instance>,
    /// Next unique instance ID (for stable indexing)
    next_instance_id: usize,
    /// Which section the cursor is in
    pub section: Section,
    /// Selected index within the current section
    pub selected: usize,
    pub shutdown: bool,
    /// Circuit breaker window in seconds (default 30, lower for tests)
    pub failure_window_secs: i64,
}

impl AppState {
    pub fn new(config: &Config) -> Self {
        let available = config
            .commands
            .iter()
            .map(|c| AvailableCommand { config: c.clone() })
            .collect();
        Self {
            available,
            instances: Vec::new(),
            next_instance_id: 0,
            section: Section::Available,
            selected: 0,
            shutdown: false,
            failure_window_secs: 30,
        }
    }

    /// Create a new instance for a given available command index.
    /// Returns the index into `self.instances`.
    pub fn create_instance(&mut self, config_index: usize) -> usize {
        let instance = Instance {
            config_index,
            status: InstanceStatus::Running,
            pid: None,
            started_at: None,
            restart_count: 0,
            recent_failures: Vec::new(),
        };
        let idx = self.instances.len();
        self.instances.push(instance);
        self.next_instance_id += 1;
        idx
    }

    /// Remove a stopped/parked instance by index.
    pub fn remove_instance(&mut self, idx: usize) {
        if idx < self.instances.len() {
            self.instances.remove(idx);
        }
    }

    /// Count running instances for a given config_index.
    pub fn running_count(&self, config_index: usize) -> usize {
        self.instances
            .iter()
            .filter(|i| i.config_index == config_index && i.status == InstanceStatus::Running)
            .count()
    }

    /// Get the name for an instance (from its config).
    pub fn instance_name(&self, instance_idx: usize) -> &str {
        let config_idx = self.instances[instance_idx].config_index;
        &self.available[config_idx].config.name
    }

    /// Get the command string for an instance.
    pub fn instance_command(&self, instance_idx: usize) -> &str {
        let config_idx = self.instances[instance_idx].config_index;
        &self.available[config_idx].config.command
    }

    /// Total navigable items: running instances + available commands.
    pub fn total_items(&self) -> usize {
        self.instances.len() + self.available.len()
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.total_items() == 0 {
            return;
        }
        match self.section {
            Section::Running => {
                if self.selected == 0 {
                    // Wrap to bottom of Available
                    if !self.available.is_empty() {
                        self.section = Section::Available;
                        self.selected = self.available.len() - 1;
                    } else {
                        self.selected = self.instances.len().saturating_sub(1);
                    }
                } else {
                    self.selected -= 1;
                }
            }
            Section::Available => {
                if self.selected == 0 {
                    // Move to bottom of Running section
                    if !self.instances.is_empty() {
                        self.section = Section::Running;
                        self.selected = self.instances.len() - 1;
                    } else {
                        self.selected = self.available.len().saturating_sub(1);
                    }
                } else {
                    self.selected -= 1;
                }
            }
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if self.total_items() == 0 {
            return;
        }
        match self.section {
            Section::Running => {
                if self.selected + 1 >= self.instances.len() {
                    // Move to top of Available
                    if !self.available.is_empty() {
                        self.section = Section::Available;
                        self.selected = 0;
                    } else {
                        self.selected = 0;
                    }
                } else {
                    self.selected += 1;
                }
            }
            Section::Available => {
                if self.selected + 1 >= self.available.len() {
                    // Wrap to top of Running
                    if !self.instances.is_empty() {
                        self.section = Section::Running;
                        self.selected = 0;
                    } else {
                        self.selected = 0;
                    }
                } else {
                    self.selected += 1;
                }
            }
        }
    }

    /// Clamp selection to valid range after instances change.
    pub fn clamp_selection(&mut self) {
        match self.section {
            Section::Running => {
                if self.instances.is_empty() {
                    self.section = Section::Available;
                    self.selected = 0;
                } else if self.selected >= self.instances.len() {
                    self.selected = self.instances.len() - 1;
                }
            }
            Section::Available => {
                if self.available.is_empty() {
                    self.section = Section::Running;
                    self.selected = 0;
                } else if self.selected >= self.available.len() {
                    self.selected = self.available.len() - 1;
                }
            }
        }
    }
}

pub type SharedState = Arc<Mutex<AppState>>;

// Keep CommandStatus for backward compatibility with tests
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandStatus {
    Stopped,
    Running,
    Parked,
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
