use axum::{routing::post, Router};
use clap::{Parser, Subcommand};
use common::config::Config;
use common::platform;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::Level;

mod routes;
mod state;

#[derive(Parser, Debug)]
#[command(
    name = "aether-bridge",
    version,
    about = "A Rust-native Local AI Orchestration Platform",
    long_about = "AetherBridge bridges your web AI subscriptions (like Google Antigravity) to OpenAI-compatible API endpoints, allowing use with tools like Claude Code, OpenCode, or Gemini CLI."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Port to listen on
    #[arg(short, long, env = "AETHER_PORT", default_value_t = 8080, global = true)]
    port: u16,

    /// Host to bind to
    #[arg(short = 'H', long, env = "AETHER_HOST", default_value = "127.0.0.1", global = true)]
    host: String,

    /// Path to browser profile for cookie extraction (auto-detected if not specified)
    #[arg(short, long, env = "AETHER_BROWSER_PROFILE", global = true)]
    browser_profile: Option<String>,

    /// AI provider to use
    #[arg(short = 'P', long, env = "AETHER_PROVIDER", default_value = "google", global = true)]
    provider: String,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// Start the bridge server (default if no command specified)
    Serve,
    /// Show detected configuration and browser profiles
    Status,
    /// Print help for integrating with other tools
    Setup,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging based on verbosity
    let log_level = if args.verbose { Level::DEBUG } else { Level::INFO };
    tracing_subscriber::fmt().with_max_level(log_level).init();

    match args.command.clone().unwrap_or(Commands::Serve) {
        Commands::Serve => run_server(args).await,
        Commands::Status => show_status(args),
        Commands::Setup => show_setup(),
    }
}

async fn run_server(args: Args) -> anyhow::Result<()> {
    let mut config = Config::default();

    // Override config with CLI args
    config.server.port = args.port;
    config.server.host = args.host.clone();

    // Auto-detect browser profile if not specified
    config.server.browser_profile_path = args.browser_profile.or_else(|| {
        tracing::info!("Auto-detecting browser profile...");
        platform::detect_browser_profile().map(|p| {
            let path_str = p.to_string_lossy().to_string();
            tracing::info!("Detected browser profile: {}", path_str);
            path_str
        })
    });

    if config.server.browser_profile_path.is_none() {
        tracing::warn!(
            "No browser profile detected. Cookie extraction will fail. \
            Please log into your AI provider in a supported browser (Chrome, Brave, Edge, Chromium)."
        );
    }

    let automator = browser_automator::Automator::new(&config)?;
    let state = state::AppState::new(config.clone(), automator);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;

    println!();
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║             AetherBridge v{}                      ║", env!("CARGO_PKG_VERSION"));
    println!("╠════════════════════════════════════════════════════════════╣");
    println!("║  Server:    http://{}                        ║", addr);
    println!("║  Provider:  {:<46} ║", args.provider);
    println!("║  OS:        {:<46} ║", platform::get_os_name());
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Endpoints:");
    println!("  POST /v1/chat/completions  (OpenAI compatible)");
    println!("  POST /v1/messages          (Anthropic compatible)");
    println!();
    println!("Quick test:");
    println!("  curl http://{}/v1/chat/completions -d '{{\"model\":\"bridge\",\"messages\":[{{\"role\":\"user\",\"content\":\"Hello\"}}]}}'", addr);
    println!();

    tracing::info!("Starting server on {}", addr);

    let app = Router::new()
        .route("/v1/chat/completions", post(routes::chat_completions))
        .route("/v1/messages", post(routes::messages))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

fn show_status(args: Args) -> anyhow::Result<()> {
    println!("AetherBridge Status");
    println!("═══════════════════");
    println!();

    // OS Detection
    println!("Platform: {}", platform::get_os_name());
    println!();

    // Browser Detection
    println!("Detected Browser Profiles:");
    for browser in platform::Browser::all() {
        if let Some(path) = platform::get_browser_profile_path(*browser) {
            let status = if path.exists() { "✓" } else { "✗" };
            println!("  {} {} - {:?}", status, browser.name(), path);
        }
    }
    println!();

    // Current Configuration
    println!("Current Configuration:");
    println!("  Host: {}", args.host);
    println!("  Port: {}", args.port);
    println!("  Provider: {}", args.provider);
    if let Some(ref profile) = args.browser_profile {
        println!("  Browser Profile: {}", profile);
    } else if let Some(detected) = platform::detect_browser_profile() {
        println!("  Browser Profile: {:?} (auto-detected)", detected);
    } else {
        println!("  Browser Profile: None detected!");
    }
    println!();

    // Config file location
    if let Some(config_path) = platform::get_config_path() {
        let status = if config_path.exists() { "exists" } else { "not found" };
        println!("Config File: {:?} ({})", config_path, status);
    }

    Ok(())
}

fn show_setup() -> anyhow::Result<()> {
    println!("AetherBridge Setup Guide");
    println!("════════════════════════");
    println!();
    println!("1. PREREQUISITES");
    println!("   - Log into your AI provider (e.g., ide.google.com) in Chrome/Brave/Edge");
    println!("   - Ensure the browser is closed before starting AetherBridge");
    println!();
    println!("2. START THE SERVER");
    println!("   $ aether-bridge serve");
    println!("   or with custom port:");
    println!("   $ aether-bridge --port 9090 serve");
    println!();
    println!("3. CONFIGURE YOUR TOOLS");
    println!();
    println!("   Claude Code:");
    println!("   $ export OPENAI_BASE_URL=\"http://localhost:8080/v1\"");
    println!("   $ export OPENAI_API_KEY=\"dummy\"");
    println!("   $ claude");
    println!();
    println!("   Kuse Cowork:");
    println!("   $ export ANTHROPIC_BASE_URL=\"http://localhost:8080/v1\"");
    println!("   $ kuse cowork");
    println!();
    println!("   OpenCode / VS Code Extensions:");
    println!("   Set apiBase to: http://localhost:8080/v1");
    println!();
    println!("4. ENVIRONMENT VARIABLES");
    println!("   AETHER_PORT            - Override default port (8080)");
    println!("   AETHER_HOST            - Override bind address (127.0.0.1)");
    println!("   AETHER_BROWSER_PROFILE - Override browser profile path");
    println!("   AETHER_PROVIDER        - Set default provider (google)");

    Ok(())
}
