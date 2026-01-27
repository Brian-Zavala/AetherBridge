# 🌉 AetherBridge

**Your flat-rate AI subscription, now available as a local API.**

AetherBridge unlocks the power of your existing browser sessions (like Google Antigravity or Project IDX) and turns them into a local, OpenAI-compatible API server. This lets you use powerful AI development tools like **Claude Code**, **OpenCode**, or **Kuse Cowork** without paying expensive per-token API fees.

---

## ✨ Why AetherBridge?

- **💸 Zero Extra Cost**: Uses the subscription you already pay for.
- **🚀 Turbo Performance**: Built in Rust for blazing speed and low memory usage.
- **🛡️ Privacy First**: Runs entirely on your local machine. No third-party proxy servers.
- **🔌 Universal**: Exposes standard OpenAI endpoints (`/v1/chat/completions`) that work with almost any AI tool.
- **🖥️ TUI Dashboard**: A beautiful terminal interface to manage everything.
- **🍪 Auto-Magic Auth**: Automatically finds your session cookies (Chrome, Brave, Edge, Chromium) without manual copying.

---

## 🚦 Quick Start

### 1. Prerequisites
- **Rust** installed (version 1.85+)
- A supported browser (Chrome, Brave, Edge, Chromium) logged into your AI provider (e.g., `ide.google.com`).

### 2. Install & Run
```bash
git clone https://github.com/Brian-Zavala/AetherBridge.git
cd AetherBridge
cargo run -p aether-tui
```

### 3. Using the Dashboard
You'll see a retro-style dashboard. It auto-detects your browser profiles.
- **Press [S]** to Start the bridge server.
- **Press [C]** to Copy your local API URL (`http://127.0.0.1:8080`).

---

## 🛠️ How to Connect Your Tools

Once the server is running (green status), configure your tools to point to **Localhost**:

### 🧠 Claude Code / CLI
```bash
export OPENAI_BASE_URL="http://localhost:8080/v1"
export OPENAI_API_KEY="dummy"  # Any string works
claude
```

### 💻 VS Code Extensions (Continue, OpenCode, etc.)
Edit your `config.json`:
```json
{
  "title": "AetherBridge (Google)",
  "provider": "openai",
  "model": "google-bridge",
  "apiBase": "http://localhost:8080/v1",
  "apiKey": "dummy"
}
```

### 🤖 Kuse Cowork
```bash
export ANTHROPIC_BASE_URL="http://localhost:8080/v1"
kuse cowork
```

---

## ❓ Troubleshooting

### "Server fails to start?"
> Check if port `8080` is already in use. You can change the port by pressing **[P]** in the dashboard.

### "Authentication failed?"
> AetherBridge needs to read your browser cookies.
> 1. Ensure you are logged into `ide.google.com` in your browser.
> 2. **Close your browser completely** (sometimes browsers lock the cookie database).
> 3. Restart AetherBridge.

### "No browser found?"
> We support default profiles for Chrome, Brave, Chromium, and Edge. If you use a custom profile or different browser, try exporting `AETHER_BROWSER_PROFILE=/path/to/profile` before running.

---

## 🌍 Platform Notes

| OS | Notes |
|----|-------|
| **Linux** 🐧 | Works best with `xclip` or `wl-copy` installed for clipboard support. |
| **Windows** 🪟 | **Must close browser** before starting to unlock cookie DB. |
| **macOS** 🍎 | Terminal may need "Full Disk Access" to read browser cookies. |

---

*Built with ❤️ and 🦀 Rust. Free and Open Source.*
