//! Application state and update logic

use anyhow::Result;
use common::platform::{self, Browser};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::Stdout;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::ui;

/// Server running state
#[derive(Debug, Clone, PartialEq)]
pub enum ServerState {
    Stopped,
    Starting,
    Running { port: u16 },
    Error(String),
}

impl ServerState {
    /// Get the server URL if running
    pub fn url(&self) -> Option<String> {
        match self {
            ServerState::Running { port } => Some(format!("http://127.0.0.1:{}", port)),
            _ => None,
        }
    }
}

/// Browser detection result
#[derive(Debug, Clone)]
pub struct BrowserInfo {
    pub name: String,
    pub path: String,
    pub available: bool,
}

/// Log entry with level
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub message: String,
    pub level: LogLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Active input mode
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    PortInput(String),
    Help,
}

/// Main application state
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Server state
    pub server_state: ServerState,
    /// Detected browsers
    pub browsers: Vec<BrowserInfo>,
    /// Log buffer
    pub logs: Vec<LogEntry>,
    /// Log scroll position
    pub log_scroll: usize,
    /// Current port
    pub port: u16,
    /// Provider name
    pub provider: String,
    /// Current input mode
    pub input_mode: InputMode,
    /// Host address
    pub host: String,
}

impl App {
    /// Create a new App instance
    pub fn new() -> Self {
        let browsers = Self::detect_browsers();
        let available_count = browsers.iter().filter(|b| b.available).count();

        let mut app = Self {
            running: true,
            server_state: ServerState::Stopped,
            browsers,
            logs: Vec::new(),
            log_scroll: 0,
            port: 8080,
            provider: "Google".to_string(),
            input_mode: InputMode::Normal,
            host: "127.0.0.1".to_string(),
        };

        app.log_info("AetherBridge TUI started");
        app.log_info(format!("Detected {} available browser(s)", available_count));
        app.log_info("Press [H] for help, [S] to start server");

        app
    }

    /// Detect available browsers
    fn detect_browsers() -> Vec<BrowserInfo> {
        Browser::all()
            .iter()
            .map(|browser| {
                let path = platform::get_browser_profile_path(*browser);
                let available = path.as_ref().map(|p| p.exists()).unwrap_or(false);
                BrowserInfo {
                    name: browser.name().to_string(),
                    path: path
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Not found".to_string()),
                    available,
                }
            })
            .collect()
    }

    /// Get current timestamp
    fn now() -> String {
        chrono::Local::now().format("%H:%M:%S").to_string()
    }

    /// Add a log entry with level
    fn log_with_level(&mut self, message: impl Into<String>, level: LogLevel) {
        self.logs.push(LogEntry {
            timestamp: Self::now(),
            message: message.into(),
            level,
        });
        // Auto-scroll to bottom (keep last 5 visible)
        if self.logs.len() > 5 {
            self.log_scroll = self.logs.len().saturating_sub(5);
        }
    }

    pub fn log_info(&mut self, message: impl Into<String>) {
        self.log_with_level(message, LogLevel::Info);
    }

    pub fn log_success(&mut self, message: impl Into<String>) {
        self.log_with_level(message, LogLevel::Success);
    }

    pub fn log_warning(&mut self, message: impl Into<String>) {
        self.log_with_level(message, LogLevel::Warning);
    }

    pub fn log_error(&mut self, message: impl Into<String>) {
        self.log_with_level(message, LogLevel::Error);
    }

    /// Copy text to system clipboard using system commands (more reliable on Linux)
    fn copy_to_clipboard(&mut self, text: &str) {
        let text_owned = text.to_string();

        // Try different clipboard commands based on what's available
        #[cfg(target_os = "linux")]
        {
            // Try xclip first, then xsel, then wl-copy for Wayland
            let result = Command::new("xclip")
                .args(["-selection", "clipboard"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text_owned.as_bytes())?;
                    }
                    child.wait()
                })
                .or_else(|_| {
                    Command::new("xsel")
                        .args(["--clipboard", "--input"])
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                        .and_then(|mut child| {
                            use std::io::Write;
                            if let Some(stdin) = child.stdin.as_mut() {
                                stdin.write_all(text_owned.as_bytes())?;
                            }
                            child.wait()
                        })
                })
                .or_else(|_| {
                    Command::new("wl-copy")
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                        .and_then(|mut child| {
                            use std::io::Write;
                            if let Some(stdin) = child.stdin.as_mut() {
                                stdin.write_all(text_owned.as_bytes())?;
                            }
                            child.wait()
                        })
                });

