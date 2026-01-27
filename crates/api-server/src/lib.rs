//! AetherBridge API Server Library
//!
//! This crate provides the HTTP server for the AetherBridge platform,
//! exposing OpenAI-compatible API endpoints.

pub mod routes;
pub mod server;
pub mod state;

pub use server::{create_router, start_server, run_server_blocking, ServerHandle};
pub use state::AppState;
