#!/bin/bash
set -e

echo "🔧 AetherBridge: Fixing Claude Code Authentication (Final Polish)..."

# Directories
CLAUDE_HOME="$HOME/.claude"
CONFIG_HOME="$HOME/.config/claude-code"

mkdir -p "$CLAUDE_HOME"
mkdir -p "$CONFIG_HOME"

# 1. Skip Onboarding Wizard (Global Config)
GLOBAL_CONFIG='{
  "hasCompletedOnboarding": true,
  "hasTrustDialogAccepted": true,
  "hasCompletedProjectOnboarding": true,
  "theme": "dark"
}'

echo "$GLOBAL_CONFIG" > "$HOME/.claude.json"
echo "$GLOBAL_CONFIG" > "$CONFIG_HOME/config.json"
echo "✅ Created global config to skip onboarding wizard."

# 2. Configure Settings (CLEAN - No apiKeyHelper)
# To avoid "Auth conflict", we REMOVE apiKeyHelper and just rely on the env var.
SETTINGS_JSON='{
  "verbose": true,
  "autoUpdaterStatus": "disabled",
  "preferredTheme": "dark"
}'

echo "$SETTINGS_JSON" > "$CLAUDE_HOME/settings.json"
echo "✅ Created clean settings.json (conflict-free)."

# 3. Environment Variables Instructions
echo ""
echo "🎉 Fix applied! Now please run the following commands"
echo "   (or add them to your ~/.zshrc or ~/.bashrc):"
echo ""
echo "---------------------------------------------------------"
echo "export ANTHROPIC_BASE_URL=\"http://127.0.0.1:8080\""
echo "export ANTHROPIC_API_KEY=\"sk-ant-aetherbridge-bypass-key\""
echo "export GOOGLE_CLOUD_PROJECT=\"\${GOOGLE_CLOUD_PROJECT:-$AETHER_PROJECT_ID}\""
echo "---------------------------------------------------------"
echo ""
echo "👉 Then restart your shell and run: claude \"hi\""
