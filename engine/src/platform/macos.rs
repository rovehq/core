//! macOS-specific platform functions

use std::path::PathBuf;
use sdk::errors::EngineError;

/// Default transport path for brain communication (Unix Domain Socket)
pub fn default_transport_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rove")
        .join("brain.sock")
}

/// Known llama-server installation paths on macOS
pub fn llama_search_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin/llama-server"),   // Homebrew Apple Silicon
        PathBuf::from("/usr/local/bin/llama-server"),       // Homebrew Intel / MacPorts
        PathBuf::from("/opt/local/bin/llama-server"),       // MacPorts
    ]
}

/// Get available RAM in bytes using sysctl
pub fn available_ram() -> u64 {
    use std::process::Command;
    
    // Try to get hw.memsize via sysctl
    if let Ok(output) = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
    {
        if let Ok(s) = String::from_utf8(output.stdout) {
            if let Ok(bytes) = s.trim().parse::<u64>() {
                return bytes;
            }
        }
    }
    
    // Fallback: assume 8GB if sysctl fails
    8 * 1024 * 1024 * 1024
}

/// Get a secret from macOS Keychain
pub fn keychain_get(key: &str) -> Result<String, EngineError> {
    use std::process::Command;
    
    // Use current user from environment
    let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            &username,
            "-s",
            key,
            "-w",
        ])
        .output()
        .map_err(|e| EngineError::KeyringError(format!("Failed to execute security command: {}", e)))?;
    
    if !output.status.success() {
        return Err(EngineError::KeyringError(format!(
            "Key '{}' not found in keychain",
            key
        )));
    }
    
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .map_err(|e| EngineError::KeyringError(format!("Invalid UTF-8 in keychain value: {}", e)))
}

/// Set a secret in macOS Keychain
pub fn keychain_set(key: &str, value: &str) -> Result<(), EngineError> {
    use std::process::Command;
    
    // Use current user from environment
    let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    
    // First try to delete existing entry (ignore errors)
    let _ = Command::new("security")
        .args([
            "delete-generic-password",
            "-a",
            &username,
            "-s",
            key,
        ])
        .output();
    
    // Add new entry
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            &username,
            "-s",
            key,
            "-w",
            value,
        ])
        .output()
        .map_err(|e| EngineError::KeyringError(format!("Failed to execute security command: {}", e)))?;
    
    if !output.status.success() {
        return Err(EngineError::KeyringError(format!(
            "Failed to store key '{}' in keychain",
            key
        )));
    }
    
    Ok(())
}
