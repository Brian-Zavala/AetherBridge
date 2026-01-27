//! Server creation and management utilities
//!
//! This module exposes the server logic for use by both the CLI binary
//! and the TUI application.

use axum::{routing::{get, post}, Router};
use common::config::Config;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower_http::trace::TraceLayer;

use crate::routes;
use crate::state::AppState;

/// Create the Axum router with all routes configured
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health and status endpoints
        .route("/", get(routes::health_check))
        .route("/health", get(routes::health))
        // OpenAI compatible endpoints
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/v1/models", get(routes::list_models))
        // Anthropic compatible endpoints
        .route("/v1/messages", post(routes::messages))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Server handle that can be used to shut down the server
pub struct ServerHandle {
    shutdown_tx: oneshot::Sender<()>,
}

impl ServerHandle {
    /// Signal the server to shut down gracefully
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
    }
}

/// Start the server in a background task, returning a handle for shutdown
pub async fn start_server(
    config: Config,
    host: &str,
    port: u16,
) -> anyhow::Result<ServerHandle> {
    let automator = browser_automator::Automator::new(&config)?;
    let state = AppState::new(config, automator);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    let listener = TcpListener::bind(addr).await?;

    let app = create_router(state);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Spawn the server in a background task
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
                tracing::info!("Received shutdown signal");
            })
            .await
            .ok();
    });

    tracing::info!("Server started on {}", addr);

    Ok(ServerHandle { shutdown_tx })
}

/// Start the server and block until it shuts down (for CLI usage)
pub async fn run_server_blocking(config: Config, host: &str, port: u16) -> anyhow::Result<()> {
    let automator = browser_automator::Automator::new(&config)?;
    let state = AppState::new(config, automator);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    let listener = TcpListener::bind(addr).await?;

    let app = create_router(state);

    tracing::info!("Server running on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
