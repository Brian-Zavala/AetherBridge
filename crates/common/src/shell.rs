use std::env;
use serde_json;
use std::fs::{OpenOptions, read_to_string};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Unknown,
}

impl Shell {
    /// Detect the current shell from the SHELL environment variable
    pub fn detect() -> Self {
        if let Ok(shell_path) = env::var("SHELL") {
            if shell_path.contains("bash") {
                return Shell::Bash;
            } else if shell_path.contains("zsh") {
                return Shell::Zsh;
            } else if shell_path.contains("fish") {
                return Shell::Fish;
            } else if shell_path.contains("pwsh") || shell_path.contains("powershell") {
                return Shell::PowerShell;
            }
        }
        Shell::Unknown
    }

    /// Get the configuration file path for the shell
    pub fn config_path(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        match self {
            Shell::Bash => Some(home.join(".bashrc")),
            Shell::Zsh => Some(home.join(".zshrc")),
            Shell::Fish => Some(home.join(".config").join("fish").join("config.fish")),
            Shell::PowerShell => None, // Windows/PowerShell profile logic is more complex, skipping for now
            Shell::Unknown => None,
        }
    }

    /// Append an environment variable export to the shell configuration
    pub fn export_env(&self, var: &str, val: &str) -> anyhow::Result<()> {
        let config_path = self.config_path().ok_or_else(|| anyhow::anyhow!("Unsupported shell or config path not found"))?;

        // Ensure parent directories exist (mainly for fish)
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = if config_path.exists() {
            read_to_string(&config_path)?
        } else {
            String::new()
        };

        let export_line = match self {
            Shell::Fish => format!("set -gx {} \"{}\"", var, val),
            _ => format!("export {}=\"{}\"", var, val),
        };

        // Check if already exists to avoid duplicates (simple check)
        if content.contains(&export_line) {
            return Ok(());
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config_path)?;

        writeln!(file, "\n# Added by AetherBridge")?;
        writeln!(file, "{}", export_line)?;

        Ok(())
    }

    pub fn name(&self) -> &'static str {
        match self {
            Shell::Bash => "Bash",
            Shell::Zsh => "Zsh",
            Shell::Fish => "Fish",
            Shell::PowerShell => "PowerShell",
            Shell::Unknown => "Unknown",
        }
    }

    /// Configures Claude Code to bypass onboarding and use AetherBridge
    pub fn configure_claude() -> anyhow::Result<()> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Home config not found"))?;
        let claude_home = home.join(".claude");
        let config_home_claude = home.join(".config").join("claude-code");

        std::fs::create_dir_all(&claude_home)?;
        std::fs::create_dir_all(&config_home_claude)?;

        // 1. Skip Onboarding Wizard (Global Config)
        let global_config = serde_json::json!({
            "hasCompletedOnboarding": true,
            "hasTrustDialogAccepted": true,
            "hasCompletedProjectOnboarding": true,
            "theme": "dark"
        });
        let global_content = serde_json::to_string_pretty(&global_config)?;

        std::fs::write(home.join(".claude.json"), &global_content)?;
        std::fs::write(config_home_claude.join("config.json"), &global_content)?;

        // 2. Configure Settings (CLEAN - No apiKeyHelper)
        let settings_config = serde_json::json!({
            "verbose": true,
            "autoUpdaterStatus": "disabled",
            "preferredTheme": "dark"
        });
        let settings_content = serde_json::to_string_pretty(&settings_config)?;

        std::fs::write(claude_home.join("settings.json"), &settings_content)?;

        Ok(())
    }
}
