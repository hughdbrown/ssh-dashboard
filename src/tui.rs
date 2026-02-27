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
use crate::state::{AppState, CommandStatus};

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

/// Render the dashboard UI.
fn render(frame: &mut Frame, state: &AppState) {
    let area = frame.area();

    let chunks = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Min(5),    // Table
        Constraint::Length(1), // Footer
    ])
    .split(area);

    render_title(frame, chunks[0]);
    render_table(frame, chunks[1], state);
    render_footer(frame, chunks[2]);
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

fn render_table(frame: &mut Frame, area: Rect, state: &AppState) {
    let sorted = state.sorted_indices();

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

    let rows: Vec<Row> = sorted
        .iter()
        .map(|&i| {
            let cs = &state.commands[i];
            let status_style = match cs.status {
                CommandStatus::Running => Style::default().fg(Color::Green),
                CommandStatus::Stopped => Style::default().fg(Color::Yellow),
                CommandStatus::Parked => Style::default().fg(Color::Red),
            };
            let pid_str = cs
                .pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string());
            let uptime_str = cs
                .started_at
                .as_ref()
                .map(format_uptime)
                .unwrap_or_else(|| "-".to_string());

            Row::new(vec![
                Cell::from(cs.config.name.clone()),
                Cell::from(cs.status.to_string()).style(status_style),
                Cell::from(pid_str),
                Cell::from(uptime_str),
                Cell::from(cs.restart_count.to_string()),
            ])
        })
        .collect();

    // Find which row in sorted order corresponds to state.selected
    let selected_row = sorted.iter().position(|&i| i == state.selected);

    let widths = [
        Constraint::Min(20),
        Constraint::Length(9),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL))
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
            // Only handle key press events (not release or repeat)
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') => {
                    break;
                }
                KeyCode::Up => {
                    let mut st = state.lock().await;
                    let len = st.commands.len();
                    if len > 0 {
                        let sorted = st.sorted_indices();
                        let current_pos =
                            sorted.iter().position(|&i| i == st.selected).unwrap_or(0);
                        let new_pos = if current_pos == 0 {
                            len - 1
                        } else {
                            current_pos - 1
                        };
                        st.selected = sorted[new_pos];
                    }
                }
                KeyCode::Down => {
                    let mut st = state.lock().await;
                    let len = st.commands.len();
                    if len > 0 {
                        let sorted = st.sorted_indices();
                        let current_pos =
                            sorted.iter().position(|&i| i == st.selected).unwrap_or(0);
                        let new_pos = (current_pos + 1) % len;
                        st.selected = sorted[new_pos];
                    }
                }
                KeyCode::Enter => {
                    let status = {
                        let st = state.lock().await;
                        let idx = st.selected;
                        if idx < st.commands.len() {
                            Some(st.commands[idx].status.clone())
                        } else {
                            None
                        }
                    };
                    if let Some(status) = status {
                        let idx = state.lock().await.selected;
                        match status {
                            CommandStatus::Running => {
                                let _ = process::stop_command(state.clone(), idx).await;
                            }
                            CommandStatus::Stopped | CommandStatus::Parked => {
                                // Reset failure state for manual start
                                {
                                    let mut st = state.lock().await;
                                    st.commands[idx].recent_failures.clear();
                                    st.commands[idx].status = CommandStatus::Stopped;
                                }
                                process::start_command(state.clone(), idx, logger.clone());
                            }
                        }
                    }
                }
                KeyCode::Char('r') => {
                    let idx = {
                        let st = state.lock().await;
                        st.selected
                    };
                    let status = {
                        let st = state.lock().await;
                        if idx < st.commands.len() {
                            Some(st.commands[idx].status.clone())
                        } else {
                            None
                        }
                    };
                    if let Some(status) = status {
                        // Stop if running
                        if status == CommandStatus::Running {
                            let _ = process::stop_command(state.clone(), idx).await;
                            // Wait briefly for the process to stop
                            tokio::time::sleep(Duration::from_millis(300)).await;
                        }
                        // Reset and restart
                        {
                            let mut st = state.lock().await;
                            st.commands[idx].recent_failures.clear();
                            st.commands[idx].status = CommandStatus::Stopped;
                        }
                        process::start_command(state.clone(), idx, logger.clone());
                    }
                }
                _ => {}
            }
        }

        // Check if shutdown was requested externally (e.g., signal handler)
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
