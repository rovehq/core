//! Configuration management
//!
//! This module handles loading, validation, and management of the Rove configuration.
//! Configuration is stored in TOML format at ~/.rove/config.toml.
//!
//! # Configuration Sections
//!
//! - **core**: Workspace path, log level, data directory
//! - **llm**: LLM provider settings and preferences
//! - **tools**: Core tool enablement flags
//! - **plugins**: Plugin enablement flags
//! - **security**: Risk tier and confirmation settings
//! - **brains**: Brains configuration (optional)
//! - **policy**: Policy files and activation behavior
//!
//! # Path Expansion
//!
//! The configuration system automatically:
//! - Expands ~ to the user's home directory
//! - Canonicalizes paths to resolve symlinks and .. patterns
//! - Verifies workspace is a directory
//! - Creates workspace directory if it doesn't exist
//!
//! # Platform-Specific Path Handling
//!
//! This module uses Rust's `std::path::Path` and `PathBuf` types, which automatically
//! handle platform-specific path separators (/ on Unix, \ on Windows). The `canonicalize()`
//! method resolves paths to their absolute form using the platform-specific separator.
//!
//! **Requirements**: 25.2 - Use platform-specific paths (/ on Unix, \ on Windows)
//!
//! # Examples
//!
//! ```no_run
//! use rove_engine::config::Config;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load configuration from default location
//! let config = Config::load_or_create()?;
//!
//! // Access configuration values
//! println!("Workspace: {:?}", config.core.workspace);
//! println!("Default provider: {}", config.llm.default_provider);
//! # Ok(())
//! # }
//! ```

pub mod agent;
pub mod brain;
pub mod core;
pub mod defaults;
pub mod gateway;
pub mod llm;
pub mod memory;
pub mod metadata;
pub mod policy;
pub mod security;
pub mod steering;
pub mod telegram;
pub mod tools;
pub mod transport;
pub mod webui;

pub use agent::*;
pub use brain::*;
pub use core::*;
pub use defaults::*;
pub use gateway::*;
pub use llm::*;
pub use memory::*;
pub use metadata::*;
pub use policy::*;
pub use security::*;
pub use telegram::*;
pub use tools::*;
pub use transport::*;
pub use webui::*;

use sdk::errors::EngineError;
use std::fs;
use std::path::{Path, PathBuf};
use toml::{map::Map, Value};

impl Config {
    /// Load configuration from the default location (~/.rove/config.toml)
    ///
    /// If the configuration file doesn't exist, creates a default one.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Home directory cannot be determined
    /// - File cannot be read or written
    /// - TOML parsing fails
    /// - Validation fails
    pub fn load_or_create() -> Result<Self, EngineError> {
        let config_path = Self::config_path()?;

        if config_path.exists() {
            Self::load_from_path(&config_path)
        } else {
            Self::create_default(&config_path)
        }
    }

    /// Load configuration from a specific path
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - File cannot be read
    /// - TOML parsing fails
    /// - Validation fails
    pub fn load_from_path(path: &Path) -> Result<Self, EngineError> {
        let contents = fs::read_to_string(path)
            .map_err(|e| EngineError::Config(format!("Failed to read config file: {}", e)))?;

        let mut raw: Value = toml::from_str(&contents)
            .map_err(|e| EngineError::Config(format!("Failed to parse config: {}", e)))?;
        normalize_public_aliases(&mut raw)?;

        let mut config: Config = raw
            .try_into()
            .map_err(|e| EngineError::Config(format!("Failed to parse config: {}", e)))?;

        // Clamp and validate configuration
        config.clamp_and_validate()?;

        Ok(config)
    }

