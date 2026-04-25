//! Windows-specific platform functions

use keyring;
use sdk::errors::EngineError;
use std::path::PathBuf;

/// Default transport path for brain communication (Named Pipe)
pub fn default_transport_path() -> String {
    r"\\.\pipe\rove-brain".to_string()
}

/// Known llama-server installation paths on Windows
pub fn llama_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Winget installs under %LOCALAPPDATA%\Microsoft\WinGet\Packages\
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let winget_base = PathBuf::from(local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");

        // Would need to glob for llama-server.exe under here
        // For now, just add the base path
        paths.push(winget_base);
    }

    paths
}

/// Get available RAM in bytes using GlobalMemoryStatusEx
pub fn available_ram() -> u64 {
    // TODO: Implement using windows-sys crate
    // For now, return a reasonable default
    8 * 1024 * 1024 * 1024 // 8GB default
}

/// Approximate CPU load percentage.
pub fn cpu_load_percent() -> Option<u32> {
    None
}

/// Get a secret from Windows Credential Manager
pub fn keychain_get(key: &str) -> Result<String, EngineError> {
    keyring::Entry::new("rove", key)
        .map_err(|e| EngineError::KeyringError(e.to_string()))
        .and_then(|entry| {
            entry
                .get_password()
                .map_err(|e| EngineError::KeyringError(e.to_string()))
        })
}

/// Set a secret in Windows Credential Manager
pub fn keychain_set(key: &str, value: &str) -> Result<(), EngineError> {
    keyring::Entry::new("rove", key)
        .map_err(|e| EngineError::KeyringError(e.to_string()))
        .and_then(|entry| {
            entry
                .set_password(value)
                .map_err(|e| EngineError::KeyringError(e.to_string()))
        })
}
