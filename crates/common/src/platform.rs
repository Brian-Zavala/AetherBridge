use std::path::PathBuf;
use tracing::debug;

/// Supported browsers for cookie extraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Chromium,
    Brave,
    Edge,
}

impl Browser {
    /// Returns all browsers in preference order
    pub fn all() -> &'static [Browser] {
        &[Browser::Chrome, Browser::Chromium, Browser::Brave, Browser::Edge]
    }

    /// Returns the browser name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Browser::Chrome => "Google Chrome",
            Browser::Chromium => "Chromium",
            Browser::Brave => "Brave",
            Browser::Edge => "Microsoft Edge",
        }
    }
}

/// Detects the default browser profile path for the current platform.
/// Returns the first valid browser profile found, checking in order:
/// Chrome, Chromium, Brave, Edge.
pub fn detect_browser_profile() -> Option<PathBuf> {
    for browser in Browser::all() {
        if let Some(path) = get_browser_profile_path(*browser) {
            if path.exists() {
                debug!("Found {} profile at: {:?}", browser.name(), path);
                return Some(path);
            }
        }
    }
    None
}

/// Gets the browser profile path for a specific browser on the current platform.
pub fn get_browser_profile_path(browser: Browser) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    return get_browser_profile_linux(browser);

    #[cfg(target_os = "macos")]
    return get_browser_profile_macos(browser);

    #[cfg(target_os = "windows")]
    return get_browser_profile_windows(browser);

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        tracing::warn!("Unsupported platform for browser detection");
        None
    }
}

#[cfg(target_os = "linux")]
fn get_browser_profile_linux(browser: Browser) -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;

    let profile_path = match browser {
        Browser::Chrome => config_dir.join("google-chrome/Default"),
        Browser::Chromium => config_dir.join("chromium/Default"),
        Browser::Brave => config_dir.join("BraveSoftware/Brave-Browser/Default"),
        Browser::Edge => config_dir.join("microsoft-edge/Default"),
    };

    Some(profile_path)
}

#[cfg(target_os = "macos")]
fn get_browser_profile_macos(browser: Browser) -> Option<PathBuf> {
    let app_support = dirs::data_dir()?; // ~/Library/Application Support

    let profile_path = match browser {
        Browser::Chrome => app_support.join("Google/Chrome/Default"),
        Browser::Chromium => app_support.join("Chromium/Default"),
        Browser::Brave => app_support.join("BraveSoftware/Brave-Browser/Default"),
        Browser::Edge => app_support.join("Microsoft Edge/Default"),
    };

    Some(profile_path)
}

#[cfg(target_os = "windows")]
fn get_browser_profile_windows(browser: Browser) -> Option<PathBuf> {
    let local_app_data = dirs::data_local_dir()?; // %LOCALAPPDATA%

    let profile_path = match browser {
        Browser::Chrome => local_app_data.join("Google/Chrome/User Data/Default"),
        Browser::Chromium => local_app_data.join("Chromium/User Data/Default"),
        Browser::Brave => local_app_data.join("BraveSoftware/Brave-Browser/User Data/Default"),
        Browser::Edge => local_app_data.join("Microsoft/Edge/User Data/Default"),
    };

    Some(profile_path)
}

/// Gets the default configuration file path for AetherBridge.
/// - Linux: ~/.config/aetherbridge/config.toml
/// - macOS: ~/Library/Application Support/aetherbridge/config.toml
/// - Windows: %APPDATA%/aetherbridge/config.toml
pub fn get_config_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;
    Some(config_dir.join("aetherbridge/config.toml"))
}

/// Returns a human-readable string for the current OS.
pub fn get_os_name() -> &'static str {
    #[cfg(target_os = "linux")]
    return "Linux";

    #[cfg(target_os = "macos")]
    return "macOS";

    #[cfg(target_os = "windows")]
    return "Windows";

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return "Unknown";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_os_name() {
        let os = get_os_name();
        assert!(!os.is_empty());
    }

    #[test]
    fn test_browser_all() {
        let browsers = Browser::all();
        assert_eq!(browsers.len(), 4);
    }
}
