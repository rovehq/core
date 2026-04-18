use serde::{Deserialize, Serialize};

use crate::permission::PluginPermissions;

const DEFAULT_WASM_TIMEOUT_SECS: u64 = 60;
const DEFAULT_WASM_MAX_MEMORY_MB: u32 = 10;
const DEFAULT_WASM_FUEL_LIMIT: u64 = 50_000_000;

/// Core tool entry in the signed manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreToolEntry {
    pub name: String,
    pub version: String,
    pub path: String,
    pub hash: String,
    pub signature: String,
    pub platform: String,
}

impl CoreToolEntry {
    pub fn is_current_platform(&self) -> bool {
        self.platform == current_platform_tag()
    }
}

/// Plugin entry in the signed manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginEntry {
    pub name: String,
    pub version: String,
    pub path: String,
    pub hash: String,
    pub permissions: PluginPermissions,
    #[serde(default = "PluginEntry::default_allowed_imports")]
    pub allowed_imports: Vec<String>,
    #[serde(default)]
    pub trust_tier: u8,
}

impl PluginEntry {
    fn default_allowed_imports() -> Vec<String> {
        vec![
            "extism:host/env".to_string(),
            "wasi_snapshot_preview1".to_string(),
        ]
    }

    pub fn is_path_allowed(&self, path: &str) -> bool {
        for denied in &self.permissions.denied_paths {
            if path.contains(denied) {
                return false;
            }
        }

        if self.permissions.allowed_paths.is_empty() {
            return true;
        }

        self.permissions
            .allowed_paths
            .iter()
            .any(|allowed| path.starts_with(allowed) || allowed == "workspace")
    }

    pub fn is_command_allowed(&self, command: &str) -> bool {
        if !self.permissions.can_execute {
            return false;
        }

        if let Some(denied_flags) = &self.permissions.denied_flags {
            for flag in denied_flags {
                if command.contains(flag) {
                    return false;
                }
            }
        }

        if let Some(allowed_commands) = &self.permissions.allowed_commands {
            if allowed_commands.is_empty() {
                return true;
            }

            return allowed_commands
                .iter()
                .any(|allowed| command.starts_with(allowed));
        }

        true
    }

    pub fn is_network_host_allowed(&self, host: &str) -> bool {
        self.host_matches_patterns(host, &self.permissions.allowed_network_domains)
    }

    pub fn is_secret_declared(&self, key: &str) -> bool {
        self.permissions
            .allowed_secret_keys
            .iter()
            .any(|allowed| allowed == key)
    }

    pub fn is_secret_host_allowed(&self, host: &str) -> bool {
        self.host_matches_patterns(host, &self.permissions.secret_host_patterns)
    }

    fn host_matches_patterns(&self, host: &str, patterns: &[String]) -> bool {
        if patterns.is_empty() {
            return false;
        }

        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        patterns
            .iter()
            .map(|pattern| pattern.trim().trim_end_matches('.').to_ascii_lowercase())
            .any(|pattern| match pattern.as_str() {
                "*" => true,
                pattern if pattern.starts_with("*.") => {
                    let suffix = &pattern[2..];
                    host == suffix || host.ends_with(&format!(".{suffix}"))
                }
                _ => host == pattern,
            })
    }

    pub fn can_read_memory(&self) -> bool {
        self.permissions.memory_read
    }

    pub fn can_write_memory(&self) -> bool {
        self.permissions.memory_write
    }

    pub fn wasm_timeout_secs(&self) -> u64 {
        self.permissions
            .max_execution_time
            .unwrap_or(DEFAULT_WASM_TIMEOUT_SECS)
            .clamp(1, 600)
    }

    pub fn wasm_max_memory_mb(&self) -> u32 {
        self.permissions
            .wasm_max_memory_mb
            .unwrap_or(DEFAULT_WASM_MAX_MEMORY_MB)
            .clamp(1, 1024)
    }

    pub fn wasm_fuel_limit(&self) -> u64 {
        self.permissions
            .wasm_fuel_limit
            .unwrap_or(DEFAULT_WASM_FUEL_LIMIT)
            .max(1)
    }
}

fn current_platform_tag() -> String {
    format!("{}-{}", current_os_tag(), std::env::consts::ARCH)
}

#[cfg(target_os = "macos")]
fn current_os_tag() -> &'static str {
    "macos"
}

#[cfg(target_os = "linux")]
fn current_os_tag() -> &'static str {
    "linux"
}

#[cfg(target_os = "windows")]
fn current_os_tag() -> &'static str {
    "windows"
}

#[cfg(test)]
mod tests {
    use super::PluginEntry;
    use crate::permission::PluginPermissions;

    fn plugin_with_domains(domains: &[&str]) -> PluginEntry {
        let mut permissions = PluginPermissions::default();
        permissions.allowed_network_domains =
            domains.iter().map(|value| value.to_string()).collect();
        PluginEntry {
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            path: "test.wasm".to_string(),
            hash: "hash".to_string(),
            permissions,
            allowed_imports: vec![],
            trust_tier: 1,
        }
    }

    #[test]
    fn network_permissions_match_exact_hosts() {
        let plugin = plugin_with_domains(&["api.example.com"]);
        assert!(plugin.is_network_host_allowed("api.example.com"));
        assert!(!plugin.is_network_host_allowed("other.example.com"));
    }

    #[test]
    fn network_permissions_match_wildcards() {
        let plugin = plugin_with_domains(&["*.example.com"]);
        assert!(plugin.is_network_host_allowed("api.example.com"));
        assert!(plugin.is_network_host_allowed("example.com"));
        assert!(!plugin.is_network_host_allowed("example.org"));
    }

    #[test]
    fn secret_permissions_require_declared_key_and_host() {
        let mut permissions = PluginPermissions::default();
        permissions.allowed_secret_keys = vec!["OPENAI_API_KEY".to_string()];
        permissions.secret_host_patterns = vec!["api.openai.com".to_string()];
        let plugin = PluginEntry {
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            path: "test.wasm".to_string(),
            hash: "hash".to_string(),
            permissions,
            allowed_imports: vec![],
            trust_tier: 1,
        };

        assert!(plugin.is_secret_declared("OPENAI_API_KEY"));
        assert!(!plugin.is_secret_declared("OTHER_KEY"));
        assert!(plugin.is_secret_host_allowed("api.openai.com"));
        assert!(!plugin.is_secret_host_allowed("api.anthropic.com"));
    }

    #[test]
    fn wasm_limits_use_safe_defaults() {
        let plugin = PluginEntry::default();
        assert_eq!(plugin.wasm_timeout_secs(), 60);
        assert_eq!(plugin.wasm_max_memory_mb(), 10);
        assert_eq!(plugin.wasm_fuel_limit(), 50_000_000);
    }
}
