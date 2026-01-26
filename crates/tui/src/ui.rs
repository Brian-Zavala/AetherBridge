//! UI rendering functions
//!
//! This module contains all the Ratatui rendering logic for the TUI.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, InputMode, LogLevel, ServerState};

/// Primary colors for the UI
const ACCENT_COLOR: Color = Color::Cyan;
const SUCCESS_COLOR: Color = Color::Green;
const ERROR_COLOR: Color = Color::Red;
const WARNING_COLOR: Color = Color::Yellow;
const MUTED_COLOR: Color = Color::DarkGray;

/// Render the entire UI
pub fn render(frame: &mut Frame, app: &App) {
    // Main layout: Header, Content, Footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Header with status
            Constraint::Length(6),  // Browser panel
            Constraint::Min(5),     // Logs
            Constraint::Length(3),  // Help footer
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_browser_panel(frame, app, chunks[1]);
    render_logs(frame, app, chunks[2]);
    render_footer(frame, app, chunks[3]);

    // Render overlays
    if app.input_mode == InputMode::Help {
        render_help_overlay(frame);
    }

    if let InputMode::PortInput(ref current) = app.input_mode {
        render_port_input(frame, current);
    }
}

/// Render the header with server status
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let (status_text, status_color) = match &app.server_state {
        ServerState::Stopped => ("● Stopped", ERROR_COLOR),
        ServerState::Starting => ("◐ Starting...", WARNING_COLOR),
        ServerState::Running { port: _ } => ("● Running", SUCCESS_COLOR),
        ServerState::Error(_e) => ("● Error", ERROR_COLOR),
    };

    let status_line = match &app.server_state {
        ServerState::Running { port } => {
            format!("{}  http://{}:{}", status_text, app.host, port)
        }
        ServerState::Error(e) => format!("{}: {}", status_text, e),
        _ => status_text.to_string(),
    };

    let header_text = vec![
        Line::from(vec![
            Span::raw("  Status: "),
            Span::styled(status_line, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("  Provider: "),
            Span::styled(&app.provider, Style::default().fg(ACCENT_COLOR)),
            Span::styled(" (ide.google.com)", Style::default().fg(MUTED_COLOR)),
        ]),
        Line::from(vec![
            Span::raw("  Port: "),
            Span::styled(app.port.to_string(), Style::default().fg(Color::White)),
            Span::styled(" | Host: ", Style::default().fg(MUTED_COLOR)),
            Span::styled(&app.host, Style::default().fg(Color::White)),
        ]),
    ];

    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .title(format!(" AetherBridge v{} ", env!("CARGO_PKG_VERSION")))
                .title_style(Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(ACCENT_COLOR)),
        );

    frame.render_widget(header, area);
}

/// Render the browser detection panel
fn render_browser_panel(frame: &mut Frame, app: &App, area: Rect) {
    let browser_items: Vec<Line> = app
        .browsers
        .iter()
        .map(|browser| {
            let (icon, color) = if browser.available {
                ("✓", SUCCESS_COLOR)
            } else {
                ("✗", MUTED_COLOR)
            };

            let path_display = if browser.path.len() > 45 {
                format!("...{}", &browser.path[browser.path.len() - 42..])
            } else {
                browser.path.clone()
            };

            Line::from(vec![
                Span::styled(format!("  {} ", icon), Style::default().fg(color)),
                Span::styled(
                    format!("{:<10}", browser.name),
                    Style::default().fg(if browser.available { Color::White } else { MUTED_COLOR }),
                ),
                Span::styled(path_display, Style::default().fg(MUTED_COLOR)),
            ])
        })
        .collect();

    let panel = Paragraph::new(browser_items)
        .block(
            Block::default()
                .title(" Browsers ")
                .title_style(Style::default().fg(Color::White))
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(MUTED_COLOR)),
        );

    frame.render_widget(panel, area);
}

