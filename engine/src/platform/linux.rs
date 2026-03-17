//! Linux-specific platform functions

use std::path::PathBuf;
use sdk::errors::EngineError;

/// Default transport path for brain communication (Unix Domain Socket)
pub fn default_transport_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rove")
        .join("brain.sock")
}

/// Known llama-server installation paths on Linux
pub fn llama_search_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/local/bin/llama-server"),
        PathBuf::from("/usr/bin/llama-server"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin/llama-server"),  // Homebrew on Linux
    ]
}

/// Get available RAM in bytes from /proc/meminfo
pub fn available_ram() -> u64 {
    use std::fs;
    
    // Read /proc/meminfo
    if let Ok(contents) = fs::read_to_string("/proc/meminfo") {
        for line in contents.lines() {
            if line.starts_with("MemAvailable:") {
                // Format: "MemAvailable:    12345678 kB"
                if let Some(value_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = value_str.parse::<u64>() {
                        return kb * 1024; // Convert KB to bytes
                    }
                }
            }
        }
    }
    
    // Fallback: assume 8GB if /proc/meminfo fails
    8 * 1024 * 1024 * 1024
}

/// Get a secret from Linux Secret Service (libsecret)
pub fn keychain_get(_key: &str) -> Result<String, EngineError> {
    // TODO: Implement using secret-service crate
    // For now, return error indicating not implemented
    Err(EngineError::KeyringError(
        "Linux keychain support requires secret-service crate (Phase 5)".to_string()
    ))
}

/// Set a secret in Linux Secret Service (libsecret)
pub fn keychain_set(_key: &str, _value: &str) -> Result<(), EngineError> {
    // TODO: Implement using secret-service crate
    // For now, return error indicating not implemented
    Err(EngineError::KeyringError(
        "Linux keychain support requires secret-service crate (Phase 5)".to_string()
    ))
}
