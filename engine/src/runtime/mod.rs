//! Runtime module for loading and managing execution surfaces.

pub mod builtin;
pub mod manifest;
pub mod mcp;
pub mod native;
pub mod registry;
pub mod wasm;

use std::collections::BTreeMap;
use std::sync::Arc;

use sdk::errors::EngineError;
use sdk::manifest::Manifest as SdkManifest;
use sdk::manifest::{PluginEntry, PluginPermissions};
use tokio::sync::Mutex;
use tracing::warn;

use crate::config::Config;
use crate::security::crypto::CryptoModule;
use crate::security::fs_guard::FileSystemGuard;
use crate::storage::Database;
use crate::storage::InstalledPlugin;

pub use builtin::{FilesystemTool, TerminalTool, VisionTool};
pub use manifest::*;
pub use mcp::{
    McpSandbox, McpServer, McpServerConfig, McpSpawner, McpToolDescriptor, SandboxProfile,
};
pub use native::NativeRuntime;
pub use registry::ToolRegistry;
pub use wasm::WasmRuntime;

pub struct RuntimeManager {
    pub registry: Arc<ToolRegistry>,
    #[allow(dead_code)]
    native: Option<Arc<Mutex<NativeRuntime>>>,
    #[allow(dead_code)]
    wasm: Option<Arc<Mutex<WasmRuntime>>>,
    #[allow(dead_code)]
    mcp: Option<Arc<McpSpawner>>,
}

impl RuntimeManager {
    pub async fn build(database: &Database, config: &Config) -> Result<Self, EngineError> {
        let installed_plugins = database
            .installed_plugins()
            .get_enabled_plugins()
            .await
            .map_err(|error| EngineError::Database(error.to_string()))?;
        let wasm_manifest = sdk_manifest_from_installed_plugins(&installed_plugins);
        let mcp_configs = load_installed_mcp_configs(&installed_plugins).unwrap_or_else(|error| {
            warn!("Failed to load installed MCP configs: {}", error);
            Vec::new()
        });

        let native = match CryptoModule::new() {
            Ok(crypto) => Some(Arc::new(Mutex::new(NativeRuntime::new(
                empty_sdk_manifest(),
                Arc::new(crypto),
            )))),
            Err(error) => {
                warn!("Failed to initialize native runtime crypto: {}", error);
                None
            }
        };

        let wasm = if wasm_manifest.plugins.is_empty() {
            None
        } else {
            match CryptoModule::new() {
                Ok(crypto) => {
                    let fs_guard = Arc::new(FileSystemGuard::new(config.core.workspace.clone())?);
                    Some(Arc::new(Mutex::new(WasmRuntime::new(
                        wasm_manifest,
                        Arc::new(crypto),
                        fs_guard,
                    ))))
                }
                Err(error) => {
                    warn!("Failed to initialize WASM runtime crypto: {}", error);
                    None
                }
            }
        };

        let effective_mcp_configs = merge_mcp_configs(&config.mcp.servers, mcp_configs);
        let mcp = if effective_mcp_configs.is_empty() {
            None
        } else {
            let spawner = Arc::new(McpSpawner::new(effective_mcp_configs.clone()));
            let keepalive = Arc::clone(&spawner);
            tokio::spawn(async move {
                keepalive.keepalive_loop().await;
            });
            Some(spawner)
        };

        let mut registry = ToolRegistry::new(
            Arc::new(config.clone()),
            native.clone(),
            wasm.clone(),
            mcp.clone(),
        );

        builtin::register_selected(
            &mut registry,
            config.core.workspace.clone(),
            builtin::BuiltinSelection::from_config(config),
        )
        .await?;

        register_installed_plugin_schemas(&mut registry, native.as_ref(), &installed_plugins).await;

        if let Some(spawner) = &mcp {
            registry.register_mcp_spawner(Arc::clone(spawner));
            for server in &effective_mcp_configs {
                if !server.enabled {
                    continue;
                }
                for tool in &server.cached_tools {
                    registry
                        .register_mcp_tool(
                            &server.name,
                            &tool.name,
                            &tool.description,
                            tool.input_schema.clone(),
                        )
                        .await;
                }
            }
        }

        Ok(Self {
            registry: Arc::new(registry),
            native,
            wasm,
            mcp,
        })
    }
}

