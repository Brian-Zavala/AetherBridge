//! Application state and update logic

use anyhow::Result;
use common::config::Config;
use common::platform::{self, Browser};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::Stdout;
use std::process::Command;
use std::time::{Duration, Instant};
use std::sync::Arc;
use oauth::{OAuthFlow, AccountManager};

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

/// Wizard step state
#[derive(Debug, Clone, PartialEq)]
pub enum WizardState {
    Welcome,
    CheckProjectId,
    ProjectIdInput(String),
    ConfigureClaude,
    ExportShell(String),
    Finished,
}

/// Active input mode
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    PortInput(String),
    Help,
    Wizard(WizardState),
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
    /// Handle to the running server (for shutdown)
    server_handle: Option<api_server::ServerHandle>,
    /// OAuth account manager
    pub account_manager: Option<Arc<AccountManager>>,
    /// Connected account emails
    pub connected_accounts: Vec<String>,
    /// Is OAuth login in progress?
    pub login_in_progress: bool,
    /// Persistent configuration
    pub config: Config,
}

impl App {
    /// Create a new App instance
    pub fn new() -> Self {
        let browsers = Self::detect_browsers();
        let available_count = browsers.iter().filter(|b| b.available).count();

        // Load config or default
        let config = Config::load().unwrap_or_else(|e| {
            eprintln!("Failed to load config: {}", e);
            Config::default()
        });

        // Determine initial mode
        let input_mode = if config.project_id.is_none() {
            InputMode::Wizard(WizardState::Welcome)
        } else {
            InputMode::Normal
        };

        let mut app = Self {
            running: true,
            server_state: ServerState::Stopped,
            browsers,
            logs: Vec::new(),
            log_scroll: 0,
            port: config.server.port,
            provider: "Google".to_string(),
            input_mode,
            host: config.server.host.clone(),
            server_handle: None,
            account_manager: None,
            connected_accounts: Vec::new(),
            login_in_progress: false,
            config,
        };

        if matches!(app.input_mode, InputMode::Wizard(_)) {
            app.log_info("Welcome! Starting setup wizard...");
        } else {
            app.log_info("AetherBridge TUI started");
            app.log_info(format!("Detected {} available browser(s)", available_count));
            app.log_info("Press [H] for help, [S] to start server, [L] to login");
        }

        app
    }

