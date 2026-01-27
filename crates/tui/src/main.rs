//! AetherBridge TUI - Interactive Terminal User Interface
//!
//! This is the main entry point for the TUI application.
//! It initializes the terminal, sets up the event loop, and runs the app.

mod app;
mod event;
mod ui;

use anyhow::Result;
use app::App;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use tracing::Level;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to file (not stdout, since we're using the terminal)
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(|| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/aether-bridge.log")
                .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap())
        })
        .init();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new();

    // Initialize OAuth account manager (loads existing accounts)
    app.init_account_manager().await;

    let result = app.run(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Handle any errors
    if let Err(err) = result {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }

    Ok(())
}
