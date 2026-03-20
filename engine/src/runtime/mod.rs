//! Runtime module for loading and managing execution surfaces.

pub mod builtin;
pub mod manifest;
pub mod mcp;
pub mod native;
pub mod registry;
pub mod wasm;

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
        let legacy_plugins = if installed_plugins.is_empty() {
            database
                .plugins()
                .get_enabled_plugins()
                .await
                .map_err(|error| EngineError::Database(error.to_string()))?
        } else {
            Vec::new()
        };
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

        let wasm = if wasm_manifest.plugins.is_empty() && legacy_plugins.is_empty() {
            None
        } else {
            match CryptoModule::new() {
                Ok(crypto) => {
                    let fs_guard = Arc::new(FileSystemGuard::new(config.core.workspace.clone())?);
                    let manifest = if wasm_manifest.plugins.is_empty() {
                        legacy_wasm_manifest(&legacy_plugins)
                    } else {
                        wasm_manifest
                    };
                    Some(Arc::new(Mutex::new(WasmRuntime::new(
                        manifest,
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

        let effective_mcp_configs = if mcp_configs.is_empty() {
            config.mcp.servers.clone()
        } else {
            mcp_configs
        };
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

        builtin::register_all(&mut registry, config.core.workspace.clone()).await?;

        if installed_plugins.is_empty() {
            for plugin in &legacy_plugins {
                registry
                    .register_tools_from_plugin_manifest(&plugin.name, &plugin.manifest_json)
                    .await;
            }
        } else {
            register_installed_plugin_schemas(&mut registry, native.as_ref(), &installed_plugins)
                .await;
        }

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

        let config = serde_json::from_str::<McpServerConfig>(raw).map_err(|error| {
            EngineError::Config(format!(
                "Invalid MCP runtime config for '{}': {}",
                plugin.name, error
            ))
        })?;
        configs.push(config);
    }

    Ok(configs)
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

fn legacy_wasm_manifest(enabled_plugins: &[crate::storage::Plugin]) -> SdkManifest {
    SdkManifest {
        version: "1.0.0".to_string(),
        team_public_key: String::new(),
        signature: String::new(),
        generated_at: String::new(),
        core_tools: Vec::new(),
        plugins: enabled_plugins
            .iter()
            .map(|plugin| PluginEntry {
                name: plugin.name.clone(),
                version: plugin.version.clone(),
                path: plugin.wasm_path.clone(),
                hash: plugin.wasm_hash.clone(),
                permissions: PluginPermissions::default(),
                allowed_imports: vec![
                    "extism:host/env".to_string(),
                    "wasi_snapshot_preview1".to_string(),
                ],
                trust_tier: 0,
            })
            .collect(),
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
    use crate::storage::{Database, InstalledPlugin};

    use super::RuntimeManager;

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
}
