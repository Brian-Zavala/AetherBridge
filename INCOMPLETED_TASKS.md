# Incomplete Tasks

## Context: What We've Built
AetherBridge now proxies requests to Google's Antigravity API using OAuth tokens, enabling CLI tools and VSCode extensions to access Claude 4.5 Sonnet/Opus and Gemini 3 models without API keys.

**Current Features:**
- ✅ OAuth 2.0 authentication with Google
- ✅ Multi-account support with rate limit rotation
- ✅ `/v1/messages` endpoint (Anthropic API format) for Claude Code CLI
- ✅ SSE streaming support
- ✅ Model spoofing (claude-3-* → antigravity models)
- ✅ TUI dashboard with OAuth login flow
- ✅ Robust Rate Limit Handling (Spoofing, Fallback, Session Distribution)
- ✅ Reliability Fixes (Header Injection for Thinking models, JSON Parsing Fixes)

---

## Phase 1: Verify Claude Code CLI Integration
- [x] Test Claude Code CLI with AetherBridge
    - [x] Set `ANTHROPIC_BASE_URL=http://127.0.0.1:8080`
    - [x] Run `claude` and verify responses
    - [x] Test streaming functionality (Fixed 400 errors and reliability)
    - [x] **Verify "Strategy 0" Rate Limit Fallback**: Confirmed working. `Gemini3Flash` now self-spoofs to trigger session rotation on 429s.
    - [x] **Verify Thinking Config**: Fixed 400 Bad Request by mapping Anthropic budgets to Gemini thinking levels.
- [ ] Implement Tool Use / Function Calling (Critical for Agentic features)
    - **Current Status**: Claude tools fail because Antigravity proxy strips tool definitions.
    - [ ] **Implement Google Search Wrapper**: Create a separate tool execution path using Gemini 3 Flash to perform searches and inject results, bypassing the stripping issue.
    - [ ] Map Anthropic tool definitions to Antigravity (native or via system prompt)
    - [ ] Verify Claude Code can execute tools (bash, glob, etc.) through the bridge
- [x] Debug and fix any Claude Code compatibility issues
    - [x] Verify SSE event format matches Anthropic spec
    - [x] Handle edge cases (Fixed 404s, 500s, 400s)
    - [x] Fix authentication conflicts (OAuth + Session Distribution)
- [ ] Improve User Onboarding UX (TUI-First Approach)
    - [ ] **Smart Environment Persistence**: TUI detects missing env vars and offers to append exports (`ANTHROPIC_BASE_URL`, `AETHER_PROJECT_ID`) to user's shell config (`.zshrc`/`.bashrc`).
    - [ ] **Interactive Project Setup Wizard**:
        - Checking for active projects on startup.
        - If missing, open browser to Project IDX/Cloud Console with one keypress.
        - Interactive guide to link/select the new project ID automatically.
    - [ ] **Rate Limit Handler**: Actionable modals for 429 errors with direct fix buttons.

- [ ] Document known limitations/workarounds

## Phase 2: Expand CLI Tool Support
- [ ] Test Gemini CLI integration
    - Research Gemini CLI API format requirements
    - Add endpoint mapping if needed
    - Verify authentication flow
- [ ] Test Kuse Cowork integration
    - Verify compatibility with `/v1/messages` endpoint
    - Document setup instructions
- [ ] Add CLI tool compatibility matrix to README

## Phase 3: VSCode Extension Compatibility
- [ ] Test Continue extension
    - Verify OpenAI endpoint compatibility
    - Test model selection and streaming
- [ ] Test Roo Code extension
    - Verify configuration format
    - Test multi-model support
- [ ] Test Cline extension
    - Verify API compatibility
    - Document any required settings
- [ ] Create VSCode configuration templates for each extension

## Phase 4: TUI Enhancements
- [ ] Add real-time connection monitoring
    - Show active API requests in TUI
    - Display which account is handling each request
- [ ] Add usage statistics panel
    - Track requests per model
    - Show rate limit status per account
- [ ] Add log viewer in TUI
    - Filter by log level
    - Search functionality

## Phase 5: Stability & Polish
- [ ] Improve error handling
    - Better error messages for common failures
    - Retry logic for transient errors
- [ ] Add `/v1/chat/completions` SSE streaming
    - Currently only `/v1/messages` has streaming
    - Required for some OpenAI-compatible tools
- [ ] Performance optimization
    - Connection pooling
    - Response caching (if applicable)

## Future / Low Priority
- [ ] Integration tests with mock OAuth server
- [ ] Windows installer (MSI/exe)
- [ ] macOS app bundle (.app)
- [ ] TOML configuration file support
- [ ] Tauri GUI (alternative to TUI)
- [ ] Account switching/removal in TUI
