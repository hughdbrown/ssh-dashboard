use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

#[derive(Debug)]
pub enum EventKind {
    Started,
    Stopped { exit_code: Option<i32> },
    Restarted,
    Parked,
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventKind::Started => write!(f, "STARTED"),
            EventKind::Stopped {
                exit_code: Some(code),
            } => write!(f, "STOPPED (exit_code={code})"),
            EventKind::Stopped { exit_code: None } => write!(f, "STOPPED (exit_code=unknown)"),
            EventKind::Restarted => write!(f, "RESTARTED"),
            EventKind::Parked => write!(f, "PARKED"),
        }
    }
}

pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub command_name: String,
    pub event: EventKind,
}

impl LogEntry {
    pub fn now(command_name: &str, event: EventKind) -> Self {
        Self {
            timestamp: Utc::now(),
            command_name: command_name.to_string(),
            event,
        }
    }
}

impl std::fmt::Display for LogEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} | {} | {}",
            self.timestamp.format("%Y-%m-%dT%H:%M:%SZ"),
            self.command_name,
            self.event,
        )
    }
}

/// File-based event logger. Thread-safe via std::sync::Mutex (no async needed for file writes).
pub struct Logger {
    path: PathBuf,
    file: StdMutex<std::fs::File>,
}

impl Logger {
    pub fn new(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating log directory: {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening log file: {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            file: StdMutex::new(file),
        })
    }

    pub fn log(&self, entry: &LogEntry) -> Result<()> {
        let line = format!("{entry}\n");
        let mut file = self.file.lock().unwrap();
        file.write_all(line.as_bytes())
            .with_context(|| format!("writing to log: {}", self.path.display()))?;
        file.flush()
            .with_context(|| format!("flushing log: {}", self.path.display()))?;
        Ok(())
    }
}
