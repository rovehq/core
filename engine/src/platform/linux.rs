//! Linux-specific platform functions

use keyring;
use sdk::errors::EngineError;
use std::path::PathBuf;

/// Default transport path for brain communication (Unix Domain Socket)
pub fn default_transport_path() -> PathBuf {
    crate::config::paths::rove_home().join("brain.sock")
}

/// Known llama-server installation paths on Linux
pub fn llama_search_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/local/bin/llama-server"),
        PathBuf::from("/usr/bin/llama-server"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin/llama-server"), // Homebrew on Linux
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

/// Approximate CPU load percentage from the 1-minute load average.
pub fn cpu_load_percent() -> Option<u32> {
    let mut samples = [0f64; 3];
    let count = unsafe { libc::getloadavg(samples.as_mut_ptr(), 3) };
    if count < 1 {
        return None;
    }
    let cores = std::thread::available_parallelism().ok()?.get() as f64;
    let normalized = (samples[0] / cores) * 100.0;
    Some(normalized.clamp(0.0, 999.0).round() as u32)
}

/// Get a secret from Linux Secret Service (libsecret / keyutils fallback)
pub fn keychain_get(key: &str) -> Result<String, EngineError> {
    keyring::Entry::new("rove", key)
        .map_err(|e| EngineError::KeyringError(e.to_string()))
        .and_then(|entry| {
            entry
                .get_password()
                .map_err(|e| EngineError::KeyringError(e.to_string()))
        })
}

/// Set a secret in Linux Secret Service (libsecret / keyutils fallback)
pub fn keychain_set(key: &str, value: &str) -> Result<(), EngineError> {
    keyring::Entry::new("rove", key)
        .map_err(|e| EngineError::KeyringError(e.to_string()))
        .and_then(|entry| {
            entry
                .set_password(value)
                .map_err(|e| EngineError::KeyringError(e.to_string()))
        })
}
