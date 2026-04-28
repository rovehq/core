//! Configuration management
//!
//! This module handles loading, validation, and management of the Rove configuration.
//! Configuration is stored in TOML format at ~/.rove/config.toml.
//!
//! # Configuration Sections
//!
//! - **config_schema_version**: persisted config schema version
//! - **config_written_by**: engine version that most recently wrote the file
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
pub mod approvals;
pub mod brain;
pub mod browser;
pub mod channel;
pub mod core;
pub mod daemon;
pub mod defaults;
pub mod extensions;
pub mod gateway;
pub mod llm;
pub mod loadout;
pub mod memory;
pub mod metadata;
pub mod paths;
pub mod policy;
pub mod profile;
pub mod remote;
pub mod search;
pub mod secrets;
pub mod security;
pub mod steering;
pub mod telegram;
pub mod tools;
pub mod transport;
pub mod update;
pub mod voice;
pub mod wasm;
pub mod webui;

pub use agent::*;
pub use approvals::*;
pub use brain::*;
pub use browser::*;
pub use channel::Channel;
pub use core::*;
pub use daemon::*;
pub use defaults::*;
pub use extensions::ExtensionsConfig;
pub use gateway::*;
pub use llm::*;
pub use loadout::*;
pub use memory::*;
pub use metadata::*;
pub use paths::rove_home;
pub use policy::*;
pub use profile::*;
pub use remote::*;
pub use search::*;
pub use secrets::*;
pub use security::*;
pub use telegram::*;
pub use tools::*;
pub use transport::*;
pub use update::UpdateConfig;
pub use voice::*;
pub use wasm::*;
pub use webui::*;

