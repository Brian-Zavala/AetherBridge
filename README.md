# AetherBridge 🌉

**Unlock the power of your existing AI subscriptions.**

AetherBridge turns your active browser sessions (like Google Antigravity/Project IDX) into local, OpenAI-compatible API endpoints. Use powerful development tools like `Claude Code`, `OpenCode`, or `Kuse Cowork` with the flat-rate subscriptions you already pay for—no per-token costs.

---

## ✨ Features

- **Interactive TUI**: Beautiful terminal interface for easy management
- **Cross-Platform**: Seamless support for Windows, macOS, and Linux
- **Auto-Magic**: Automatically detects browser profiles (Chrome, Brave, Edge, Chromium)
- **Zero Config**: Works out-of-the-box with sensible defaults
- **Universal Compatibility**: Exposes standard `/v1/chat/completions` API

---

## 🚀 Quick Start

### 1. Prerequisites
- **Rust** (latest stable 1.85+)
- A Chromium-based browser (Chrome, Brave, Edge, Chromium) logged into your AI provider (e.g., `ide.google.com`)

### 2. Install & Run
```bash
git clone https://github.com/Brian-Zavala/AetherBridge.git
cd AetherBridge
cargo run -p aether-tui
```

### 3. Using the TUI
Once running, you'll see the interactive dashboard. Use these keys:

| Key | Action |
|-----|--------|
| **[S]** | **Start/Stop** the bridge server |
| **[C]** | **Copy** the local server URL to clipboard |
| **[P]** | **Port** configuration (default: 8080) |
| **[R]** | **Refresh** browser detection |
| **[H]** | **Help** overlay with full keybindings |
| **[Q]** | **Quit** application |

---

## 🛠️ Integration Examples

### connect with Claude Code
```bash
export OPENAI_BASE_URL="http://localhost:8080/v1"
export OPENAI_API_KEY="dummy"
claude
```

### VS Code Extensions (Continue/OpenCode)
Add this to your `config.json`:
```json
{
  "title": "AetherBridge (Google)",
  "provider": "openai",
  "model": "google-bridge",
  "apiBase": "http://localhost:8080/v1",
  "apiKey": "dummy"
}
```

---

## 🌍 Platform Specifics

### Linux 🐧
- **Clipboard Support**: Ensure you have `xclip`, `xsel`, or `wl-clipboard` installed.
- **Paths**: Auto-detects `~/.config/google-chrome`, `~/.config/chromium`, etc.

### Windows 🪟
- **Important**: Close your browser completely before starting AetherBridge (browsers lock cookie files).
- **Run as Admin**: May be required depending on your installation path.
- **Paths**: Auto-detects `%LOCALAPPDATA%` profiles.

### macOS 🍎
- **Permissions**: You may need to grant Terminal "Full Disk Access" to read browser cookies.
- **Paths**: Auto-detects `~/Library/Application Support/` profiles.

---

## ❓ Troubleshooting

**"Error sending request for url"**
> Your session cookies might be expired.
> 1. Close AetherBridge.
> 2. Open your browser and refresh `ide.google.com` to ensure you're logged in.
> 3. Close the browser completely.
> 4. Restart AetherBridge.

**"No browser profile detected"**
> Press **[R]** to refresh detection. If it persists, ensure you have launched your browser at least once and logged in.

---

## 💻 Development

```bash
# Run the TUI
cargo run -p aether-tui

# Run the CLI/Server directly
cargo run -p api-server -- serve
```

---

*Built with Rust 🦀 - [License: MIT](./LICENSE)*
