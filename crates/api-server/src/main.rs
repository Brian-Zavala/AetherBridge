use axum::{
    routing::post,
    Router,
};
use common::config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

mod routes;
mod state;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::default();
    let automator = browser_automator::Automator::new(&config)?;
    let state = state::AppState::new(config, automator);

    let addr = SocketAddr::from(([127, 0, 0, 1], state.config.server.port));

    tracing::info!("Listening on {}", addr);

    let app = Router::new()
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/v1/messages", post(routes::messages))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
