# AetherBridge

**AetherBridge** is a Rust-native Local AI Orchestration Platform that bridges your existing web AI subscriptions (like Google Antigravity/Project IDX) to API-compatible endpoints.

This allows you to use powerful developer tools that require an OpenAI-compatible API (like `Claude Code`, `OpenCode`, or `Gemini CLI`) using the flat-rate subscriptions you already pay for, without incurring per-token API costs.

## Architecture

AetherBridge operates as a local proxy server:

1.  **API Server**: Listens on `http://localhost:8080/v1` and accepts standard OpenAI Chat Completion requests.
2.  **Browser Automator**: Handles the interaction with the upstream Web AI providers.
    *   **Strategy A (Protocol)**: Extracts cookies from your local browser and impersonates web requests (high speed).
    *   **Strategy B (Visual)**: (Fallback) Captures screen content and uses OCR/Input simulation to drive the web UI directly.

## Prerequisites

*   Rust (latest stable)
*   Google Chrome / Chromium (for cookie extraction)
*   Linux (tested), macOS/Windows (experimental)

## Installation

```bash
git clone https://github.com/Brian-Zavala/AetherBridge.git
cd AetherBridge
cargo build --release
```

## Usage

### 1. Configuration (Optional)

By default, AetherBridge looks for Google cookies in your default Chrome profile. You can customize this by editing `config.toml` (if implemented) or modifying `crates/common/src/config.rs`.

### 2. Running the Bridge

```bash
./target/release/api-server
```

The server will start at `http://localhost:8080`.

**Options:**
- `--port <PORT>`: Bind to a specific port (default: 8080).
- `--browser-profile <PATH>`: Path to your browser profile for cookie extraction.

Example:
```bash
./target/release/api-server --port 9090 --browser-profile "/home/user/.config/google-chrome/Default"
```

### 3. Tool Integration

#### Claude Code

Configure Claude Code to use AetherBridge as a custom OpenAI provider:

```bash
export OPENAI_BASE_URL="http://localhost:8080/v1"
export OPENAI_API_KEY="dummy-key" # AetherBridge ignores the key
claude
```

#### OpenCode / Editor Extensions

For VS Code extensions like **Continue** or **OpenCode**, add a custom model configuration:

```json
{
  "title": "AetherBridge (Google)",
  "provider": "openai",
  "model": "google-bridge",
  "apiBase": "http://localhost:8080/v1",
  "apiKey": "dummy"
}
```

#### Gemini CLI

If using a CLI wrapper that supports custom endpoints:

```bash
gemini --base-url http://localhost:8080/v1 --key dummy "Hello world"
```

## Troubleshooting

### "Error: error sending request for url"
This usually means AetherBridge could not find valid session cookies for the provider.
1.  Open Chrome/Chromium.
2.  Log in to `https://ide.google.com` (or the target provider).
3.  Ensure your `browser_profile_path` in `crates/common` matches your actual profile location (e.g., `~/.config/google-chrome/Default`).
4.  Restart AetherBridge.

## License
MIT
