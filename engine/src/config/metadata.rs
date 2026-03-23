//! Centralized application metadata
//!
//! Single source of truth for version numbers, app name, descriptions,
//! and other metadata. Edit these values here before a release.
//!
//! CI reads the version from `Cargo.toml` (not secrets), so keep
//! `Cargo.toml` `version` in sync with `VERSION` below.

/// Semantic version — bump this for each release.
/// Must match `Cargo.toml [package] version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Persisted config schema version written to `config.toml`.
pub const CONFIG_SCHEMA_VERSION: u32 = 2;

/// Application name (short, lowercase)
pub const APP_NAME: &str = "rove";

/// Application display name (title-case, for UI)
pub const APP_DISPLAY_NAME: &str = "Rove";

/// One-line tagline
pub const TAGLINE: &str = "Local-first, plugin-driven AI agent engine";

/// Long description
pub const DESCRIPTION: &str = "Rove is a local-first, plugin-driven AI agent engine.\n\
Run `rove` with no arguments to enter interactive mode.";

/// Keychain / secret-store service name
pub const SERVICE_NAME: &str = "rove";

/// Default daemon port
pub const DEFAULT_PORT: u16 = 43177;

/// User-Agent string for outbound HTTP requests
pub fn user_agent() -> String {
    format!("{}/{}", APP_NAME, VERSION)
}

/// Engine banner line (e.g. "Rove Engine v0.0.2")
pub fn engine_banner() -> String {
    format!("{} Engine v{}", APP_DISPLAY_NAME, VERSION)
}