            match result {
                Ok(_) => self.log_success(format!("Copied: {}", text)),
                Err(_) => self.log_error("Install xclip, xsel, or wl-copy"),
            }
        }

        #[cfg(target_os = "macos")]
        {
            let result = Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(stdin) = child.stdin.as_mut() {
                        stdin.write_all(text_owned.as_bytes())?;
                    }
                    child.wait()
                });

            match result {
                Ok(_) => self.log_success(format!("Copied: {}", text)),
                Err(e) => self.log_error(format!("Copy failed: {}", e)),
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Use PowerShell on Windows
            let result = Command::new("powershell")
                .args(["-Command", &format!("Set-Clipboard -Value '{}'", text_owned)])
                .spawn()
                .and_then(|mut child| child.wait());

            match result {
                Ok(_) => self.log_success(format!("Copied: {}", text)),
                Err(e) => self.log_error(format!("Copy failed: {}", e)),
            }
        }
    }

    /// Copy server URL to clipboard
    fn copy_server_url(&mut self) {
        if let Some(url) = self.server_state.url() {
            self.copy_to_clipboard(&url);
        } else {
            self.log_warning("Server not running - nothing to copy");
        }
    }

    /// Run the main event loop
    pub async fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        let tick_rate = Duration::from_millis(100);
        let mut last_tick = Instant::now();

        while self.running {
            // Draw UI
            terminal.draw(|frame| ui::render(frame, self))?;

            // Handle events with timeout
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    // Only handle key press events (not release)
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code).await;
                    }
                }
            }

            // Tick updates
            if last_tick.elapsed() >= tick_rate {
                self.tick();
                last_tick = Instant::now();
            }
        }

        Ok(())
    }

    /// Handle keyboard input
    async fn handle_key(&mut self, key: KeyCode) {
        match &self.input_mode {
            InputMode::Normal => self.handle_normal_key(key).await,
            InputMode::PortInput(current) => self.handle_port_input(key, current.clone()),
            InputMode::Help => {
                // Any key exits help
                self.input_mode = InputMode::Normal;
            }
        }
    }

    /// Handle keys in normal mode
    async fn handle_normal_key(&mut self, key: KeyCode) {
        match key {
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => {
                self.log_info("Shutting down...");
                self.running = false;
            }
            // Start/Stop server
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.toggle_server().await;
            }
            // Refresh browsers
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.refresh_browsers();
            }
            // Copy URL to clipboard
            KeyCode::Char('c') | KeyCode::Char('C') => {
                self.copy_server_url();
            }
            // Change port
            KeyCode::Char('p') | KeyCode::Char('P') => {
                if self.server_state == ServerState::Stopped {
                    self.input_mode = InputMode::PortInput(self.port.to_string());
                    self.log_info("Enter new port number, then press Enter");
                } else {
                    self.log_warning("Stop the server first to change port");
                }
            }
            // Help
            KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Char('?') => {
                self.input_mode = InputMode::Help;
            }
            // Scroll logs up
            KeyCode::Up | KeyCode::Char('k') => {
                self.log_scroll = self.log_scroll.saturating_sub(1);
            }
            // Scroll logs down
            KeyCode::Down | KeyCode::Char('j') => {
                if self.log_scroll < self.logs.len().saturating_sub(1) {
                    self.log_scroll += 1;
                }
            }
            // Home - scroll to top
            KeyCode::Home | KeyCode::Char('g') => {
                self.log_scroll = 0;
            }
            // End - scroll to bottom
            KeyCode::End | KeyCode::Char('G') => {
                self.log_scroll = self.logs.len().saturating_sub(5);
            }
            _ => {}
        }
    }

    /// Handle keys in port input mode
    fn handle_port_input(&mut self, key: KeyCode, current: String) {
        match key {
            KeyCode::Enter => {
                if let Ok(port) = current.parse::<u16>() {
                    if port > 0 {
                        self.port = port;
                        self.log_success(format!("Port set to {}", port));
                    } else {
                        self.log_error("Invalid port number (must be 1-65535)");
                    }
                } else {
                    self.log_error("Invalid port number");
                }
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Esc => {
                self.log_info("Port change cancelled");
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                let mut new = current;
                new.pop();
                self.input_mode = InputMode::PortInput(new);
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let mut new = current;
                if new.len() < 5 {
                    new.push(c);
                }
                self.input_mode = InputMode::PortInput(new);
            }
            _ => {}
        }
    }

    /// Toggle server start/stop
    async fn toggle_server(&mut self) {
        match &self.server_state {
            ServerState::Stopped | ServerState::Error(_) => {
                self.log_info(format!("Starting server on port {}...", self.port));
                self.server_state = ServerState::Starting;

                // TODO: Actually start the server in a background task
                // For now, simulate startup
                self.server_state = ServerState::Running { port: self.port };
                let url = format!("http://{}:{}", self.host, self.port);
                self.log_success(format!("Server running at {}", url));
                self.log_info("Press [C] to copy URL to clipboard");
            }
            ServerState::Running { .. } => {
                self.log_info("Stopping server...");
                self.server_state = ServerState::Stopped;
                self.log_success("Server stopped");
            }
            ServerState::Starting => {
                self.log_warning("Server is starting, please wait...");
            }
        }
    }

    /// Refresh browser detection
    fn refresh_browsers(&mut self) {
        self.log_info("Refreshing browser detection...");
        self.browsers = Self::detect_browsers();
        let count = self.browsers.iter().filter(|b| b.available).count();
        self.log_success(format!("Found {} available browser(s)", count));
    }

    /// Periodic tick updates
    fn tick(&mut self) {
        // Future: update server stats, check health, etc.
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
