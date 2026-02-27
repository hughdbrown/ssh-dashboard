use std::sync::Arc;

use ssh_dashboard::config::{CommandConfig, Config};
use ssh_dashboard::process;
use ssh_dashboard::state::{AppState, InstanceStatus, Section, SharedState};
use tokio::sync::Mutex;

extern crate libc;

fn make_state(commands: Vec<(&str, &str)>) -> SharedState {
    let config = Config {
        commands: commands
            .into_iter()
            .map(|(name, cmd)| CommandConfig {
                name: name.to_string(),
                command: cmd.to_string(),
                startup: false,
                interactive: false,
            })
            .collect(),
        log: None,
    };
    let mut app = AppState::new(&config);
    // Use a short failure window for tests (2 seconds)
    app.failure_window_secs = 2;
    Arc::new(Mutex::new(app))
}

#[tokio::test]
async fn test_process_start_success() {
    let state = make_state(vec![("sleeper", "sleep 60")]);

    let handle = process::start_instance(state.clone(), 0, None);

    // Give it a moment to spawn
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let st = state.lock().await;
    assert_eq!(st.instances.len(), 1);
    assert_eq!(st.instances[0].status, InstanceStatus::Running);
    assert!(st.instances[0].pid.is_some());
    drop(st);

    // Clean up
    process::stop_instance(state.clone(), 0).await.unwrap();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle).await;
}

#[tokio::test]
async fn test_process_exit_detected() {
    let state = make_state(vec![("quick-exit", "true")]);

    let _handle = process::start_instance(state.clone(), 0, None);

    // Wait for the process to exit and be restarted at least once
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let st = state.lock().await;
    assert!(!st.instances.is_empty());
    assert!(
        st.instances[0].restart_count > 0,
        "expected restart_count > 0, got {}",
        st.instances[0].restart_count
    );
    drop(st);

    process::shutdown_all(state.clone()).await;
}

#[tokio::test]
async fn test_process_auto_restart() {
    let state = make_state(vec![("restarter", "true")]);

    let _handle = process::start_instance(state.clone(), 0, None);

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let st = state.lock().await;
    assert!(
        st.instances[0].restart_count >= 1,
        "expected at least 1 restart, got {}",
        st.instances[0].restart_count
    );
    drop(st);

    process::shutdown_all(state.clone()).await;
}

#[tokio::test]
async fn test_process_park_after_3_failures() {
    let state = make_state(vec![("always-fail", "/bin/false")]);

    let handle = process::start_instance(state.clone(), 0, None);

    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;

    let st = state.lock().await;
    assert_eq!(
        st.instances[0].status,
        InstanceStatus::Parked,
        "expected Parked status after 3 rapid failures"
    );
}

#[tokio::test]
async fn test_process_stop_sends_sigterm() {
    let state = make_state(vec![("stoppable", "sleep 60")]);

    let _handle = process::start_instance(state.clone(), 0, None);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    {
        let st = state.lock().await;
        assert_eq!(st.instances[0].status, InstanceStatus::Running);
    }

    process::stop_instance(state.clone(), 0).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    process::shutdown_all(state.clone()).await;
}

#[tokio::test]
async fn test_graceful_shutdown() {
    let state = make_state(vec![("sleeper1", "sleep 60"), ("sleeper2", "sleep 60")]);

    let _h1 = process::start_instance(state.clone(), 0, None);
    let _h2 = process::start_instance(state.clone(), 1, None);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    {
        let st = state.lock().await;
        assert_eq!(st.instances.len(), 2);
        assert_eq!(st.instances[0].status, InstanceStatus::Running);
        assert_eq!(st.instances[1].status, InstanceStatus::Running);
    }

    process::shutdown_all(state.clone()).await;

    let st = state.lock().await;
    assert!(st.shutdown);
}

#[tokio::test]
async fn test_multiple_instances_same_command() {
    let state = make_state(vec![("sleeper", "sleep 60")]);

    // Start two instances of the same command
    let _h1 = process::start_instance(state.clone(), 0, None);
    let _h2 = process::start_instance(state.clone(), 0, None);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let st = state.lock().await;
    assert_eq!(st.instances.len(), 2);
    assert_eq!(st.instances[0].config_index, 0);
    assert_eq!(st.instances[1].config_index, 0);
    assert_eq!(st.running_count(0), 2);
    drop(st);

    process::shutdown_all(state.clone()).await;
}

#[tokio::test]
async fn test_stop_requested_prevents_restart() {
    let state = make_state(vec![("stoppable", "sleep 60")]);

    let handle = process::start_instance(state.clone(), 0, None);

    // Wait for the process to start
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Verify it's running
    {
        let st = state.lock().await;
        assert_eq!(st.instances.len(), 1);
        assert_eq!(st.instances[0].status, InstanceStatus::Running);
    }

    // Set stop_requested and send SIGTERM (simulates Enter on Running)
    let pid = {
        let mut st = state.lock().await;
        st.instances[0].stop_requested = true;
        st.instances[0].pid
    };
    if let Some(pid) = pid {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    // Wait for monitor to process the exit
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle).await;

    // Instance should be removed (not restarted)
    let st = state.lock().await;
    assert!(
        st.instances.is_empty(),
        "expected instance to be removed after stop_requested, but found {} instances",
        st.instances.len()
    );
}

#[test]
fn test_navigation_between_sections() {
    let config = Config {
        commands: vec![
            CommandConfig {
                name: "cmd1".to_string(),
                command: "echo".to_string(),
                startup: false,
                interactive: false,
            },
            CommandConfig {
                name: "cmd2".to_string(),
                command: "echo".to_string(),
                startup: false,
                interactive: false,
            },
        ],
        log: None,
    };
    let mut state = AppState::new(&config);

    // Start in Available section
    assert_eq!(state.section, Section::Available);
    assert_eq!(state.selected, 0);

    // Move down
    state.select_next();
    assert_eq!(state.section, Section::Available);
    assert_eq!(state.selected, 1);

    // Move down again: should wrap (no running instances, so wraps within Available)
    state.select_next();
    assert_eq!(state.section, Section::Available);
    assert_eq!(state.selected, 0);

    // Add a running instance
    state.create_instance(0);

    // Move up from Available[0]: should go to Running section
    state.select_prev();
    assert_eq!(state.section, Section::Running);
    assert_eq!(state.selected, 0);

    // Move up again: should wrap to bottom of Available
    state.select_prev();
    assert_eq!(state.section, Section::Available);
    assert_eq!(state.selected, 1);
}