/// Render the log viewer with colored levels
fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    // Calculate max message width (area width - borders - timestamp - icon - padding)
    let max_msg_width = area.width.saturating_sub(22) as usize;

    let log_lines: Vec<Line> = app
        .logs
        .iter()
        .skip(app.log_scroll)
        .take(visible_height)
        .map(|entry| {
            let level_color = match entry.level {
                LogLevel::Info => MUTED_COLOR,
                LogLevel::Success => SUCCESS_COLOR,
                LogLevel::Warning => WARNING_COLOR,
                LogLevel::Error => ERROR_COLOR,
            };

            let level_icon = match entry.level {
                LogLevel::Info => "•",
                LogLevel::Success => "✓",
                LogLevel::Warning => "⚠",
                LogLevel::Error => "✗",
            };

            // Truncate message if too long
            let message = if entry.message.len() > max_msg_width {
                format!("{}…", &entry.message[..max_msg_width.saturating_sub(1)])
            } else {
                entry.message.clone()
            };

            Line::from(vec![
                Span::styled(
                    format!(" [{}] ", entry.timestamp),
                    Style::default().fg(MUTED_COLOR),
                ),
                Span::styled(
                    format!("{} ", level_icon),
                    Style::default().fg(level_color),
                ),
                Span::styled(message, Style::default().fg(Color::White)),
            ])
        })
        .collect();

    let total_logs = app.logs.len();
    let scroll_info = if total_logs > visible_height {
        let current_page = app.log_scroll / visible_height.max(1) + 1;
        let total_pages = (total_logs + visible_height - 1) / visible_height.max(1);
        format!(" Logs [{}/{}] ", current_page, total_pages)
    } else {
        " Logs ".to_string()
    };

    let logs = Paragraph::new(log_lines)
        .block(
            Block::default()
                .title(scroll_info)
                .title_style(Style::default().fg(Color::White))
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(MUTED_COLOR)),
        );

    frame.render_widget(logs, area);
}

/// Render the help footer
fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let help_text = match &app.input_mode {
        InputMode::Normal => {
            let server_action = if app.server_state == ServerState::Stopped { "Start" } else { "Stop" };
            Line::from(vec![
                Span::styled(" [S]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw(format!("{:<6}", server_action)),
                Span::styled("[C]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw("opy URL "),
                Span::styled("[P]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw("ort "),
                Span::styled("[R]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw("efresh "),
                Span::styled("[H]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw("elp "),
                Span::styled("[Q]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw("uit"),
            ])
        }
        InputMode::PortInput(_) => {
            Line::from(vec![
                Span::styled(" Enter port number, ", Style::default().fg(WARNING_COLOR)),
                Span::styled("[Enter]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw(" confirm, "),
                Span::styled("[Esc]", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
                Span::raw(" cancel"),
            ])
        }
        InputMode::Help => {
            Line::from(vec![
                Span::styled(" Press any key to close help", Style::default().fg(MUTED_COLOR)),
            ])
        }
    };

    let footer = Paragraph::new(help_text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(MUTED_COLOR)),
        );

    frame.render_widget(footer, area);
}

/// Render help overlay
fn render_help_overlay(frame: &mut Frame) {
    let area = centered_rect(60, 70, frame.area());

    // Clear the background
    frame.render_widget(Clear, area);

    let help_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Keybindings", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  S      ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Start/Stop the bridge server"),
        ]),
        Line::from(vec![
            Span::styled("  C      ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Copy server URL to clipboard"),
        ]),
        Line::from(vec![
            Span::styled("  P      ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Change port (when stopped)"),
        ]),
        Line::from(vec![
            Span::styled("  R      ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Refresh browser detection"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ↑/k    ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Scroll logs up"),
        ]),
        Line::from(vec![
            Span::styled("  ↓/j    ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Scroll logs down"),
        ]),
        Line::from(vec![
            Span::styled("  g/G    ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Jump to top/bottom of logs"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  H/?    ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Show this help"),
        ]),
        Line::from(vec![
            Span::styled("  Q/Esc  ", Style::default().fg(ACCENT_COLOR)),
            Span::raw("Quit application"),
        ]),
        Line::from(""),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .title_style(Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD))
                .borders(Borders::ALL)
                .border_set(border::DOUBLE)
                .border_style(Style::default().fg(ACCENT_COLOR)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(help, area);
}

/// Render port input overlay
fn render_port_input(frame: &mut Frame, current: &str) {
    let area = centered_rect(40, 20, frame.area());

    frame.render_widget(Clear, area);

    let input_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Port: "),
            Span::styled(current, Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)),
            Span::styled("_", Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::SLOW_BLINK)),
        ]),
        Line::from(""),
    ];

    let input = Paragraph::new(input_text)
        .block(
            Block::default()
                .title(" Configure Port ")
                .title_style(Style::default().fg(WARNING_COLOR))
                .borders(Borders::ALL)
                .border_set(border::DOUBLE)
                .border_style(Style::default().fg(WARNING_COLOR)),
        );

    frame.render_widget(input, area);
}

/// Helper to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
