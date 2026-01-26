//! Event handling for the TUI
//!
//! This module provides async event handling utilities.
//! Currently we use crossterm's built-in polling, but this
//! can be extended to support background tasks.

use crossterm::event::KeyEvent;
use tokio::sync::mpsc;

/// Event types the app can receive
#[derive(Debug)]
pub enum AppEvent {
    /// Keyboard input
    Key(KeyEvent),
    /// Periodic tick for updates
    Tick,
    /// Server status changed
    ServerStatus(ServerStatusEvent),
}

/// Server status events
#[derive(Debug)]
pub enum ServerStatusEvent {
    Started { port: u16 },
    Stopped,
    Error(String),
    Request { path: String, duration_ms: u64 },
}

/// Event sender for background tasks
pub type EventSender = mpsc::UnboundedSender<AppEvent>;
/// Event receiver for the main loop
pub type EventReceiver = mpsc::UnboundedReceiver<AppEvent>;

/// Create an event channel
pub fn create_channel() -> (EventSender, EventReceiver) {
    mpsc::unbounded_channel()
}