async fn register_installed_plugin_schemas(
    registry: &mut ToolRegistry,
    native_runtime: Option<&Arc<Mutex<NativeRuntime>>>,
    installed_plugins: &[InstalledPlugin],
) {
    for plugin in installed_plugins {
        let manifest = match Manifest::from_json(&plugin.manifest) {
            Ok(manifest) => manifest,
            Err(error) => {
                warn!("Skipping installed plugin '{}': {}", plugin.name, error);
                continue;
            }
        };

        if let Err(error) = manifest.validate_install_record(&plugin.plugin_type, plugin.trust_tier)
        {
            warn!("Skipping installed plugin '{}': {}", plugin.name, error);
            continue;
        }

        match manifest.plugin_type {
            PluginType::Skill | PluginType::Channel => {
                let catalog = match ToolCatalog::from_json(plugin.config.as_deref()) {
                    Ok(catalog) => catalog,
                    Err(error) => {
                        warn!("Skipping installed plugin '{}': {}", plugin.name, error);
                        continue;
                    }
                };

                for tool in catalog.tools {
                    let domains = if tool.domains.is_empty() {
                        crate::tools::catalog::derive_domains_from_name(&tool.name)
                    } else {
                        tool.domains.clone()
                    };
                    registry
                        .register_wasm_tool(
                            &plugin.name,
                            tool.name,
                            tool.description,
                            tool.parameters,
                            domains,
                        )
                        .await;
                }
            }
            PluginType::Brain | PluginType::Workspace => {
                let catalog = match ToolCatalog::from_json(plugin.config.as_deref()) {
                    Ok(catalog) => catalog,
                    Err(error) => {
                        warn!("Skipping installed plugin '{}': {}", plugin.name, error);
                        continue;
                    }
                };
                let Some(binary_path) = plugin.binary_path.clone() else {
                    warn!(
                        "Skipping native schema registration for '{}' because binary_path is missing",
                        plugin.name
                    );
                    continue;
                };

                if let Some(native_runtime) = native_runtime {
                    let mut runtime = native_runtime.lock().await;
                    runtime.register_library(
                        binary_path.clone(),
                        plugin.binary_hash.clone(),
                        plugin.signature.clone(),
                    );
                }

                for tool in catalog.tools {
                    let domains = if tool.domains.is_empty() {
                        crate::tools::catalog::derive_domains_from_name(&tool.name)
                    } else {
                        tool.domains.clone()
                    };
                    registry
                        .register_native_tool(
                            tool.name,
                            tool.description,
                            tool.parameters,
                            binary_path.clone(),
                            domains,
                        )
                        .await;
                }
            }
            PluginType::Mcp => {}
        }
    }
}

fn load_installed_mcp_configs(
    installed_plugins: &[InstalledPlugin],
) -> Result<Vec<McpServerConfig>, EngineError> {
    let mut configs = Vec::new();

    for plugin in installed_plugins {
        if plugin.plugin_type != PluginType::Mcp.as_str() {
            continue;
        }

        let raw = match plugin.config.as_deref() {
            Some(raw) if !raw.trim().is_empty() => raw,
            _ => {
                warn!(
                    "Skipping installed MCP plugin '{}' because runtime config is missing",
                    plugin.name
                );
                continue;
            }
        };

        let mut config = serde_json::from_str::<McpServerConfig>(raw).map_err(|error| {
            EngineError::Config(format!(
                "Invalid MCP runtime config for '{}': {}",
                plugin.name, error
            ))
        })?;
        config.enabled = plugin.enabled;
        configs.push(config);
    }

    Ok(configs)
}

fn merge_mcp_configs(
    configured: &[McpServerConfig],
    installed: Vec<McpServerConfig>,
) -> Vec<McpServerConfig> {
    let mut merged = BTreeMap::new();

    for server in configured.iter().cloned() {
        merged.insert(server.name.clone(), server);
    }

    for server in installed {
        if merged.insert(server.name.clone(), server.clone()).is_some() {
            warn!(
                server = %server.name,
                "Installed MCP plugin overrides config-backed MCP server"
            );
        }
    }

    merged.into_values().collect()
}

fn sdk_manifest_from_installed_plugins(installed_plugins: &[InstalledPlugin]) -> SdkManifest {
    let plugins = installed_plugins
        .iter()
        .filter_map(|plugin| {
            let manifest = Manifest::from_json(&plugin.manifest).ok()?;
            if !matches!(
                manifest.plugin_type,
                PluginType::Skill | PluginType::Channel
            ) {
                return None;
            }

            let path = plugin.binary_path.clone()?;
            let mut permissions = PluginPermissions::default();
            let allowed_paths: Vec<String> = manifest
                .permissions
                .filesystem
                .iter()
                .map(|pattern| pattern.0.clone())
                .collect();
            if !allowed_paths.is_empty() {
                permissions.allowed_paths = allowed_paths;
            }

            Some(PluginEntry {
                name: plugin.name.clone(),
                version: plugin.version.clone(),
                path,
                hash: plugin.binary_hash.clone(),
                permissions,
                allowed_imports: vec![
                    "extism:host/env".to_string(),
                    "wasi_snapshot_preview1".to_string(),
                ],
                trust_tier: plugin.trust_tier as u8,
            })
        })
        .collect();

    SdkManifest {
        version: SDK_VERSION.to_string(),
        team_public_key: String::new(),
        signature: String::new(),
        generated_at: String::new(),
        core_tools: Vec::new(),
        plugins,
    }
}

