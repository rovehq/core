use serde::{Deserialize, Serialize};

/// Plugin permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginPermissions {
    pub allowed_paths: Vec<String>,
    pub denied_paths: Vec<String>,
    #[serde(default)]
    pub allowed_network_domains: Vec<String>,
    #[serde(default)]
    pub allowed_secret_keys: Vec<String>,
    #[serde(default)]
    pub secret_host_patterns: Vec<String>,
    #[serde(default)]
    pub memory_read: bool,
    #[serde(default)]
    pub memory_write: bool,
    pub max_file_size: Option<u64>,
    #[serde(default)]
    pub wasm_max_memory_mb: Option<u32>,
    pub can_execute: bool,
    pub allowed_commands: Option<Vec<String>>,
    pub denied_flags: Option<Vec<String>>,
    #[serde(default)]
    pub wasm_fuel_limit: Option<u64>,
    pub max_execution_time: Option<u64>,
}

impl Default for PluginPermissions {
    fn default() -> Self {
        Self {
            allowed_paths: vec!["workspace".to_string()],
            denied_paths: vec![
                ".ssh".to_string(),
                ".env".to_string(),
                "credentials".to_string(),
                "id_rsa".to_string(),
                "id_ed25519".to_string(),
            ],
            allowed_network_domains: Vec::new(),
            allowed_secret_keys: Vec::new(),
            secret_host_patterns: Vec::new(),
            memory_read: false,
            memory_write: false,
            max_file_size: Some(10 * 1024 * 1024),
            wasm_max_memory_mb: Some(10),
            can_execute: false,
            allowed_commands: None,
            denied_flags: Some(vec![
                "--force".to_string(),
                "-rf".to_string(),
                "--delete".to_string(),
                "--hard".to_string(),
            ]),
            wasm_fuel_limit: Some(50_000_000),
            max_execution_time: Some(60),
        }
    }
}
