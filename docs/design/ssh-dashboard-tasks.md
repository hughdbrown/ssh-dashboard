# SSH Dashboard — Implementation Task List

## Stage 1: Project Skeleton + Config

**Deliverable:** `cargo test` passes with config parsing tests.
**Files touched:** `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/config.rs`, `tests/config_test.rs`

### Task 1.1: Initialize Cargo project and module structure

- **Dependencies** (`Cargo.toml`):
  ```toml
  [dependencies]
  anyhow = "1.0"
  chrono = { version = "0.4", features = ["serde"] }
  clap = { version = "4.5", features = ["derive"] }
  crossterm = "0.28"
  dirs = "6.0"
  ratatui = "0.29"
  serde = { version = "1.0", features = ["derive"] }
  tokio = { version = "1", features = ["full"] }
  toml = "0.8"

  [dev-dependencies]
  tempfile = "3"
  ```
- **Files to create:**
  - `src/lib.rs` — declares modules: `pub mod config;`
  - `src/main.rs` — minimal main with clap arg parsing (`--config` path override), calls into lib
  - `src/config.rs` — empty placeholder with `pub struct Config`
- **CLI args** (clap derive in `src/main.rs`):
  - `--config <PATH>` — optional, overrides default `~/.ssh-dashboard/config.toml`
- **Verify:** `cargo check`

### Task 1.2: Config parsing with tests (test-first)

- **Tests** (`tests/config_test.rs`):
  - `test_config_parse_valid` — TOML with 2 commands and a log path → correct `Config` struct
  - `test_config_parse_defaults` — TOML without `log` field → default log path `~/.ssh-dashboard/history.log`
  - `test_config_parse_empty_commands` — TOML with `commands = []` → zero commands
  - `test_config_parse_invalid` — malformed TOML → descriptive error
- **Code** (`src/config.rs`):
  - `CommandConfig` struct: `name: String`, `command: String`, `startup: bool`
  - `Config` struct: `commands: Vec<CommandConfig>`, `log: Option<PathBuf>`
  - `Config::load(path: &Path) -> anyhow::Result<Config>` — reads and parses TOML
  - `Config::log_path(&self) -> PathBuf` — returns configured path or default
  - Derive `Deserialize` on both structs
- **Verify:** `cargo test`

### Task 1.3: Example config file

- **File:** `example-config.toml` (project root, for reference)
  ```toml
  # SSH Dashboard configuration
  # Place this at ~/.ssh-dashboard/config.toml

  log = "~/.ssh-dashboard/history.log"

  [[commands]]
  name = "agentsview"
  command = "agentsview"
  startup = true

  [[commands]]
  name = "ssh-tunnel-dev"
  command = "ssh -N -L 18789:127.0.0.1:18789 hughdbrown@10.0.0.211"
  startup = true

  [[commands]]
  name = "redis-local"
  command = "redis-server"
  startup = false
  ```
- **Verify:** manual review

---

## Stage 2: Process Manager Core

**Deliverable:** Integration tests pass with real child processes.
**Files touched:** `src/lib.rs`, `src/process.rs`, `src/state.rs`, `tests/process_test.rs`

### Task 2.1: App state types

- **Code** (`src/state.rs`):
  - `CommandStatus` enum: `Stopped`, `Running`, `Parked`
  - `CommandState` struct:
    - `config: CommandConfig` (clone of the command config)
    - `status: CommandStatus`
    - `pid: Option<u32>`
    - `started_at: Option<DateTime<Utc>>`
    - `restart_count: u32`
    - `recent_failures: Vec<DateTime<Utc>>` (timestamps of recent failures for circuit breaker)
  - `AppState` struct:
    - `commands: Vec<CommandState>`
    - `selected: usize` (TUI selection index)
    - `shutdown: bool`
  - `AppState::new(config: &Config) -> AppState`
  - `AppState::sorted_indices(&self) -> Vec<usize>` — returns indices sorted by status (Running, Stopped, Parked)
- **Tests** (`tests/process_test.rs` or unit tests in `src/state.rs`):
  - `test_app_state_sorting` — create state with mixed statuses, verify sorted order
- **Verify:** `cargo test`

### Task 2.2: Process spawning and monitoring

