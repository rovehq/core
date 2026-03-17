use serde::{Deserialize, Serialize};

/// Plugin permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginPermissions {
    pub allowed_paths: Vec<String>,
    pub denied_paths: Vec<String>,
    pub max_file_size: Option<u64>,
    pub can_execute: bool,
    pub allowed_commands: Option<Vec<String>>,
    pub denied_flags: Option<Vec<String>>,
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
            max_file_size: Some(10 * 1024 * 1024),
            can_execute: false,
            allowed_commands: None,
            denied_flags: Some(vec![
                "--force".to_string(),
                "-rf".to_string(),
                "--delete".to_string(),
                "--hard".to_string(),
            ]),
            max_execution_time: Some(30),
        }
    }
}
