# SSH Dashboard — Product Requirements Document

## Overview

SSH Dashboard is a persistent TUI application that manages long-running commands (local services, SSH tunnels, port forwards) from a single terminal. It reads a TOML config file of commands, starts them as child processes, monitors their health, auto-restarts on failure, and displays live status in a ratatui-based dashboard. The problem it solves: long-running commands scattered across terminals that fail silently with no central visibility or control.

## Goals

- Display a live dashboard of all configured commands with status, PID, uptime, and restart count
- Auto-start commands marked with `startup = true` on launch
- Auto-restart commands that exit unexpectedly
- Park commands that fail 3 times within 30 seconds (circuit breaker)
- Allow manual start/stop/restart of any command via keyboard
- Log all events (start, stop, crash, restart, park) with timestamps
- Gracefully terminate all child processes on shutdown (SIGINT/SIGTERM)

## Non-Goals

- No remote access or web UI — this is a local terminal application
- No command output capture/display in v1 — just status monitoring
- No editing of config from within the TUI — edit the TOML file externally
- No Windows support — Unix signals and process management only
- No authentication or multi-user support

## Acceptance Criteria (as test descriptions)

1. `test_config_parse_valid` — A valid `config.toml` with two commands and a log path produces the correct `Config` struct with all fields populated.
2. `test_config_parse_defaults` — A `config.toml` without a `log` field defaults to `~/.ssh-dashboard/history.log`.
3. `test_config_parse_empty_commands` — A `config.toml` with an empty `[[commands]]` list produces a `Config` with zero commands.
4. `test_config_parse_invalid` — Malformed TOML returns a descriptive error.
5. `test_process_start_success` — Starting a command (e.g., `sleep 60`) spawns a child process and transitions state to `Running` with a valid PID.
6. `test_process_exit_detected` — When a running child process exits, the manager detects it and transitions state to `Stopped`.
7. `test_process_auto_restart` — When a running process exits, it is automatically restarted and the restart count increments.
8. `test_process_park_after_3_failures` — When a command fails to start 3 times within 30 seconds, state transitions to `Parked` and no further auto-restarts occur.
9. `test_process_manual_restart_parked` — A parked command can be manually restarted, resetting its failure counter.
10. `test_process_stop_sends_sigterm` — Stopping a running process sends SIGTERM to the child.
11. `test_graceful_shutdown` — On shutdown signal, all running child processes receive SIGTERM, and the manager waits up to 5 seconds before sending SIGKILL.
12. `test_logging_start_event` — Starting a command writes a timestamped "STARTED" entry to the log file.
13. `test_logging_stop_event` — A process exiting writes a timestamped "STOPPED" entry with the exit code.
14. `test_logging_park_event` — A command being parked writes a timestamped "PARKED" entry.
15. `test_app_state_sorting` — The command list is sorted: Running first, then Stopped, then Parked.

## Technical Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Async runtime | tokio (multi-thread) | Process monitoring needs concurrent task spawning; signal handling is built-in |
| TUI framework | ratatui 0.29 + crossterm 0.28 | Standard Rust TUI stack, cross-platform terminal backend |
| Config format | TOML via `toml` crate | Simple, human-readable, standard for Rust CLI tools |
| Serialization | serde with derive | Required by `toml` crate; idiomatic Rust |
| Error handling | `anyhow::Result` | Application code, not a library — anyhow is appropriate |
| CLI args | clap 4 with derive | Parse `--config` path override; follows skill conventions |
| Time | chrono | Timestamps in logs, uptime calculation |
| Config directory | dirs crate | Portable `~/.ssh-dashboard/` resolution |
| Process spawning | `tokio::process::Command` | Async child process management with signal support |
| Shared state | `Arc<tokio::sync::Mutex<AppState>>` | Shared between TUI render loop and process monitor tasks; tokio mutex because held across `.await` |
| Signal handling | `tokio::signal` | Async SIGINT/SIGTERM handling |

## Design and Operation

### User Perspective

```
SSH Dashboard
┌──────────────────────────────────────────────────────────────┐
│ Name              │ Status  │ PID    │ Uptime  │ Restarts   │
│───────────────────│─────────│────────│─────────│────────────│
│ agentsview        │ Running │ 12345  │ 2h 15m  │ 0          │
│ ssh-tunnel-dev    │ Running │ 12350  │ 1h 03m  │ 1          │
│ ssh-tunnel-prod   │ Stopped │ -      │ -       │ 0          │
│ redis-local       │ Parked  │ -      │ -       │ 3          │
└──────────────────────────────────────────────────────────────┘
 [q] Quit  [Enter] Start/Stop  [r] Restart  [↑↓] Navigate
```