- **Code** (`src/process.rs`):
  - `spawn_command(cmd: &str) -> anyhow::Result<tokio::process::Child>` — splits command string, spawns with `tokio::process::Command`
  - `monitor_command(state: Arc<Mutex<AppState>>, index: usize)` — async fn:
    1. Spawn child process
    2. Update state to `Running` with PID
    3. Wait for child to exit
    4. Update state to `Stopped`
    5. Check circuit breaker: if <3 failures in 30s, auto-restart (loop); else set `Parked`
  - `stop_command(state: Arc<Mutex<AppState>>, index: usize)` — sends SIGTERM to child PID
  - `start_command(state: Arc<Mutex<AppState>>, index: usize)` — spawns a new `monitor_command` task
- **Tests** (`tests/process_test.rs`):
  - `test_process_start_success` — start `sleep 60`, verify state is `Running` with valid PID, then clean up
  - `test_process_exit_detected` — start `sleep 1`, wait 2s, verify state transitions to `Stopped` or auto-restarted
  - `test_process_auto_restart` — start `true` (exits immediately), wait, verify restart_count > 0
  - `test_process_park_after_3_failures` — start `false` (always fails), wait, verify state is `Parked` after 3 rapid failures
  - `test_process_manual_restart_parked` — park a command, call start, verify it transitions to `Running`
  - `test_process_stop_sends_sigterm` — start `sleep 60`, call stop, verify process terminates
- **Verify:** `cargo test`
- **Risks:** Tests that rely on timing (`sleep`, "within 30 seconds") may be flaky on slow CI. Use generous timeouts in tests. The circuit breaker window should be configurable (default 30s) to allow faster test windows.

### Task 2.3: Graceful shutdown

- **Code** (`src/process.rs`):
  - `shutdown_all(state: Arc<Mutex<AppState>>)` — async fn:
    1. Set `state.shutdown = true`
    2. Send SIGTERM to all running children
    3. Wait up to 5 seconds for each to exit
    4. Send SIGKILL to any still running
- **Tests** (`tests/process_test.rs`):
  - `test_graceful_shutdown` — start 2 `sleep 60` commands, call `shutdown_all`, verify both terminate within timeout
- **Verify:** `cargo test`

---

## Stage 3: Logging

**Deliverable:** Log file assertions pass in tests.
**Files touched:** `src/lib.rs`, `src/logging.rs`, `tests/logging_test.rs`

### Task 3.1: Event logger

- **Code** (`src/logging.rs`):
  - `EventKind` enum: `Started`, `Stopped { exit_code: Option<i32> }`, `Restarted`, `Parked`
  - `LogEntry` struct: `timestamp: DateTime<Utc>`, `command_name: String`, `event: EventKind`
  - `Logger` struct: holds a `PathBuf` and opens file in append mode
  - `Logger::new(path: &Path) -> anyhow::Result<Logger>`
  - `Logger::log(&self, entry: &LogEntry) -> anyhow::Result<()>` — writes formatted line
  - Format: `2026-02-27T10:30:00Z | agentsview | STARTED`
  - Format: `2026-02-27T10:30:05Z | agentsview | STOPPED (exit_code=1)`
  - Format: `2026-02-27T10:30:05Z | agentsview | PARKED (3 failures in 30s)`
- **Tests** (`tests/logging_test.rs`):
  - `test_logging_start_event` — log a Started event, read file, verify format
  - `test_logging_stop_event` — log a Stopped event with exit code, verify format
  - `test_logging_park_event` — log a Parked event, verify format
  - `test_logging_append` — log two events, verify both lines present
  - Use `tempfile::NamedTempFile` for test log files
- **Verify:** `cargo test`

### Task 3.2: Integrate logger with process manager

- **Code** (`src/process.rs`):
  - Add `logger: Arc<Logger>` parameter to `monitor_command` and `shutdown_all`
  - Log events at each state transition: spawn → STARTED, exit → STOPPED, restart → RESTARTED, park → PARKED
- **Code** (`src/state.rs`):
  - Add `logger: Arc<Logger>` to `AppState`
- **Verify:** `cargo test` (existing process tests still pass; logging integration verified by existing logging tests)

---

## Stage 4: TUI Display

**Deliverable:** Application launches and displays command state.
**Files touched:** `src/lib.rs`, `src/tui.rs`

### Task 4.1: Terminal setup and restore

- **Code** (`src/tui.rs`):
  - `init_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>>` — enable raw mode, enter alternate screen, create terminal
  - `restore_terminal(terminal: &mut Terminal<...>) -> anyhow::Result<()>` — disable raw mode, leave alternate screen
  - Install panic hook that restores terminal before printing panic
- **Verify:** `cargo check` (TUI setup is hard to unit test; verified by manual run)

### Task 4.2: Dashboard rendering