use sdk::config_handle::{
    ApprovalConfigSnapshot, ChannelsConfigSnapshot, ConfigMetadataSnapshot, CoreConfigSnapshot,
    DaemonConfigSnapshot, LlmConfigSnapshot, SecretConfigSnapshot, ServicesConfigSnapshot,
    VersionedConfigSnapshot,
};
use sdk::errors::EngineError;
use std::fs;
use std::fs::OpenOptions;
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
        migrate_custom_providers(&mut raw);

        let mut config: Config = raw
            .try_into()
            .map_err(|e| EngineError::Config(format!("Failed to parse config: {}", e)))?;

        config.apply_env_overrides();

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

    /// Get the default configuration file path (`$ROVE_HOME/config.toml`).
    fn default_config_path() -> Result<PathBuf, EngineError> {
        if let Some(path) = std::env::var_os("ROVE_CONFIG_PATH").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(path));
        }

        if dirs::home_dir().is_none() {
            return Err(EngineError::Config(
                "Could not determine home directory".to_string(),
            ));
        }

        Ok(paths::rove_home().join("config.toml"))
    }

    /// Resolve the effective configuration path, honoring `ROVE_CONFIG_PATH`.
    pub fn config_path() -> Result<PathBuf, EngineError> {
        Self::default_config_path()
    }

    /// Clamp all config values to valid ranges and validate
    fn clamp_and_validate(&mut self) -> Result<(), EngineError> {
        if self.config_schema_version > metadata::CONFIG_SCHEMA_VERSION {
            return Err(EngineError::Config(format!(
                "Config schema version {} is newer than this engine supports ({})",
                self.config_schema_version,
                metadata::CONFIG_SCHEMA_VERSION
            )));
        }
        self.config_schema_version = metadata::CONFIG_SCHEMA_VERSION;
        self.config_written_by = metadata::VERSION.to_string();

        // Clamp memory config values
        self.memory.consolidation_interval_mins =
            self.memory.consolidation_interval_mins.clamp(5, 1440);
        self.memory.min_to_consolidate = self.memory.min_to_consolidate.max(1);
        self.memory.query_limit = self.memory.query_limit.clamp(1, 20);
        self.memory.min_importance_to_inject = self.memory.min_importance_to_inject.clamp(0.1, 0.9);

        // Clamp WASM runtime defaults
        self.wasm.normalize();

        // Update + extension channel metadata
        self.update.normalize();
        self.extensions.normalize();

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

        // Validate default provider — built-ins or any configured custom provider are valid.
        let builtin_providers = ["ollama", "openai", "anthropic", "gemini", "nvidia_nim"];
        let custom_provider_names: Vec<&str> = self
            .llm
            .custom_providers
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        let is_valid_provider = builtin_providers.contains(&self.llm.default_provider.as_str())
            || custom_provider_names.contains(&self.llm.default_provider.as_str());
        if !is_valid_provider {
            let mut all_valid = builtin_providers.to_vec();
            all_valid.extend(custom_provider_names);
            return Err(EngineError::Config(format!(
                "Invalid default provider '{}'. Must be one of: {}",
                self.llm.default_provider,
                all_valid.join(", ")
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

        ensure_directory_writable(&self.core.data_dir)?;
        ensure_database_path_writable(&self.core.data_dir.join("rove.db"))?;
        self.validate_profiles_and_loadouts()?;
        normalize_and_validate_browser(&mut self.browser)?;
        normalize_and_validate_voice(&mut self.voice)?;

        Ok(())
    }

    fn apply_env_overrides(&mut self) {
        if let Some(data_dir) = std::env::var_os("ROVE_DATA_DIR").filter(|value| !value.is_empty())
        {
            self.core.data_dir = PathBuf::from(data_dir);
        }
    }

    /// Apply the built-in preset defaults for the selected daemon profile.
    pub fn apply_profile_preset(&mut self) {
        match self.daemon.profile {
            DaemonProfile::Desktop => {
                self.webui.enabled = true;
                self.ws_client.enabled = false;
                self.secrets.backend = SecretBackend::Auto;
                self.approvals.mode = ApprovalMode::Default;
            }
            DaemonProfile::Headless => {
                self.webui.enabled = true;
                self.ws_client.enabled = true;
                self.secrets.backend = SecretBackend::Vault;
                self.approvals.mode = ApprovalMode::Allowlist;
            }
            DaemonProfile::Edge => {
                self.webui.enabled = false;
                self.ws_client.enabled = true;
                self.secrets.backend = SecretBackend::Vault;
                self.approvals.mode = ApprovalMode::Allowlist;
                self.memory.mode = MemoryMode::GraphOnly;
                self.memory.retrieval_assist = MemoryRetrievalAssist::Off;
            }
        }
    }

    pub fn sdk_snapshot(&self) -> VersionedConfigSnapshot {
        VersionedConfigSnapshot {
            metadata: ConfigMetadataSnapshot {
                schema_version: self.config_schema_version,
                written_by_version: self.config_written_by.clone(),
            },
            daemon: DaemonConfigSnapshot {
                profile: self.daemon.profile.as_str().to_string(),
                developer_mode: self.daemon.developer_mode,
            },
            core: CoreConfigSnapshot {
                workspace: self.core.workspace.display().to_string(),
                data_dir: self.core.data_dir.display().to_string(),
                log_level: self.core.log_level.clone(),
            },
            approvals: ApprovalConfigSnapshot {
                mode: self.approvals.mode.as_str().to_string(),
            },
            llm: LlmConfigSnapshot {
                default_provider: self.llm.default_provider.clone(),
            },
            secrets: SecretConfigSnapshot {
                backend: self.secrets.backend.as_str().to_string(),
            },
            services: ServicesConfigSnapshot {
                webui_enabled: self.webui.enabled,
                remote_enabled: self.ws_client.enabled,
                connector_engine_enabled: !self.mcp.servers.is_empty(),
            },
            channels: ChannelsConfigSnapshot {
                telegram_enabled: self.telegram.enabled,
            },
        }
    }
}

fn config_to_toml(config: &Config) -> Result<String, toml::ser::Error> {
    let mut table = Map::new();
    table.insert(
        "config_schema_version".to_string(),
        Value::Integer(config.config_schema_version as i64),
    );
    table.insert(
        "config_written_by".to_string(),
        Value::String(config.config_written_by.clone()),
    );
    table.insert("daemon".to_string(), Value::try_from(&config.daemon)?);
    if let Some(active_profile) = config.active_profile.as_ref() {
        table.insert(
            "active_profile".to_string(),
            Value::String(active_profile.clone()),
        );
    }
    table.insert("core".to_string(), Value::try_from(&config.core)?);
    if !config.profiles.is_empty() {
        table.insert("profiles".to_string(), Value::try_from(&config.profiles)?);
    }
    if !config.loadouts.is_empty() {
        table.insert("loadouts".to_string(), Value::try_from(&config.loadouts)?);
    }
    table.insert("approvals".to_string(), Value::try_from(&config.approvals)?);
    table.insert("browser".to_string(), Value::try_from(&config.browser)?);
    table.insert("llm".to_string(), Value::try_from(&config.llm)?);
    table.insert("tools".to_string(), Value::try_from(&config.tools)?);
    table.insert("plugins".to_string(), Value::try_from(&config.plugins)?);
    table.insert("security".to_string(), Value::try_from(&config.security)?);
    table.insert("agent".to_string(), Value::try_from(&config.agent)?);
    table.insert("memory".to_string(), Value::try_from(&config.memory)?);
    table.insert(
        "policy".to_string(),
        canonical_policy_value(Value::try_from(&config.policy)?),
    );
    table.insert("secrets".to_string(), Value::try_from(&config.secrets)?);
    table.insert("remote".to_string(), Value::try_from(&config.remote)?);
    table.insert("gateway".to_string(), Value::try_from(&config.gateway)?);
    table.insert("mcp".to_string(), Value::try_from(&config.mcp)?);
    table.insert("voice".to_string(), Value::try_from(&config.voice)?);
    table.insert(
        "services".to_string(),
        Value::Table(public_services_table(config)),
    );
    table.insert(
        "channels".to_string(),
        Value::Table(public_channels_table(config)),
    );
    insert_public_brain_aliases(&mut table, config)?;

    toml::to_string_pretty(&Value::Table(table))
}

/// Migrate `[llm] custom_providers = ["name"]` (legacy string array) to the
/// current struct format `[[llm.custom_providers]] name = "..." ...`.
///
/// Old configs stored provider names as strings and kept their settings in a
/// separate `[llm.<name>]` table. We reconstruct a full `CustomProvider` map
/// from those sibling tables so the daemon can start without a hard error.
/// Missing fields (`protocol`, `secret_key`) are filled in with safe defaults.
fn migrate_custom_providers(value: &mut Value) {
    let table = match value.as_table_mut() {
        Some(t) => t,
        None => return,
    };

    let llm = match table.get_mut("llm").and_then(Value::as_table_mut) {
        Some(t) => t,
        None => return,
    };

    // Check whether custom_providers contains any plain strings.
    let has_string_entries = llm
        .get("custom_providers")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().any(|v| v.is_str()))
        .unwrap_or(false);

    if !has_string_entries {
        return;
    }

    // Collect the string names and their sibling table data.
    let names: Vec<String> = llm
        .get("custom_providers")
        .and_then(Value::as_array)
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();

    let mut migrated: Vec<Value> = Vec::new();

    for name in &names {
        // Look for a sibling `[llm.<name>]` table (TOML parses it as a key on llm).
        let sibling = llm.get(name).and_then(Value::as_table).cloned();

        let base_url = sibling
            .as_ref()
            .and_then(|t| t.get("base_url"))
            .and_then(Value::as_str)
            .unwrap_or("http://localhost:5580/v1")
            .to_string();

        let model = sibling
            .as_ref()
            .and_then(|t| t.get("model"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let protocol = sibling
            .as_ref()
            .and_then(|t| t.get("protocol"))
            .and_then(Value::as_str)
            .unwrap_or("openai")
            .to_string();

        let secret_key = sibling
            .as_ref()
            .and_then(|t| t.get("secret_key"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}_api_key", name.replace('-', "_")));

        let mut entry = toml::map::Map::new();
        entry.insert("name".to_string(), Value::String(name.clone()));
        entry.insert("protocol".to_string(), Value::String(protocol));
        entry.insert("base_url".to_string(), Value::String(base_url));
        entry.insert("model".to_string(), Value::String(model));
        entry.insert("secret_key".to_string(), Value::String(secret_key));
        migrated.push(Value::Table(entry));
    }

    // Also keep any existing struct entries that weren't strings.
    let existing_structs: Vec<Value> = llm
        .get("custom_providers")
        .and_then(Value::as_array)
        .unwrap_or(&vec![])
        .iter()
        .filter(|v| v.is_table())
        .cloned()
        .collect();

    migrated.extend(existing_structs);
    llm.insert("custom_providers".to_string(), Value::Array(migrated));
}

fn normalize_public_aliases(value: &mut Value) -> Result<(), EngineError> {
    let table = value
        .as_table_mut()
        .ok_or_else(|| EngineError::Config("Config root must be a TOML table".to_string()))?;

    if !table.contains_key("core") {
        if let Some(kernel) = table.get("kernel").cloned() {
            table.insert("core".to_string(), kernel);
        }
    }

    if let Some(services) = table.get("services").and_then(Value::as_table).cloned() {
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
        let has_legacy_shape = brains.contains_key("enabled")
            || brains.contains_key("fallback")
            || brains.contains_key("ram_limit_mb");
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
    logging.insert(
        "level".to_string(),
        Value::String(config.core.log_level.clone()),
    );
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
    brains_table.insert("dispatch".to_string(), Value::try_from(&config.brains)?);
    Ok(())
}

fn normalize_and_validate_browser(browser: &mut BrowserConfig) -> Result<(), EngineError> {
    browser.default_profile_id = normalize_optional_string(browser.default_profile_id.take());

    let mut seen_ids = std::collections::HashSet::new();
    for profile in &mut browser.profiles {
        profile.id = profile.id.trim().to_string();
        profile.name = profile.name.trim().to_string();
        profile.browser = normalize_optional_string(profile.browser.take());
        profile.user_data_dir = normalize_optional_string(profile.user_data_dir.take());
        profile.startup_url = normalize_optional_string(profile.startup_url.take());
        profile.cdp_url = normalize_optional_string(profile.cdp_url.take());
        profile.notes = normalize_optional_string(profile.notes.take());

        if profile.id.is_empty() {
            return Err(EngineError::Config(
                "browser profile id cannot be empty".to_string(),
            ));
        }
        if profile.name.is_empty() {
            return Err(EngineError::Config(format!(
                "browser profile '{}' must have a non-empty name",
                profile.id
            )));
        }
        if !seen_ids.insert(profile.id.clone()) {
            return Err(EngineError::Config(format!(
                "duplicate browser profile id '{}'",
                profile.id
            )));
        }
        if matches!(
            profile.mode,
            BrowserProfileMode::AttachExisting | BrowserProfileMode::RemoteCdp
        ) && profile.cdp_url.is_none()
        {
            return Err(EngineError::Config(format!(
                "browser profile '{}' uses {} mode but has no cdp_url",
                profile.id,
                profile.mode.as_str()
            )));
        }
        if let Some(url) = profile.cdp_url.as_deref() {
            let valid_scheme = ["ws://", "wss://", "http://", "https://"]
                .iter()
                .any(|prefix| url.starts_with(prefix));
            if !valid_scheme {
                return Err(EngineError::Config(format!(
                    "browser profile '{}' has invalid cdp_url '{}'",
                    profile.id, url
                )));
            }
        }
    }

    if let Some(default_profile_id) = browser.default_profile_id.as_deref() {
        if !browser
            .profiles
            .iter()
            .any(|profile| profile.id == default_profile_id)
        {
            return Err(EngineError::Config(format!(
                "browser.default_profile_id '{}' does not match any browser profile",
                default_profile_id
            )));
        }
    }

    Ok(())
}

fn normalize_and_validate_voice(voice: &mut VoiceConfig) -> Result<(), EngineError> {
    voice.selected_input_device_id =
        normalize_optional_string(voice.selected_input_device_id.take());
    voice.selected_output_device_id =
        normalize_optional_string(voice.selected_output_device_id.take());

    let mut seen_ids = std::collections::HashSet::new();
    for engine in &mut voice.engines {
        engine.model = normalize_optional_string(engine.model.take());
        engine.voice = normalize_optional_string(engine.voice.take());
        engine.runtime_path = normalize_optional_string(engine.runtime_path.take());
        engine.asset_dir = normalize_optional_string(engine.asset_dir.take());
        engine.notes = normalize_optional_string(engine.notes.take());

        if !seen_ids.insert(engine.kind.as_str().to_string()) {
            return Err(EngineError::Config(format!(
                "duplicate voice engine '{}'",
                engine.kind.as_str()
            )));
        }

        match engine.kind {
            crate::config::VoiceEngineKind::NativeOs => {}
            crate::config::VoiceEngineKind::LocalWhisper => {
                if engine.enabled && engine.model.is_none() {
                    return Err(EngineError::Config(
                        "voice engine 'local_whisper' requires model when enabled".to_string(),
                    ));
                }
            }
            crate::config::VoiceEngineKind::LocalPiper => {
                if engine.enabled && engine.voice.is_none() {
                    return Err(EngineError::Config(
                        "voice engine 'local_piper' requires voice when enabled".to_string(),
                    ));
                }
            }
        }
    }

    if let Some(active_input_engine) = voice.active_input_engine {
        let engine = voice
            .engines
            .iter()
            .find(|engine| engine.kind == active_input_engine)
            .ok_or_else(|| {
                EngineError::Config(format!(
                    "voice.active_input_engine '{}' does not match any voice engine",
                    active_input_engine.as_str()
                ))
            })?;
        if !engine.enabled {
            return Err(EngineError::Config(format!(
                "voice.active_input_engine '{}' is disabled",
                active_input_engine.as_str()
            )));
        }
        if !active_input_engine.supports_input() {
            return Err(EngineError::Config(format!(
                "voice.active_input_engine '{}' does not support input",
                active_input_engine.as_str()
            )));
        }
    }

    if let Some(active_output_engine) = voice.active_output_engine {
        let engine = voice
            .engines
            .iter()
            .find(|engine| engine.kind == active_output_engine)
            .ok_or_else(|| {
                EngineError::Config(format!(
                    "voice.active_output_engine '{}' does not match any voice engine",
                    active_output_engine.as_str()
                ))
            })?;
        if !engine.enabled {
            return Err(EngineError::Config(format!(
                "voice.active_output_engine '{}' is disabled",
                active_output_engine.as_str()
            )));
        }
        if !active_output_engine.supports_output() {
            return Err(EngineError::Config(format!(
                "voice.active_output_engine '{}' does not support output",
                active_output_engine.as_str()
            )));
        }
    }

    Ok(())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
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

fn ensure_directory_writable(path: &Path) -> Result<(), EngineError> {
    let probe = path.join(format!(".rove-write-test-{}", std::process::id()));
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe)
        .map_err(|error| {
            EngineError::Config(format!(
                "Configured data directory is not writable: {} ({})",
                path.display(),
                error
            ))
        })?;
    let _ = fs::remove_file(probe);
    Ok(())
}

fn ensure_database_path_writable(path: &Path) -> Result<(), EngineError> {
    if !path.exists() {
        return Ok(());
    }

    OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| {
            EngineError::Config(format!(
                "Configured database path is not writable: {} ({}). Update `core.data_dir` or set `ROVE_DATA_DIR` to a writable location.",
                path.display(),
                error
            ))
        })?;
    Ok(())
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
        assert_eq!(
            config.policy.default_policies,
            vec!["rust-safe".to_string()]
        );
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
        config.core.data_dir = base.join("data");
        std::fs::create_dir_all(&config.core.data_dir).expect("data dir");
        config.core.workspace = base.join("workspace");
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        config.webui.enabled = true;
        config.ws_client.enabled = true;
        config.telegram.enabled = true;
        config.policy.default_policies = vec!["rust-safe".to_string()];
        config.brains.enabled = true;

        config.save_to_path(&config_path).expect("save config");
        let raw = std::fs::read_to_string(&config_path).expect("read config");
        assert!(raw.contains("config_schema_version = 2"));
        assert!(raw.contains("config_written_by = "));
        assert!(raw.contains("[core]"));
        assert!(!raw.contains("[kernel]"));
        assert!(raw.contains("[policy]"));
        assert!(raw.contains("[services.logging]"));
        assert!(raw.contains("[services.webui]"));
        assert!(raw.contains("[services.remote]"));
        assert!(raw.contains("[channels.telegram]"));
        assert!(raw.contains("[brains.dispatch]"));
    }

    #[test]
    fn load_rejects_newer_config_schema() {
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
                r#"config_schema_version = 999
config_written_by = "99.0.0"

[core]
workspace = "{workspace}"
data_dir = "{data_dir}"

[llm]
default_provider = "ollama"
sensitivity_threshold = 0.7
complexity_threshold = 0.7

[security]
max_risk_tier = 2
"#,
                workspace = workspace.display(),
                data_dir = base.join("data").display(),
            ),
        )
        .expect("write config");

        let error = Config::load_from_path(&config_path).expect_err("newer schema should fail");
        assert!(error
            .to_string()
            .contains("Config schema version 999 is newer"));
    }

    #[test]
    fn sdk_snapshot_tracks_schema_and_profile() {
        let mut config = Config::default();
        config.core.workspace = std::env::current_dir().expect("cwd");
        config.core.data_dir = config
            .core
            .workspace
            .join("target/config-tests/sdk-snapshot");
        config.daemon.profile = crate::config::DaemonProfile::Headless;
        config.approvals.mode = crate::config::ApprovalMode::Allowlist;
        config.secrets.backend = crate::config::SecretBackend::Vault;
        config.ws_client.enabled = true;
        config.telegram.enabled = true;
        config.clamp_and_validate().expect("valid");

        let snapshot = config.sdk_snapshot();
        assert_eq!(snapshot.metadata.schema_version, 2);
        assert_eq!(snapshot.daemon.profile, "headless");
        assert_eq!(snapshot.approvals.mode, "allowlist");
        assert_eq!(snapshot.secrets.backend, "vault");
        assert!(snapshot.services.remote_enabled);
        assert!(snapshot.channels.telegram_enabled);
    }

    #[test]
    fn edge_profile_preset_disables_webui_and_keeps_graph_only_memory() {
        let mut config = Config::default();
        config.webui.enabled = true;
        config.ws_client.enabled = false;
        config.memory.mode = crate::config::MemoryMode::AlwaysOn;
        config.memory.retrieval_assist = crate::config::MemoryRetrievalAssist::Rerank;
        config.daemon.profile = crate::config::DaemonProfile::Edge;

        config.apply_profile_preset();

        assert!(!config.webui.enabled);
        assert!(config.ws_client.enabled);
        assert_eq!(config.secrets.backend, crate::config::SecretBackend::Vault);
        assert_eq!(
            config.approvals.mode,
            crate::config::ApprovalMode::Allowlist
        );
        assert_eq!(config.memory.mode, crate::config::MemoryMode::GraphOnly);
        assert_eq!(
            config.memory.retrieval_assist,
            crate::config::MemoryRetrievalAssist::Off
        );
    }

    #[test]
    fn browser_profiles_require_unique_ids_and_valid_default() {
        let mut config = Config::default();
        config.core.workspace = std::env::current_dir().expect("cwd");
        config.core.data_dir = config
            .core
            .workspace
            .join("target/config-tests/browser-duplicate");
        config.browser.enabled = true;
        config.browser.default_profile_id = Some("ops".to_string());
        config.browser.profiles = vec![
            crate::config::BrowserProfileConfig {
                id: "ops".to_string(),
                name: "Ops".to_string(),
                ..Default::default()
            },
            crate::config::BrowserProfileConfig {
                id: "ops".to_string(),
                name: "Duplicate".to_string(),
                ..Default::default()
            },
        ];

        let error = config.clamp_and_validate().expect_err("duplicate id");
        assert!(error.to_string().contains("duplicate browser profile id"));
    }

    #[test]
    fn browser_remote_cdp_requires_url() {
        let mut config = Config::default();
        config.core.workspace = std::env::current_dir().expect("cwd");
        config.core.data_dir = config
            .core
            .workspace
            .join("target/config-tests/browser-remote-cdp");
        config.browser.enabled = true;
        config.browser.profiles = vec![crate::config::BrowserProfileConfig {
            id: "remote".to_string(),
            name: "Remote".to_string(),
            mode: crate::config::BrowserProfileMode::RemoteCdp,
            ..Default::default()
        }];

        let error = config
            .clamp_and_validate()
            .expect_err("remote cdp should require cdp url");
        assert!(error.to_string().contains("has no cdp_url"));
    }

    #[test]
    fn enabled_local_whisper_requires_model() {
        let mut config = Config::default();
        config.core.workspace = std::env::current_dir().expect("cwd");
        config.core.data_dir = config
            .core
            .workspace
            .join("target/config-tests/voice-local-whisper-model");
        config.voice.enabled = true;
        config.voice.engines = vec![crate::config::VoiceEngineConfig {
            kind: crate::config::VoiceEngineKind::LocalWhisper,
            ..Default::default()
        }];

        let error = config
            .clamp_and_validate()
            .expect_err("enabled local whisper should require model");
        assert!(error.to_string().contains("requires model"));
    }

    #[test]
    fn active_voice_engine_must_match_capability() {
        let mut config = Config::default();
        config.core.workspace = std::env::current_dir().expect("cwd");
        config.core.data_dir = config
            .core
            .workspace
            .join("target/config-tests/voice-active-capability");
        config.voice.enabled = true;
        config.voice.active_input_engine = Some(crate::config::VoiceEngineKind::LocalPiper);
        config.voice.engines = vec![crate::config::VoiceEngineConfig {
            kind: crate::config::VoiceEngineKind::LocalPiper,
            voice: Some("en_US-lessac-medium".to_string()),
            ..Default::default()
        }];

        let error = config
            .clamp_and_validate()
            .expect_err("active input engine should need input support");
        assert!(error.to_string().contains("does not support input"));
    }

    #[test]
    fn loadout_resolution_uses_active_profile_and_normalizes_entries() {
        let mut config = Config::default();
        config.core.workspace = std::env::current_dir().expect("cwd");
        config.core.data_dir = config
            .core
            .workspace
            .join("target/config-tests/loadout-resolution");
        config.active_profile = Some("isolated".to_string());
        config.profiles.insert(
            "isolated".to_string(),
            crate::config::ProfileConfig {
                loadout: "offline".to_string(),
                browser_profile: Some("none".to_string()),
                ..Default::default()
            },
        );
        config.loadouts.insert(
            "offline".to_string(),
            crate::config::LoadoutConfig {
                builtins: vec![
                    " terminal ".to_string(),
                    "filesystem".to_string(),
                    "terminal".to_string(),
                ],
                drivers: vec![" vision ".to_string()],
                plugins: vec!["fs-editor".to_string(), "fs-editor".to_string()],
            },
        );

        config.clamp_and_validate().expect("valid");
        let resolved = config.resolved_loadout().expect("resolved loadout");

        assert_eq!(resolved.profile_name, "isolated");
        assert_eq!(resolved.loadout_name, "offline");
        assert!(resolved.builtins.contains("filesystem"));
        assert!(resolved.builtins.contains("terminal"));
        assert!(!resolved.builtins.contains("vision"));
        assert_eq!(resolved.browser_profile.as_deref(), Some("none"));
        assert_eq!(
            resolved.plugins.as_ref().expect("plugin set").len(),
            1,
            "plugin entries should be deduplicated"
        );
    }

    #[test]
    fn profiles_require_an_active_or_default_selection() {
        let mut config = Config::default();
        config.core.workspace = std::env::current_dir().expect("cwd");
        config.core.data_dir = config
            .core
            .workspace
            .join("target/config-tests/profile-selection");
        config.profiles.insert(
            "ops".to_string(),
            crate::config::ProfileConfig {
                loadout: "ops".to_string(),
                ..Default::default()
            },
        );
        config.profiles.insert(
            "isolated".to_string(),
            crate::config::ProfileConfig {
                loadout: "isolated".to_string(),
                ..Default::default()
            },
        );
        config
            .loadouts
            .insert("ops".to_string(), crate::config::LoadoutConfig::default());
        config.loadouts.insert(
            "isolated".to_string(),
            crate::config::LoadoutConfig::default(),
        );

        let error = config
            .clamp_and_validate()
            .expect_err("profile selection should be required");
        assert!(error
            .to_string()
            .contains("No active extension profile could be resolved"));
    }
}
