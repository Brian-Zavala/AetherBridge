use axum::{
    routing::post,
    Router,
};
use clap::Parser;
use common::config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

mod routes;
mod state;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Path to browser profile for cookie extraction
    #[arg(long)]
    browser_profile: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let mut config = Config::default();

    // Override config with CLI args
    config.server.port = args.port;
    if let Some(profile) = args.browser_profile {
        config.server.browser_profile_path = Some(profile);
    }

    let automator = browser_automator::Automator::new(&config)?;
    let state = state::AppState::new(config.clone(), automator);

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
