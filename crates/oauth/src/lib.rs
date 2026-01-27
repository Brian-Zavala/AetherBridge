//! Google OAuth 2.0 implementation for Antigravity/Cloud Code Assist
//!
//! This crate provides OAuth authentication for accessing Google's Cloud Code
//! Assist API (Antigravity), enabling access to models like Gemini 3 and Claude 4.5.

pub mod constants;
pub mod flow;
pub mod storage;
pub mod tokens;
pub mod accounts;

pub use flow::OAuthFlow;
pub use storage::TokenStorage;
pub use tokens::{TokenPair, refresh_access_token};
pub use accounts::AccountManager;
