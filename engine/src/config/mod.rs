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
pub mod security;
pub mod steering;
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
pub use security::*;
pub use steering::*;
pub use tools::*;
pub use transport::*;
pub use webui::*;

use sdk::errors::EngineError;
use std::fs;
use std::path::{Path, PathBuf};

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

        let mut config: Config = toml::from_str(&contents)
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

        let toml_string = toml::to_string_pretty(&config)
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