    /// Create default configuration and save to path
    fn create_default(path: &Path) -> Result<Self, EngineError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                EngineError::Config(format!("Failed to create config directory: {}", e))
            })?;
        }

        let mut config = Self::default();
        config.clamp_and_validate()?;

        let toml_string = config_to_toml(&config)
            .map_err(|e| EngineError::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(path, &toml_string)
            .map_err(|e| EngineError::Config(format!("Failed to write config file: {}", e)))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms).map_err(|e| {
                EngineError::Config(format!("Failed to set config file permissions: {}", e))
            })?;
        }

        Ok(config)
    }

    /// Save configuration to the default path.
    pub fn save(&self) -> Result<(), EngineError> {
        let path = Self::config_path()?;
        self.save_to_path(&path)
    }

    /// Save configuration to a specific path after validation and clamping.
    pub fn save_to_path(&self, path: &Path) -> Result<(), EngineError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                EngineError::Config(format!("Failed to create config directory: {}", e))
            })?;
        }

        let mut config = self.clone();
        config.clamp_and_validate()?;

        let toml_string = config_to_toml(&config)
            .map_err(|e| EngineError::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(path, &toml_string)
            .map_err(|e| EngineError::Config(format!("Failed to write config file: {}", e)))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms).map_err(|e| {
                EngineError::Config(format!("Failed to set config file permissions: {}", e))
            })?;
        }

        Ok(())
    }

    /// Get the default configuration file path (~/.rove/config.toml)
    fn default_config_path() -> Result<PathBuf, EngineError> {
        if let Some(path) = std::env::var_os("ROVE_CONFIG_PATH").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(path));
        }

        let home = dirs::home_dir()
            .ok_or_else(|| EngineError::Config("Could not determine home directory".to_string()))?;
        Ok(home.join(".rove").join("config.toml"))
    }

    /// Resolve the effective configuration path, honoring `ROVE_CONFIG_PATH`.
    pub fn config_path() -> Result<PathBuf, EngineError> {
        Self::default_config_path()
    }

    /// Clamp all config values to valid ranges and validate
    fn clamp_and_validate(&mut self) -> Result<(), EngineError> {
        // Clamp memory config values
        self.memory.consolidation_interval_mins =
            self.memory.consolidation_interval_mins.clamp(5, 1440);
        self.memory.min_to_consolidate = self.memory.min_to_consolidate.max(1);
        self.memory.query_limit = self.memory.query_limit.clamp(1, 20);
        self.memory.min_importance_to_inject = self.memory.min_importance_to_inject.clamp(0.1, 0.9);

        // Validate and process
        self.validate_and_process()?;

        Ok(())
    }

    /// Validate configuration and expand paths
    fn validate_and_process(&mut self) -> Result<(), EngineError> {
        // Validate log level
        let valid_log_levels = ["error", "warn", "info", "debug", "trace"];
        if !valid_log_levels.contains(&self.core.log_level.as_str()) {
            return Err(EngineError::Config(format!(
                "Invalid log level '{}'. Must be one of: {}",
                self.core.log_level,
                valid_log_levels.join(", ")
            )));
        }

        // Validate default provider
        let valid_providers = ["ollama", "openai", "anthropic", "gemini", "nvidia_nim"];
        if !valid_providers.contains(&self.llm.default_provider.as_str()) {
            return Err(EngineError::Config(format!(
                "Invalid default provider '{}'. Must be one of: {}",
                self.llm.default_provider,
                valid_providers.join(", ")
            )));
        }

        // Validate thresholds
        if self.llm.sensitivity_threshold < 0.0 || self.llm.sensitivity_threshold > 1.0 {
            return Err(EngineError::Config(
                "sensitivity_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }
        if self.llm.complexity_threshold < 0.0 || self.llm.complexity_threshold > 1.0 {
            return Err(EngineError::Config(
                "complexity_threshold must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Validate max risk tier
        if self.security.max_risk_tier > 2 {
            return Err(EngineError::Config(
                "max_risk_tier must be 0, 1, or 2".to_string(),
            ));
        }

        // Expand and validate workspace path
        self.core.workspace = expand_path(&self.core.workspace)?;
        reject_dangerous_workspace(&self.core.workspace)?;
        self.core.workspace = canonicalize_or_create(&self.core.workspace)?;

        if !self.core.workspace.is_dir() {
            return Err(EngineError::Config(format!(
                "Workspace path is not a directory: {:?}",
                self.core.workspace
            )));
        }

        // Expand and validate data directory
        self.core.data_dir = expand_path(&self.core.data_dir)?;

        if !self.core.data_dir.exists() {
            fs::create_dir_all(&self.core.data_dir).map_err(|e| {
                EngineError::Config(format!("Failed to create data directory: {}", e))
            })?;
        }

        Ok(())
    }
}

fn config_to_toml(config: &Config) -> Result<String, toml::ser::Error> {
    let mut value = Value::try_from(config)?;
    let table = value
        .as_table_mut()
        .expect("Config serialization should produce a TOML table");

    if let Some(core) = table.get("core").cloned() {
        table.insert("kernel".to_string(), core);
    }

    if let Some(policy) = table.get("policy").cloned() {
        table.insert("policy".to_string(), canonical_policy_value(policy));
    } else if let Some(steering) = table.get("steering").cloned() {
        table.insert("policy".to_string(), canonical_policy_value(steering));
    }

    table.insert("services".to_string(), Value::Table(public_services_table(config)));
    table.insert("channels".to_string(), Value::Table(public_channels_table(config)));
    insert_public_brain_aliases(table, config)?;

    toml::to_string_pretty(&value)
}

fn normalize_public_aliases(value: &mut Value) -> Result<(), EngineError> {
    let table = value.as_table_mut().ok_or_else(|| {
        EngineError::Config("Config root must be a TOML table".to_string())
    })?;

    if !table.contains_key("core") {
        if let Some(kernel) = table.get("kernel").cloned() {
            table.insert("core".to_string(), kernel);
        }
    }

    if let Some(services) = table
        .get("services")
        .and_then(Value::as_table)
        .cloned()
    {
        if !table.contains_key("webui") {
            if let Some(webui) = services.get("webui").cloned() {
                table.insert("webui".to_string(), webui);
            }
        }

        if !table.contains_key("ws_client") {
            if let Some(remote) = services.get("remote").cloned() {
                table.insert("ws_client".to_string(), remote);
            }
        }

        if let Some(logging) = services.get("logging").and_then(Value::as_table) {
            let core = ensure_table(table, "core");
            if !core.contains_key("log_level") {
                if let Some(level) = logging.get("level").cloned() {
                    core.insert("log_level".to_string(), level);
                } else if let Some(enabled) = logging.get("enabled").and_then(Value::as_bool) {
                    core.insert(
                        "log_level".to_string(),
                        Value::String(if enabled { "info" } else { "error" }.to_string()),
                    );
                }
            }
        }
    }

    if !table.contains_key("telegram") {
        if let Some(channels) = table.get("channels").and_then(Value::as_table) {
            if let Some(telegram) = channels.get("telegram").cloned() {
                table.insert("telegram".to_string(), telegram);
            }
        }
    }

    if let Some(brains) = table.get("brains").and_then(Value::as_table).cloned() {
        let has_legacy_shape =
            brains.contains_key("enabled") || brains.contains_key("fallback") || brains.contains_key("ram_limit_mb");
        if !has_legacy_shape {
            if let Some(dispatch) = brains.get("dispatch").cloned() {
                table.insert("brains".to_string(), dispatch);
            }
        }
    }

    Ok(())
}

fn ensure_table<'a>(table: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    if !table.contains_key(key) {
        table.insert(key.to_string(), Value::Table(Map::new()));
    }
    table
        .get_mut(key)
        .and_then(Value::as_table_mut)
        .expect("table entry must be a TOML table")
}

fn canonical_policy_value(policy: Value) -> Value {
    let mut policy = policy;
    if let Some(table) = policy.as_table_mut() {
        if let Some(skill_dir) = table.remove("skill_dir") {
            table.insert("policy_dir".to_string(), skill_dir);
        }
        if let Some(default_skills) = table.remove("default_skills") {
            table.insert("default_policies".to_string(), default_skills);
        }
    }
    policy
}

fn public_services_table(config: &Config) -> Map<String, Value> {
    let mut services = Map::new();

    let mut logging = Map::new();
    logging.insert(
        "enabled".to_string(),
        Value::Boolean(!config.core.log_level.eq_ignore_ascii_case("error")),
    );
    logging.insert("level".to_string(), Value::String(config.core.log_level.clone()));
    services.insert("logging".to_string(), Value::Table(logging));

    services.insert(
        "webui".to_string(),
        Value::try_from(&config.webui).expect("webui config should serialize"),
    );
    services.insert(
        "remote".to_string(),
        Value::try_from(&config.ws_client).expect("remote config should serialize"),
    );

    let mut connector_engine = Map::new();
    connector_engine.insert(
        "enabled".to_string(),
        Value::Boolean(!config.mcp.servers.is_empty()),
    );
    connector_engine.insert(
        "configured_servers".to_string(),
        Value::Integer(config.mcp.servers.len() as i64),
    );
    services.insert(
        "connector_engine".to_string(),
        Value::Table(connector_engine),
    );

    services
}

fn public_channels_table(config: &Config) -> Map<String, Value> {
    let mut channels = Map::new();
    channels.insert(
        "telegram".to_string(),
        Value::try_from(&config.telegram).expect("telegram config should serialize"),
    );
    channels
}

fn insert_public_brain_aliases(
    table: &mut Map<String, Value>,
    config: &Config,
) -> Result<(), toml::ser::Error> {
    if !table.contains_key("brains") {
        table.insert("brains".to_string(), Value::Table(Map::new()));
    }
    let brains_table = table
        .get_mut("brains")
        .and_then(Value::as_table_mut)
        .expect("brains entry must be a TOML table");
    brains_table.insert(
        "dispatch".to_string(),
        Value::try_from(&config.brains)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::Config;

    #[test]
    fn load_accepts_public_section_aliases() {
        let base = std::env::current_dir()
            .expect("cwd")
            .join("target/config-tests")
            .join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&base).expect("base dir");
        let workspace = base.join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let config_path = base.join("config.toml");

        std::fs::write(
            &config_path,
            format!(
                r#"[kernel]
workspace = "{workspace}"
auto_sync = true
data_dir = "{data_dir}"

[llm]
default_provider = "ollama"
sensitivity_threshold = 0.7
complexity_threshold = 0.7

[security]
max_risk_tier = 2

[services.logging]
enabled = false
level = "error"

[services.webui]
enabled = true
bind_addr = "127.0.0.1:3788"

[services.remote]
enabled = true
url = "ws://127.0.0.1:4010/ws"
reconnect_delay_secs = 10

[channels.telegram]
enabled = true
allowed_ids = [123]

[policy]
default_policies = ["rust-safe"]
auto_detect = true
policy_dir = "{policy_dir}"

[brains.dispatch]
enabled = true
ram_limit_mb = 512
fallback = "ollama"
auto_unload = true
"#,
                workspace = workspace.display(),
                data_dir = base.join("data").display(),
                policy_dir = base.join("policy").display(),
            ),
        )
        .expect("write config");

        let config = Config::load_from_path(&config_path).expect("load config");
        assert_eq!(config.core.log_level, "error");
        assert!(config.webui.enabled);
        assert!(config.ws_client.enabled);
        assert!(config.telegram.enabled);
        assert_eq!(config.policy.default_policies, vec!["rust-safe".to_string()]);
        assert!(config.brains.enabled);
    }

    #[test]
    fn save_writes_public_alias_sections() {
        let base = std::env::current_dir()
            .expect("cwd")
            .join("target/config-tests")
            .join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&base).expect("base dir");
        let config_path = base.join("config.toml");
        let mut config = Config::default();
        config.core.workspace = base.join("workspace");
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        config.webui.enabled = true;
        config.ws_client.enabled = true;
        config.telegram.enabled = true;
        config.policy.default_policies = vec!["rust-safe".to_string()];
        config.brains.enabled = true;

        config.save_to_path(&config_path).expect("save config");
        let raw = std::fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("[kernel]"));
        assert!(raw.contains("[policy]"));
        assert!(raw.contains("[services.logging]"));
        assert!(raw.contains("[services.webui]"));
        assert!(raw.contains("[services.remote]"));
        assert!(raw.contains("[channels.telegram]"));
        assert!(raw.contains("[brains.dispatch]"));
    }
}

/// Expand ~ in path to user's home directory
fn expand_path(path: &Path) -> Result<PathBuf, EngineError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::Config("Invalid UTF-8 in path".to_string()))?;

    if let Some(rest) = path_str.strip_prefix("~/") {
        let home = dirs::home_dir()
            .ok_or_else(|| EngineError::Config("Could not determine home directory".to_string()))?;
        Ok(home.join(rest))
    } else if path_str == "~" {
        dirs::home_dir()
            .ok_or_else(|| EngineError::Config("Could not determine home directory".to_string()))
    } else {
        Ok(path.to_path_buf())
    }
}

/// Reject dangerous workspace paths (system roots)
fn reject_dangerous_workspace(path: &Path) -> Result<(), EngineError> {
    let dangerous_paths = ["/", "/usr", "/bin", "/sbin", "/etc", "/var", "/root"];
    let path_str = path.to_string_lossy();
    for dangerous in dangerous_paths {
        if path_str == dangerous || path_str.starts_with(&format!("{}/", dangerous)) {
            return Err(EngineError::Config(format!(
                "Dangerous workspace path rejected: {:?}",
                path
            )));
        }
    }
    Ok(())
}

/// Canonicalize path or create if doesn't exist
fn canonicalize_or_create(path: &Path) -> Result<PathBuf, EngineError> {
    if path.exists() {
        path.canonicalize()
            .map_err(|e| EngineError::Config(format!("Failed to canonicalize path: {}", e)))
    } else {
        Ok(path.to_path_buf())
    }
}
