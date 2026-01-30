# 🌉 AetherBridge

**Your flat-rate AI subscription, now available as a local API.**

AetherBridge unlocks the power of Google's Cloud Code Assist (Antigravity) and turns it into a local, OpenAI and Anthropic compatible API server. This lets you use powerful AI development tools like **Claude Code**, **Gemini CLI**, or **Kuse Cowork** without paying expensive per-token API fees.

---

## ✨ Why AetherBridge?

- **💸 Zero Extra Cost**: Uses your Google Cloud Code Assist quota via OAuth.
- **🚀 Advanced Models**: Access **Gemini 3 Pro**, **Gemini 3 Flash**, and **Claude 4.5 Sonnet & Opus** (via Antigravity).
- **🧠 Thinking Mode**: Supports extended thinking/reasoning capabilities.
- **🛡️ Secure**: Standard Google OAuth 2.0 flow. No cookie extraction hacks needed.
- **🔌 Universal**:
  - OpenAI-compatible endpoint: `/v1/chat/completions`
  - Anthropic-compatible endpoint: `/v1/messages` (for Claude Code CLI)
- **🖥️ TUI Dashboard**: A beautiful terminal interface to manage connections and see logs.

---

## 🚦 Quick Start

### 1. Prerequisites
- **Rust** installed (version 1.85+)
- A Google account with access to **Project IDX** or **Cloud Code Assist**.

### 2. Install & Run
```bash
git clone https://github.com/Brian-Zavala/AetherBridge.git
cd AetherBridge
cargo run --release
```

### 3. Login
You'll see the AetherBridge TUI dashboard.
- **Press [L]** to start the OAuth login flow.
- A browser window will open. detailed to allow AetherBridge access.
- Once authenticated, your account email will appear in the dashboard.

### 4. Connect Your Tools

#### 🧠 Claude Code CLI
AetherBridge fully supports the official Claude Code CLI by spoofing the Anthropic API:
```bash
# Configure Claude Code to use AetherBridge
export ANTHROPIC_BASE_URL="http://127.0.0.1:8080"
export ANTHROPIC_API_KEY="aetherbridge"  # Dummy key

# Run the tool
claude

# ✅ Tool Use / Function Calling is fully supported!
# AetherBridge automatically handles tool schema compatibility and execution.
```

#### 💻 VS Code Extensions (Continue, Roo Code, etc.)
Configure as an **OpenAI-compatible** provider:
- **Base URL**: `http://127.0.0.1:8080/v1`
- **Model**: `antigravity-claude-sonnet-4-5` or `antigravity-gemini-3-pro`
- **API Key**: `dummy`

---

## 🤖 Available Models

AetherBridge maps requests to the following internal Antigravity models:

| Model ID (API) | Description |
|----------------|-------------|
| `antigravity-claude-sonnet-4-5` | Claude 4.5 Sonnet (Balanced) |
| `antigravity-claude-sonnet-4-5-thinking` | Claude 4.5 Sonnet with Thinking |
| `antigravity-claude-opus-4-5-thinking` | Claude 4.5 Opus (Reasoning) |
| `antigravity-gemini-3-pro` | Gemini 3 Pro (Reasoning) |
| `antigravity-gemini-3-flash` | Gemini 3 Flash (Speed) |

*Note: When using Claude Code CLI, standard Claude model names (e.g. `claude-3-5-sonnet-20241022`) are automatically mapped to the corresponding Antigravity model.*

---

## ❓ Troubleshooting

### "Rate Limited?"
> AetherBridge automatically handles rate limits. If you have multiple Google accounts, log in with all of them! AetherBridge will round-robin between accounts to maximize your throughput.

### "OAuth doesn't open?"
> Check your terminal output. If the browser doesn't open automatically, look for the authorization URL in the logs and copy-paste it into your browser manually.

---

## 🌍 Platform Notes

| OS | Notes |
|----|-------|
| **Linux** 🐧 | Uses `libsecret` (Gnome Keyring / KWallet) for secure token storage. |
| **Windows** 🪟 | Uses Windows Credential Manager. |
| **macOS** 🍎 | Uses macOS Keychain. |

---

*Built with ❤️ and 🦀 Rust. Free and Open Source.*