fn empty_sdk_manifest() -> SdkManifest {
    SdkManifest {
        version: "1.0.0".to_string(),
        team_public_key: String::new(),
        signature: String::new(),
        generated_at: String::new(),
        core_tools: Vec::new(),
        plugins: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::config::Config;
    use crate::runtime::mcp::SandboxProfile;
    use crate::storage::{Database, InstalledPlugin};

    use super::{merge_mcp_configs, RuntimeManager};

    #[tokio::test]
    async fn runtime_build_registers_installed_plugin_schemas() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime.db"))
            .await
            .expect("database");

        let plugin = InstalledPlugin {
            id: "echo-skill".to_string(),
            name: "echo-skill".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Skill".to_string(),
            trust_tier: 1,
            manifest: r#"{
                "name": "echo-skill",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Echo skill"
            }"#
            .to_string(),
            binary_path: Some("echo-skill.wasm".to_string()),
            binary_hash: "abc123".to_string(),
            signature: "deadbeef".to_string(),
            enabled: true,
            installed_at: 1_710_000_000,
            last_used: None,
            config: Some(
                r#"{
                    "tools": [
                        {
                            "name": "echo_text",
                            "description": "Echo text",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "text": {"type": "string"}
                                },
                                "required": ["text"]
                            },
                            "domains": ["all", "general"]
                        }
                    ]
                }"#
                .to_string(),
            ),
        };

        database
            .installed_plugins()
            .upsert_plugin(&plugin)
            .await
            .expect("insert plugin");

        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.mcp.servers.clear();

        let runtime = RuntimeManager::build(&database, &config)
            .await
            .expect("runtime manager");
        let schemas = runtime.registry.schemas_for("general").await;

        assert!(schemas.iter().any(|schema| schema.name == "echo_text"));
    }

    #[tokio::test]
    async fn runtime_build_ignores_legacy_plugin_rows() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime-legacy.db"))
            .await
            .expect("database");

        database
            .plugins()
            .register_plugin(
                "legacy-echo",
                "legacy-echo",
                "0.1.0",
                "legacy-echo.wasm",
                "abc123",
                r#"{
                    "tools": [
                        {
                            "name": "legacy_echo",
                            "description": "Legacy echo tool",
                            "parameters": {
                                "type": "object",
                                "properties": {}
                            }
                        }
                    ]
                }"#,
            )
            .await
            .expect("legacy plugin row");

        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.mcp.servers.clear();

        let runtime = RuntimeManager::build(&database, &config)
            .await
            .expect("runtime manager");
        let schemas = runtime.registry.schemas_for("all").await;

        assert!(!schemas.iter().any(|schema| schema.name == "legacy_echo"));
    }

    #[test]
    fn merge_mcp_configs_prefers_installed_servers_on_name_collision() {
        let configured = vec![
            super::McpServerConfig {
                name: "github".to_string(),
                template: Some("github".to_string()),
                description: Some("config".to_string()),
                command: "config-command".to_string(),
                args: vec!["one".to_string()],
                profile: SandboxProfile::default(),
                cached_tools: Vec::new(),
                enabled: true,
            },
            super::McpServerConfig {
                name: "slack".to_string(),
                template: Some("slack".to_string()),
                description: Some("config".to_string()),
                command: "slack-command".to_string(),
                args: Vec::new(),
                profile: SandboxProfile::default(),
                cached_tools: Vec::new(),
                enabled: true,
            },
        ];
        let installed = vec![super::McpServerConfig {
            name: "github".to_string(),
            template: Some("github".to_string()),
            description: Some("installed".to_string()),
            command: "installed-command".to_string(),
            args: vec!["two".to_string()],
            profile: SandboxProfile::default(),
            cached_tools: Vec::new(),
            enabled: true,
        }];

        let merged = merge_mcp_configs(&configured, installed);

        assert_eq!(merged.len(), 2);
        let github = merged
            .iter()
            .find(|server| server.name == "github")
            .expect("github server");
        assert_eq!(github.command, "installed-command");
        assert_eq!(github.args, vec!["two"]);
    }
}
