use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use tokio::sync::Mutex;

use crate::state::{AppState, CommandStatus};

/// Spawn a child process from a command string.
/// Splits on whitespace (no shell expansion).
fn spawn_command(cmd: &str) -> Result<tokio::process::Child> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let (program, args) = parts
        .split_first()
        .context("empty command string")?;
    let child = tokio::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("spawning command: {cmd}"))?;
    Ok(child)
}

/// Monitor a command: spawn it, wait for exit, auto-restart or park.
/// This function runs in a loop until the command is parked or shutdown is requested.
pub async fn monitor_command(state: Arc<Mutex<AppState>>, index: usize) {
    loop {
        // Check if shutdown was requested
        {
            let st = state.lock().await;
            if st.shutdown {
                return;
            }
        }

        // Get the command string
        let cmd = {
            let st = state.lock().await;
            st.commands[index].config.command.clone()
        };

        // Try to spawn the child process
        let child = match spawn_command(&cmd) {
            Ok(child) => child,
            Err(_e) => {
                // Spawn failed — record failure and check circuit breaker
                let mut st = state.lock().await;
                let window_secs = st.failure_window_secs;
                let cs = &mut st.commands[index];
                cs.recent_failures.push(Utc::now());
                cs.status = CommandStatus::Stopped;
                cs.pid = None;
                cs.started_at = None;

                let window = chrono::Duration::seconds(window_secs);
                let cutoff = Utc::now() - window;
                cs.recent_failures.retain(|t| *t > cutoff);

                if cs.recent_failures.len() >= 3 {
                    cs.status = CommandStatus::Parked;
                    return;
                }
                // Brief delay before retry
                drop(st);
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };

        let pid = child.id();

        // Update state to Running
        {
            let mut st = state.lock().await;
            let cs = &mut st.commands[index];
            cs.status = CommandStatus::Running;
            cs.pid = pid;
            cs.started_at = Some(Utc::now());
        }

        // Wait for child to exit
        let mut child = child;
        let _exit_status = child.wait().await;

        // Process exited — update state
        let should_park = {
            let mut st = state.lock().await;
            if st.shutdown {
                return;
            }
            let window_secs = st.failure_window_secs;
            let cs = &mut st.commands[index];
            cs.status = CommandStatus::Stopped;
            cs.pid = None;
            let started_at = cs.started_at.take();

            // If the process ran for longer than the failure window, reset failures
            let window = chrono::Duration::seconds(window_secs);
            if let Some(start) = started_at {
                if Utc::now() - start > window {
                    cs.recent_failures.clear();
                }
            }

            cs.recent_failures.push(Utc::now());
            cs.restart_count += 1;

            let cutoff = Utc::now() - window;
            cs.recent_failures.retain(|t| *t > cutoff);

            if cs.recent_failures.len() >= 3 {
                cs.status = CommandStatus::Parked;
                true
            } else {
                false
            }
        };

        if should_park {
            return;
        }

        // Brief delay before auto-restart
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Start a command by spawning a monitor task. Returns the JoinHandle.
pub fn start_command(
    state: Arc<Mutex<AppState>>,
    index: usize,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(monitor_command(state, index))
}

/// Stop a running command by killing its child process.
pub async fn stop_command(state: Arc<Mutex<AppState>>, index: usize) -> Result<()> {
    let pid = {
        let st = state.lock().await;
        st.commands[index].pid
    };
    if let Some(pid) = pid {
        // Send SIGTERM via nix or raw libc
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
    Ok(())
}

/// Graceful shutdown: SIGTERM all children, wait up to 5s, then SIGKILL.
pub async fn shutdown_all(state: Arc<Mutex<AppState>>) {
    let pids: Vec<u32> = {
        let mut st = state.lock().await;
        st.shutdown = true;
        st.commands
            .iter()
            .filter(|c| c.status == CommandStatus::Running)
            .filter_map(|c| c.pid)
            .collect()
    };

    // Send SIGTERM to all
    for &pid in &pids {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    // Wait up to 5 seconds for processes to exit
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        let any_running = {
            let st = state.lock().await;
            st.commands
                .iter()
                .any(|c| c.status == CommandStatus::Running)
        };
        if !any_running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // SIGKILL any remaining
    let remaining_pids: Vec<u32> = {
        let st = state.lock().await;
        st.commands
            .iter()
            .filter(|c| c.status == CommandStatus::Running)
            .filter_map(|c| c.pid)
            .collect()
    };
    for &pid in &remaining_pids {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}
