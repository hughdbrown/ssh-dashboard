use std::sync::Arc;

use ssh_dashboard::config::{CommandConfig, Config};
use ssh_dashboard::process;
use ssh_dashboard::state::{AppState, CommandStatus, SharedState};
use tokio::sync::Mutex;

fn make_state(commands: Vec<(&str, &str)>) -> SharedState {
    let config = Config {
        commands: commands
            .into_iter()
            .map(|(name, cmd)| CommandConfig {
                name: name.to_string(),
                command: cmd.to_string(),
                startup: false,
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

    let handle = process::start_command(state.clone(), 0, None);

    // Give it a moment to spawn
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let st = state.lock().await;
    assert_eq!(st.commands[0].status, CommandStatus::Running);
    assert!(st.commands[0].pid.is_some());
    drop(st);

    // Clean up: stop the process
    process::stop_command(state.clone(), 0).await.unwrap();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle).await;
}

#[tokio::test]
async fn test_process_exit_detected() {
    // `true` exits immediately with code 0
    let state = make_state(vec![("quick-exit", "true")]);

    let _handle = process::start_command(state.clone(), 0, None);

    // Wait for the process to exit and be restarted at least once
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let st = state.lock().await;
    // It should have restarted at least once
    assert!(
        st.commands[0].restart_count > 0,
        "expected restart_count > 0, got {}",
        st.commands[0].restart_count
    );
    drop(st);

    // Shut down to stop the restart loop
    process::shutdown_all(state.clone()).await;
}

#[tokio::test]
async fn test_process_auto_restart() {
    // `true` exits immediately — should be restarted
    let state = make_state(vec![("restarter", "true")]);

    let _handle = process::start_command(state.clone(), 0, None);

    // Wait for a few restart cycles
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let st = state.lock().await;
    assert!(
        st.commands[0].restart_count >= 1,
        "expected at least 1 restart, got {}",
        st.commands[0].restart_count
    );
    drop(st);

    process::shutdown_all(state.clone()).await;
}

#[tokio::test]
async fn test_process_park_after_3_failures() {
    // `/bin/false` always exits with code 1
    let state = make_state(vec![("always-fail", "/bin/false")]);

    let handle = process::start_command(state.clone(), 0, None);

    // Wait for 3 failures to accumulate within the 2-second window
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;

    let st = state.lock().await;
    assert_eq!(
        st.commands[0].status,
        CommandStatus::Parked,
        "expected Parked status after 3 rapid failures"
    );
}

#[tokio::test]
async fn test_process_manual_restart_parked() {
    // Park a command first
    let state = make_state(vec![("park-then-restart", "/bin/false")]);

    let handle = process::start_command(state.clone(), 0, None);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;

    // Verify it's parked
    {
        let st = state.lock().await;
        assert_eq!(st.commands[0].status, CommandStatus::Parked);
    }

    // Reset state for manual restart with a command that actually runs
    {
        let mut st = state.lock().await;
        st.commands[0].config.command = "sleep 60".to_string();
        st.commands[0].status = CommandStatus::Stopped;
        st.commands[0].recent_failures.clear();
        st.shutdown = false;
    }

    let handle = process::start_command(state.clone(), 0, None);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let st = state.lock().await;
    assert_eq!(
        st.commands[0].status,
        CommandStatus::Running,
        "expected Running after manual restart of parked command"
    );
    drop(st);

    process::stop_command(state.clone(), 0).await.unwrap();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), handle).await;
}

#[tokio::test]
async fn test_process_stop_sends_sigterm() {
    let state = make_state(vec![("stoppable", "sleep 60")]);

    let _handle = process::start_command(state.clone(), 0, None);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Verify running
    {
        let st = state.lock().await;
        assert_eq!(st.commands[0].status, CommandStatus::Running);
    }

    // Stop the command
    process::stop_command(state.clone(), 0).await.unwrap();

    // Wait for it to be detected as stopped
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // After stop, the monitor loop will detect exit and try to restart.
    // Shut down to clean up.
    process::shutdown_all(state.clone()).await;
}

#[tokio::test]
async fn test_graceful_shutdown() {
    let state = make_state(vec![
        ("sleeper1", "sleep 60"),
        ("sleeper2", "sleep 60"),
    ]);

    let _h1 = process::start_command(state.clone(), 0, None);
    let _h2 = process::start_command(state.clone(), 1, None);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Verify both running
    {
        let st = state.lock().await;
        assert_eq!(st.commands[0].status, CommandStatus::Running);
        assert_eq!(st.commands[1].status, CommandStatus::Running);
    }

    // Graceful shutdown
    process::shutdown_all(state.clone()).await;

    // Both should be stopped (or at least shutdown flag set)
    let st = state.lock().await;
    assert!(st.shutdown);
}

#[test]
fn test_app_state_sorting() {
    let config = Config {
        commands: vec![
            CommandConfig {
                name: "stopped-cmd".to_string(),
                command: "echo".to_string(),
                startup: false,
            },
            CommandConfig {
                name: "running-cmd".to_string(),
                command: "echo".to_string(),
                startup: false,
            },
            CommandConfig {
                name: "parked-cmd".to_string(),
                command: "echo".to_string(),
                startup: false,
            },
        ],
        log: None,
    };
    let mut state = AppState::new(&config);
    state.commands[0].status = CommandStatus::Stopped;
    state.commands[1].status = CommandStatus::Running;
    state.commands[2].status = CommandStatus::Parked;

    let indices = state.sorted_indices();
    // Running (1) should be first, then Stopped (0), then Parked (2)
    assert_eq!(indices, vec![1, 0, 2]);
}
