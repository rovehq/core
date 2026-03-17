//! Windows-specific platform functions

use std::path::PathBuf;
use sdk::errors::EngineError;

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
    8 * 1024 * 1024 * 1024  // 8GB default
}

/// Get a secret from Windows Credential Manager
pub fn keychain_get(_key: &str) -> Result<String, EngineError> {
    // TODO: Implement using windows-sys crate
    // For now, return error indicating not implemented
    Err(EngineError::KeyringError(
        "Windows keychain support requires windows-sys crate (Phase 5)".to_string()
    ))
}

/// Set a secret in Windows Credential Manager
pub fn keychain_set(_key: &str, _value: &str) -> Result<(), EngineError> {
    // TODO: Implement using windows-sys crate
    // For now, return error indicating not implemented
    Err(EngineError::KeyringError(
        "Windows keychain support requires windows-sys crate (Phase 5)".to_string()
    ))
}