    /// Initialize the account manager and load existing accounts
    pub async fn init_account_manager(&mut self) {
        match AccountManager::new().await {
            Ok(manager) => {
                let count = manager.account_count().await;
                self.connected_accounts = manager.get_account_emails().await;
                self.account_manager = Some(Arc::new(manager));

                if count > 0 {
                    self.log_success(format!("Loaded {} Google account(s)", count));
                } else {
                    if !matches!(self.input_mode, InputMode::Wizard(_)) {
                         self.log_info("No accounts configured. Press [L] to login.");
                    }
                }
            }
            Err(e) => {
                self.log_warning(format!("Account manager init failed: {}", e));
            }
        }
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
            InputMode::Wizard(state) => self.handle_wizard_key(key, state.clone()).await,
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
            // Login with Google
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.start_oauth_login().await;
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
                        self.config.server.port = port;
                         if let Err(e) = self.config.save() {
                             self.log_error(format!("Failed to save config: {}", e));
                         }
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

    /// Handle keys in wizard mode
    async fn handle_wizard_key(&mut self, key: KeyCode, state: WizardState) {
        match state {
            WizardState::Welcome => {
                // Any key to continue
                if matches!(key, KeyCode::Enter | KeyCode::Char(' ')) {
                    self.input_mode = InputMode::Wizard(WizardState::CheckProjectId);
                } else if matches!(key, KeyCode::Esc | KeyCode::Char('q')) {
                    self.running = false;
                }
            }
            WizardState::CheckProjectId => {
                 match key {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                         self.input_mode = InputMode::Wizard(WizardState::ProjectIdInput(String::new()));
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.log_info("Opening Google Cloud Console to create a project...");
                        if let Err(e) = open::that("https://console.cloud.google.com/projectcreate") {
                            self.log_error(format!("Failed to open browser: {}", e));
                            self.log_info("Please manually visit: https://console.cloud.google.com/projectcreate");
                        }
                        self.input_mode = InputMode::Wizard(WizardState::ProjectIdInput(String::new()));
                    }
                     KeyCode::Esc => {
                        self.running = false;
                    }
                    _ => {}
                }
            }
            WizardState::ProjectIdInput(current) => {
                match key {
                    KeyCode::Enter => {
                        if !current.is_empty() {
                            self.config.project_id = Some(current.clone());
                            if let Err(e) = self.config.save() {
                                self.log_error(format!("Failed to save config: {}", e));
                            } else {
                                self.log_success("Configuration saved!");
                            }

                            // Transition to ConfigureClaude instead of ExportShell directly
                            use common::shell::Shell;
                            let shell = Shell::detect();
                            if shell != Shell::Unknown && shell != Shell::PowerShell {
                                self.input_mode = InputMode::Wizard(WizardState::ConfigureClaude);
                            } else {
                                self.input_mode = InputMode::Wizard(WizardState::Finished);
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        let mut new = current;
                        new.pop();
                        self.input_mode = InputMode::Wizard(WizardState::ProjectIdInput(new));
                    }
                    KeyCode::Char(c) if c.is_ascii_graphic() => {
                        let mut new = current;
                        new.push(c);
                        self.input_mode = InputMode::Wizard(WizardState::ProjectIdInput(new));
                    }
                     KeyCode::Esc => {
                        self.running = false;
                    }
                    _ => {}
                }
            }
            WizardState::ConfigureClaude => {
                match key {
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                        use common::shell::Shell;
                        if let Err(e) = Shell::configure_claude() {
                            self.log_error(format!("Failed to configure Claude Code: {}", e));
                        } else {
                            self.log_success("Claude Code configured to bypass onboarding!");
                        }
                        // Move to ExportShell, passing project_id which we need to retrieve from config
                        if let Some(project_id) = &self.config.project_id {
                             self.input_mode = InputMode::Wizard(WizardState::ExportShell(project_id.clone()));
                        } else {
                             self.input_mode = InputMode::Wizard(WizardState::Finished);
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.log_warning("Skipping Claude Code configuration...");
                        if let Some(project_id) = &self.config.project_id {
                             self.input_mode = InputMode::Wizard(WizardState::ExportShell(project_id.clone()));
                        } else {
                             self.input_mode = InputMode::Wizard(WizardState::Finished);
                        }
                    }
                     KeyCode::Esc => {
                        self.running = false;
                    }
                    _ => {}
                }
            }
            WizardState::ExportShell(project_id) => {
                match key {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        use common::shell::Shell;
                        let shell = Shell::detect();
                        let mut success = true;

                        // Export PROJECT ID
                        if let Err(e) = shell.export_env("GOOGLE_CLOUD_PROJECT", &project_id) {
                            self.log_error(format!("Failed to export GOOGLE_CLOUD_PROJECT: {}", e));
                            success = false;
                        }

                        // Export Claude Code variables
                        if let Err(e) = shell.export_env("ANTHROPIC_BASE_URL", "http://127.0.0.1:8080") {
                             self.log_error(format!("Failed to export ANTHROPIC_BASE_URL: {}", e));
                             success = false;
                        }
                        if let Err(e) = shell.export_env("ANTHROPIC_API_KEY", "sk-ant-aetherbridge-bypass-key") {
                             self.log_error(format!("Failed to export ANTHROPIC_API_KEY: {}", e));
                             success = false;
                        }

                        if success {
                            self.log_success(format!("Added exports to {}", shell.name()));
                            self.log_info("Please restart your shell or run 'source <config_file>'");
                        }
                        self.input_mode = InputMode::Wizard(WizardState::Finished);
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.input_mode = InputMode::Wizard(WizardState::Finished);
                    }
                     KeyCode::Esc => {
                        self.running = false;
                    }
                    _ => {}
                }
            }
            WizardState::Finished => {
                self.input_mode = InputMode::Normal;
                self.log_info("Setup complete! You can now use AetherBridge.");
                self.log_info(format!("Detected {} available browser(s)", self.browsers.iter().filter(|b| b.available).count()));
                self.log_info("Press [H] for help, [S] to start server, [L] to login");
            }
        }
    }

    /// Toggle server start/stop
    async fn toggle_server(&mut self) {
        match &self.server_state {
            ServerState::Stopped | ServerState::Error(_) => {
                self.log_info(format!("Starting server on port {}...", self.port));
                self.server_state = ServerState::Starting;

                // Create config with auto-detected browser profile
                let mut config = Config::default();
                config.server.port = self.port;
                config.server.host = self.host.clone();
                // Prefer config path if set, otherwise detect
                config.server.browser_profile_path = self.config.server.browser_profile_path.clone()
                    .or_else(|| platform::detect_browser_profile().map(|p| p.to_string_lossy().to_string()));
                config.project_id = self.config.project_id.clone();


                // Actually start the server
                match api_server::start_server(config, &self.host, self.port).await {
                    Ok(handle) => {
                        self.server_handle = Some(handle);
                        self.server_state = ServerState::Running { port: self.port };
                        let url = format!("http://{}:{}", self.host, self.port);
                        self.log_success(format!("Server running at {}", url));
                        self.log_info("Press [C] to copy URL to clipboard");
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        self.server_state = ServerState::Error(error_msg.clone());
                        self.log_error(format!("Failed to start server: {}", error_msg));
                    }
                }
            }
            ServerState::Running { .. } => {
                self.log_info("Stopping server...");
                // Take ownership of the handle and shut it down
                if let Some(handle) = self.server_handle.take() {
                    handle.shutdown();
                }
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

    /// Start the OAuth login flow
    async fn start_oauth_login(&mut self) {
        if self.login_in_progress {
            self.log_warning("Login already in progress...");
            return;
        }

        self.login_in_progress = true;
        self.log_info("Starting Google OAuth login...");

        // Create OAuth flow
        let flow = OAuthFlow::new();
        let auth_url = flow.authorization_url();

        self.log_info("Opening browser for authentication...");
        self.log_info("Complete the login in your browser, then return here.");

        // Open browser
        if let Err(e) = open::that(&auth_url) {
            self.log_error(format!("Failed to open browser: {}", e));
            self.log_info(format!("Please manually open: {}", auth_url));
        }

        // Wait for the callback (with timeout)
        self.log_info("Waiting for authorization (5 minute timeout)...");

        match flow.wait_for_callback().await {
            Ok(code) => {
                self.log_success("Authorization code received!");
                self.log_info("Exchanging code for tokens...");

                match flow.exchange_code(&code).await {
                    Ok(token_pair) => {
                        self.log_success(format!("Logged in as: {}", token_pair.email));

                        // Add to account manager
                        // Clone the Arc to avoid borrow conflict
                        let manager_arc = self.account_manager.clone();

                        if let Some(manager) = manager_arc {
                            if let Err(e) = manager.add_account(token_pair.clone()).await {
                                self.log_warning(format!("Failed to save account: {}", e));
                            }
                            self.connected_accounts = manager.get_account_emails().await;
                        } else {
                            // Initialize account manager if not already done
                            match AccountManager::new().await {
                                Ok(manager) => {
                                    if let Err(e) = manager.add_account(token_pair.clone()).await {
                                        self.log_warning(format!("Failed to save account: {}", e));
                                    }
                                    self.connected_accounts = manager.get_account_emails().await;
                                    self.account_manager = Some(Arc::new(manager));
                                }
                                Err(e) => {
                                    self.log_error(format!("Failed to init account manager: {}", e));
                                }
                            }
                        }

                        self.log_success("Account added successfully!");
                        self.log_info("You can now use Antigravity models via OAuth.");
                    }
                    Err(e) => {
                        self.log_error(format!("Token exchange failed: {}", e));
                    }
                }
            }
            Err(e) => {
                self.log_error(format!("OAuth callback failed: {}", e));
            }
        }

        self.login_in_progress = false;
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
