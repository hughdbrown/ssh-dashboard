use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use tokio::sync::Mutex;

use crate::logging::{EventKind, LogEntry, Logger};
use crate::state::{AppState, InstanceStatus};

/// Spawn a child process from a command string.
/// Splits on whitespace (no shell expansion).
fn spawn_command(cmd: &str) -> Result<tokio::process::Child> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let (program, args) = parts.split_first().context("empty command string")?;
    let child = tokio::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("spawning command: {cmd}"))?;
    Ok(child)
}

fn log_event(logger: &Option<Arc<Logger>>, name: &str, event: EventKind) {
    if let Some(logger) = logger {
        let entry = LogEntry::now(name, event);
        let _ = logger.log(&entry);
    }
}

/// Start a new instance of a command from the available list.
/// Creates an instance in AppState, then spawns a monitor task.
pub fn start_instance(
    state: Arc<Mutex<AppState>>,
    config_index: usize,
    logger: Option<Arc<Logger>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Create the instance in state
        let instance_idx = {
            let mut st = state.lock().await;
            st.create_instance(config_index)
        };
        monitor_instance(state, instance_idx, logger).await;
    })
}

/// Monitor an instance: spawn process, wait for exit, auto-restart or park.
async fn monitor_instance(
    state: Arc<Mutex<AppState>>,
    instance_idx: usize,
    logger: Option<Arc<Logger>>,
) {
    loop {
        // Check if shutdown was requested
        {
            let st = state.lock().await;
            if st.shutdown {
                return;
            }
        }

        // Get command string and name
        let (cmd, name) = {
            let st = state.lock().await;
            if instance_idx >= st.instances.len() {
                return;
            }
            (
                st.instance_command(instance_idx).to_string(),
                st.instance_name(instance_idx).to_string(),
            )
        };

        // Try to spawn
        let child = match spawn_command(&cmd) {
            Ok(child) => child,
            Err(_e) => {
                let mut st = state.lock().await;
                if instance_idx >= st.instances.len() {
                    return;
                }
                let window_secs = st.failure_window_secs;
                let inst = &mut st.instances[instance_idx];
                inst.recent_failures.push(Utc::now());
                inst.pid = None;
                inst.started_at = None;

                let window = chrono::Duration::seconds(window_secs);
                let cutoff = Utc::now() - window;
                inst.recent_failures.retain(|t| *t > cutoff);

                if inst.recent_failures.len() >= 3 {
                    inst.status = InstanceStatus::Parked;
                    log_event(&logger, &name, EventKind::Parked);
                    return;
                }
                drop(st);
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };

        let pid = child.id();

        // Update instance to Running
        {
            let mut st = state.lock().await;
            if instance_idx >= st.instances.len() {
                return;
            }
            let inst = &mut st.instances[instance_idx];
            inst.status = InstanceStatus::Running;
            inst.pid = pid;
            inst.started_at = Some(Utc::now());
        }
        log_event(&logger, &name, EventKind::Started);

        // Wait for exit
        let mut child = child;
        let exit_status = child.wait().await;
        let exit_code = exit_status.ok().and_then(|s| s.code());

        // Process exited
        let should_park = {
            let mut st = state.lock().await;
            if st.shutdown {
                log_event(&logger, &name, EventKind::Stopped { exit_code });
                return;
            }
            if instance_idx >= st.instances.len() {
                return;
            }
            let window_secs = st.failure_window_secs;
            let inst = &mut st.instances[instance_idx];
            inst.pid = None;
            let started_at = inst.started_at.take();

            let window = chrono::Duration::seconds(window_secs);
            if let Some(start) = started_at
                && Utc::now() - start > window
            {
                inst.recent_failures.clear();
            }

            inst.recent_failures.push(Utc::now());
            inst.restart_count += 1;

            let cutoff = Utc::now() - window;
            inst.recent_failures.retain(|t| *t > cutoff);

            if inst.recent_failures.len() >= 3 {
                inst.status = InstanceStatus::Parked;
                true
            } else {
                false
            }
        };

        if should_park {
            log_event(&logger, &name, EventKind::Parked);
            return;
        }

        log_event(&logger, &name, EventKind::Restarted);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Stop a running instance by killing its child process.
pub async fn stop_instance(state: Arc<Mutex<AppState>>, instance_idx: usize) -> Result<()> {
    let pid = {
        let st = state.lock().await;
        if instance_idx < st.instances.len() {
            st.instances[instance_idx].pid
        } else {
            None
        }
    };
    if let Some(pid) = pid {
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
        st.instances
            .iter()
            .filter(|i| i.status == InstanceStatus::Running)
            .filter_map(|i| i.pid)
            .collect()
    };

    for &pid in &pids {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        let any_running = {
            let st = state.lock().await;
            st.instances
                .iter()
                .any(|i| i.status == InstanceStatus::Running)
        };
        if !any_running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let remaining_pids: Vec<u32> = {
        let st = state.lock().await;
        st.instances
            .iter()
            .filter(|i| i.status == InstanceStatus::Running)
            .filter_map(|i| i.pid)
            .collect()
    };
    for &pid in &remaining_pids {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}
