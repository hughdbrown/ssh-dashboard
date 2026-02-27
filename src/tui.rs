use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::{Frame, Terminal};
use tokio::sync::Mutex;

use crate::logging::Logger;
use crate::process;
use crate::state::{AppState, InstanceStatus, Section};

/// Initialize the terminal: enable raw mode, enter alternate screen.
pub fn init_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal: disable raw mode, leave alternate screen.
pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    terminal::disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Resume the terminal after an interactive command: re-enable raw mode, alternate screen.
pub fn resume_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    terminal::enable_raw_mode()?;
    terminal.backend_mut().execute(EnterAlternateScreen)?;
    terminal.clear()?;
    Ok(())
}

/// Install a panic hook that restores the terminal before printing the panic.
pub fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = terminal::disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        original_hook(panic_info);
    }));
}

/// Format a duration as "Xh Ym" or "Xm Ys".
fn format_uptime(started_at: &chrono::DateTime<Utc>) -> String {
    let elapsed = Utc::now() - *started_at;
    let total_secs = elapsed.num_seconds().max(0);
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}m")
    } else if mins > 0 {
        format!("{mins}m {secs:02}s")
    } else {
        format!("{secs}s")
    }
}

/// Render the dashboard UI with two sections.
fn render(frame: &mut Frame, state: &AppState) {
    let area = frame.area();

    // Calculate space: title + running section + available section + footer
    let running_height = (state.instances.len() as u16 + 2).max(3); // +2 for header+border, min 3
    let available_height = (state.available.len() as u16 + 2).max(3);

    let chunks = Layout::vertical([
        Constraint::Length(1),                // Title
        Constraint::Length(running_height),   // Running commands
        Constraint::Length(available_height), // Available commands
        Constraint::Min(0),                   // Spacer
        Constraint::Length(1),                // Footer
    ])
    .split(area);

    render_title(frame, chunks[0]);
    render_running_section(frame, chunks[1], state);
    render_available_section(frame, chunks[2], state);
    render_footer(frame, chunks[4]);
}

fn render_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![Span::styled(
        " SSH Dashboard ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    frame.render_widget(title, area);
}

fn render_running_section(frame: &mut Frame, area: Rect, state: &AppState) {
    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Status"),
        Cell::from("PID"),
        Cell::from("Uptime"),
        Cell::from("Restarts"),
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = state
        .instances
        .iter()
        .map(|inst| {
            let name = &state.available[inst.config_index].config.name;
            let status_style = match inst.status {
                InstanceStatus::Running => Style::default().fg(Color::Green),
                InstanceStatus::Parked => Style::default().fg(Color::Red),
            };
            let pid_str = inst
                .pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string());
            let uptime_str = inst
                .started_at
                .as_ref()
                .map(format_uptime)
                .unwrap_or_else(|| "-".to_string());

            Row::new(vec![
                Cell::from(name.clone()),
                Cell::from(inst.status.to_string()).style(status_style),
                Cell::from(pid_str),
                Cell::from(uptime_str),
                Cell::from(inst.restart_count.to_string()),
            ])
        })
        .collect();

    let selected_row = if state.section == Section::Running {
        Some(state.selected)
    } else {
        None
    };

    let widths = [
        Constraint::Min(20),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Running Commands "),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut table_state = TableState::default();
    table_state.select(selected_row);
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn render_available_section(frame: &mut Frame, area: Rect, state: &AppState) {
    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Command"),
        Cell::from("Startup"),
        Cell::from("Instances"),
    ])
    .style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = state
        .available
        .iter()
        .enumerate()
        .map(|(i, avail)| {
            let running = state.running_count(i);
            let instances_str = if running > 0 {
                format!("{running} running")
            } else {
                "-".to_string()
            };
            let instances_style = if running > 0 {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(avail.config.name.clone()),
                Cell::from(avail.config.command.clone()),
                Cell::from(if avail.config.startup { "yes" } else { "no" }),
                Cell::from(instances_str).style(instances_style),
            ])
        })
        .collect();

    let selected_row = if state.section == Section::Available {
        Some(state.selected)
    } else {
        None
    };

    let widths = [
        Constraint::Min(15),
        Constraint::Min(30),
        Constraint::Length(9),
        Constraint::Length(12),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Available Commands "),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut table_state = TableState::default();
    table_state.select(selected_row);
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" [q]", Style::default().fg(Color::Cyan)),
        Span::raw(" Quit  "),
        Span::styled("[Enter]", Style::default().fg(Color::Cyan)),
        Span::raw(" Start/Stop  "),
        Span::styled("[r]", Style::default().fg(Color::Cyan)),
        Span::raw(" Restart  "),
        Span::styled("[↑↓]", Style::default().fg(Color::Cyan)),
        Span::raw(" Navigate"),
    ]));
    frame.render_widget(footer, area);
}