- **Code** (`src/tui.rs`):
  - `render(frame: &mut Frame, state: &AppState)` — renders the dashboard:
    - Title bar: "SSH Dashboard"
    - Table widget with columns: Name, Status, PID, Uptime, Restarts
    - Status column color-coded: green=Running, yellow=Stopped, red=Parked
    - Selected row highlighted
    - Footer: keybinding hints `[q] Quit  [Enter] Start/Stop  [r] Restart  [↑↓] Navigate`
  - Uptime calculation: `Utc::now() - started_at` formatted as `Xh Ym`
  - PID column: show PID for running, `-` for stopped/parked
- **Verify:** `cargo run` with example config (manual verification)

### Task 4.3: Main event loop

- **Code** (`src/tui.rs` or `src/lib.rs`):
  - `run_app(state: Arc<Mutex<AppState>>) -> anyhow::Result<()>` — main loop:
    1. Initialize terminal
    2. Loop:
       a. Lock state, render frame
       b. Poll for crossterm events with 200ms timeout
       c. If quit requested or shutdown flag set, break
    3. Restore terminal
- **Code** (`src/main.rs`):
  - Wire up: load config → create state → spawn startup commands → run TUI loop → shutdown on exit
- **Verify:** `cargo run` with example config

---

## Stage 5: Keyboard Controls + Signal Handling

**Deliverable:** Full interactive application works end-to-end.
**Files touched:** `src/tui.rs`, `src/process.rs`, `src/main.rs`

### Task 5.1: Keyboard input handling

- **Code** (`src/tui.rs`):
  - In the event loop, handle `KeyEvent`:
    - `q` → set shutdown flag, break loop
    - `Up` → decrement selected index (with wrapping)
    - `Down` → increment selected index (with wrapping)
    - `Enter` → if selected is Running, stop it; if Stopped/Parked, start it
    - `r` → stop (if running) then start the selected command
  - Start/stop actions send messages or directly call process module functions
- **Verify:** manual testing with example config

### Task 5.2: Signal handling

- **Code** (`src/main.rs` or `src/lib.rs`):
  - Spawn a tokio task that listens for `tokio::signal::ctrl_c()` and `SIGTERM`
  - On signal: call `shutdown_all`, then set the shutdown flag so the TUI loop exits
- **Verify:** `cargo run`, then press Ctrl+C — verify clean exit and terminal restoration

### Task 5.3: End-to-end wiring

- **Code** (`src/main.rs`):
  - Full startup sequence:
    1. Parse CLI args
    2. Load config (exit with helpful error if missing)
    3. Create logger
    4. Create shared AppState
    5. Spawn startup commands (those with `startup = true`)
    6. Spawn signal handler
    7. Run TUI event loop
    8. On TUI exit: shutdown_all, flush logs
- **Verify:** full manual test: start with example config, verify commands start, navigate, stop/start commands, Ctrl+C for clean exit

---

## Stage 6: Polish + Build Optimization

**Deliverable:** Clean `cargo clippy`, optimized release binary.
**Files touched:** `.cargo/config.toml`, `Cargo.toml`, various source files

### Task 6.1: Build optimization

- **File** (`.cargo/config.toml`):
  ```toml
  [target.aarch64-apple-darwin]
  rustflags = ["-C", "link-arg=-fuse-ld=/opt/homebrew/bin/lld"]
  ```
- **File** (`Cargo.toml` — add release profile):
  ```toml
  [profile.release]
  codegen-units = 1
  debug = false
  lto = true
  opt-level = "z"
  panic = "abort"
  strip = true
  ```
- **Verify:** `cargo build --release`

### Task 6.2: Clippy + formatting

- Run `cargo clippy -- -D warnings` and fix all warnings
- Run `cargo fmt`
- **Verify:** both pass cleanly

### Task 6.3: Config directory bootstrapping

- **Code** (`src/config.rs`):
  - If `~/.ssh-dashboard/` doesn't exist, create it
  - If `~/.ssh-dashboard/config.toml` doesn't exist, write the example config and tell the user
- **Verify:** delete `~/.ssh-dashboard/`, run binary, verify directory and example config are created

---

## Risk Register

| Risk | Mitigation |
|---|---|
| Timing-sensitive tests (circuit breaker) | Make the failure window configurable; use 1-second window in tests |
| `sleep`-based tests slow in CI | Use short durations (`sleep 0.5`) and generous async timeouts |
| Terminal not restored on panic | Panic hook installed in Task 4.1; tested by `panic!()` in debug mode |
| Child processes orphaned on crash | Panic hook calls shutdown; OS process groups as fallback |
| Config path with `~` not expanded | Use `dirs` crate for home dir; don't rely on shell expansion |