**Keyboard controls:**
- `q` — Quit (graceful shutdown)
- `Enter` — Toggle: start a stopped/parked command, stop a running command
- `r` — Force restart a command (stop then start)
- `↑`/`↓` — Navigate the command list

### System Perspective

```
main.rs
  └─ parse CLI args (clap)
  └─ load config (config module)
  └─ initialize logger (logging module)
  └─ create AppState (shared state)
  └─ spawn process manager task (process module)
     └─ for each startup=true command: spawn monitor task
        └─ monitor task: spawn child → wait → log → restart or park
  └─ spawn signal handler task
     └─ on SIGINT/SIGTERM: set shutdown flag, terminate all children
  └─ run TUI event loop (tui module)
     └─ poll crossterm events (keyboard input)
     └─ render current state every tick (200ms)
     └─ on quit: trigger shutdown
```

**Data flow:**
1. Config is loaded once at startup → immutable `Config` struct
2. `AppState` holds mutable state: `Vec<CommandState>` (one per configured command)
3. Process monitor tasks update `CommandState` when processes start/stop/fail
4. TUI reads `AppState` each render tick to display current status
5. Keyboard events dispatch actions (start/stop/restart) through `AppState`

### State Machine per Command

```
                  ┌──────────┐
         ┌───────│  Stopped  │◄────────────┐
         │       └──────────┘              │
    start │                            manual stop
         │       ┌──────────┐              │
         └──────►│ Running  │──────────────┘
                 └──────────┘
                      │
                 exit detected
                      │
                      ▼
              ┌───────────────┐
              │ failures < 3  │──── yes ──► auto-restart → Running
              │ in 30 sec?    │
              └───────────────┘
                      │ no
                      ▼
                 ┌──────────┐
                 │  Parked  │──── manual restart ──► Running
                 └──────────┘
```

### Error Handling

| Failure mode | Handling |
|---|---|
| Config file missing | Exit with clear error message and example config path |
| Config file malformed | Exit with parse error details |
| Command not found (e.g., bad executable) | Log error, count as failure, apply circuit breaker |
| Process exits with non-zero | Log exit code, auto-restart (unless parked) |
| Process killed by signal | Log signal, auto-restart (unless parked) |
| TUI render error | Log and attempt to continue; terminal restoration on panic |
| Log file not writable | Print warning to stderr, continue without logging |

### Edge Cases

- **Empty config (no commands):** Dashboard shows empty table with a help message
- **All commands parked:** Dashboard shows all parked, user can manually restart
- **Rapid restarts:** Circuit breaker window is 30 seconds; 3 failures within that window parks the command
- **Long-running process then crash:** A process that ran for >30 seconds resets the failure window on next crash
- **Shutdown during restart:** If shutdown signal arrives while a command is restarting, cancel the restart and terminate

## Test Strategy

**Unit tests:**
- Config parsing (valid, invalid, defaults, edge cases)
- State transitions (the circuit breaker logic)
- Log formatting

**Integration tests:**
- Process lifecycle: start `sleep` commands, verify PID, send signals, verify exit detection
- Auto-restart: start a command that exits immediately (e.g., `true`), verify restart count
- Park behavior: start a command that always fails (`false`), verify it parks after 3 tries
- Graceful shutdown: start processes, send shutdown, verify all terminated

**Test infrastructure:**
- Use `tokio::test` for async tests
- Use `tempfile` crate for temporary log files and config files
- Use simple Unix commands (`sleep`, `true`, `false`, `echo`) as test commands
- No mocking of process spawning — test with real child processes for reliability

## Rollback and Safety

This is a new application — no existing behavior to break. The application is fully additive. If the binary is removed, no system state is affected. The config and log files in `~/.ssh-dashboard/` can be deleted without consequence.

The application restores terminal state on exit (including on panic via a panic hook). No risk of leaving the terminal in raw mode.

## Implementation Stages

### Stage 1: Project Skeleton + Config
Set up the Cargo project, module structure, config parsing, and CLI args. Deliverable: `cargo test` passes with config parsing tests.

### Stage 2: Process Manager Core
Implement process spawning, monitoring, auto-restart, and circuit breaker logic. Deliverable: integration tests pass with real child processes.

### Stage 3: Logging
Add event logging to file. Deliverable: log file assertions pass in tests.

### Stage 4: TUI Display
Build the ratatui dashboard showing command status. Deliverable: application launches and displays command state (manual verification).

### Stage 5: Keyboard Controls + Signal Handling
Wire up keyboard input for start/stop/restart and graceful shutdown on signals. Deliverable: full interactive application works end-to-end.

### Stage 6: Polish + Build Optimization
Add `.cargo/config.toml` for fast linker, release profile, clippy fixes, formatting. Deliverable: clean `cargo clippy`, optimized release binary.