/// Main TUI event loop. Renders the dashboard and handles keyboard input.
pub async fn run_app(state: Arc<Mutex<AppState>>, logger: Option<Arc<Logger>>) -> Result<()> {
    install_panic_hook();
    let mut terminal = init_terminal()?;

    loop {
        // Render
        {
            let st = state.lock().await;
            terminal.draw(|frame| render(frame, &st))?;
        }

        // Poll for events with 200ms timeout
        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') => {
                    break;
                }
                KeyCode::Up => {
                    let mut st = state.lock().await;
                    st.select_prev();
                }
                KeyCode::Down => {
                    let mut st = state.lock().await;
                    st.select_next();
                }
                KeyCode::Enter => {
                    let st = state.lock().await;
                    match st.section {
                        Section::Running => {
                            // Stop/kill the selected running instance
                            let idx = st.selected;
                            if idx < st.instances.len() {
                                let pid = st.instances[idx].pid;
                                drop(st);
                                if let Some(pid) = pid {
                                    unsafe {
                                        libc::kill(pid as libc::pid_t, libc::SIGTERM);
                                    }
                                }
                            }
                        }
                        Section::Available => {
                            let idx = st.selected;
                            if idx < st.available.len() {
                                let is_interactive = st.available[idx].config.interactive;
                                drop(st);
                                if is_interactive {
                                    // Create instance in state first
                                    let (instance_idx, name, cmd) = {
                                        let mut st = state.lock().await;
                                        let instance_idx = st.create_instance(idx);
                                        let name = st.instance_name(instance_idx).to_string();
                                        let cmd = st.instance_command(instance_idx).to_string();
                                        (instance_idx, name, cmd)
                                    };

                                    // Suspend the TUI so the user can see prompts
                                    restore_terminal(&mut terminal)?;

                                    println!("\n--- Starting interactive command ---");
                                    println!("  Name: {name}");
                                    println!("  Command: {cmd}");
                                    println!();

                                    match process::spawn_interactive_command(&cmd) {
                                        Ok(child) => {
                                            println!(
                                                "Process started. Enter password/passphrase if prompted."
                                            );
                                            println!("Press Enter to return to dashboard...");

                                            // Block until user presses Enter
                                            let mut input = String::new();
                                            let _ = std::io::stdin().read_line(&mut input);

                                            // Resume the TUI
                                            resume_terminal(&mut terminal)?;

                                            // Hand the child off to the process monitor
                                            process::start_instance_with_child(
                                                state.clone(),
                                                instance_idx,
                                                child,
                                                logger.clone(),
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to start command: {e}");
                                            println!("Press Enter to return to dashboard...");

                                            let mut input = String::new();
                                            let _ = std::io::stdin().read_line(&mut input);

                                            // Remove the instance we optimistically created
                                            {
                                                let mut st = state.lock().await;
                                                st.remove_instance(instance_idx);
                                                st.clamp_selection();
                                            }

                                            // Resume the TUI
                                            resume_terminal(&mut terminal)?;
                                        }
                                    }
                                } else {
                                    process::start_instance(state.clone(), idx, logger.clone());
                                }
                            }
                        }
                    }
                }
                KeyCode::Char('r') => {
                    // Restart: only makes sense for running instances
                    let st = state.lock().await;
                    if st.section == Section::Running {
                        let idx = st.selected;
                        if idx < st.instances.len() {
                            let pid = st.instances[idx].pid;
                            let config_idx = st.instances[idx].config_index;
                            drop(st);
                            // Kill existing
                            if let Some(pid) = pid {
                                unsafe {
                                    libc::kill(pid as libc::pid_t, libc::SIGTERM);
                                }
                            }
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            // Start new instance
                            process::start_instance(state.clone(), config_idx, logger.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // Check if shutdown was requested externally
        {
            let st = state.lock().await;
            if st.shutdown {
                break;
            }
        }
    }

    restore_terminal(&mut terminal)?;
    Ok(())
}
