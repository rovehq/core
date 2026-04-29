//! Runtime module for loading and managing execution surfaces.

pub mod builtin;
pub mod manifest;
pub mod mcp;
pub mod native;
pub mod registry;
pub mod wasm;

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use tokio::sync::Mutex as TokioMutex;

use async_trait::async_trait;
use sdk::brain::{Brain, BrainResponse, Message as BrainMessage, ToolSchema as BrainToolSchema};
use sdk::errors::EngineError;
use sdk::manifest::Manifest as SdkManifest;
use sdk::manifest::{PluginEntry, PluginPermissions};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::warn;

use crate::cli::database_path::expand_data_dir;
use crate::config::{Config, ResolvedLoadout};
use crate::security::crypto::CryptoModule;
use crate::security::fs_guard::FileSystemGuard;
use crate::storage::Database;
use crate::storage::InstalledPlugin;

pub use builtin::{BrowserTool, FilesystemTool, TerminalTool};
pub use manifest::*;
pub use mcp::{
    McpSandbox, McpServer, McpServerConfig, McpSpawner, McpToolDescriptor, SandboxProfile,
};
pub use native::NativeRuntime;
pub use registry::ToolRegistry;
pub use wasm::WasmRuntime;

struct NativeBrainDriverBackend {
    backend_id: String,
    lib_path: String,
    completion_fn: String,
    runtime: Arc<Mutex<NativeRuntime>>,
}

#[async_trait]
impl Brain for NativeBrainDriverBackend {
    fn name(&self) -> &str {
        &self.backend_id
    }

    async fn complete(
        &self,
        system: &str,
        messages: &[BrainMessage],
        tools: &[BrainToolSchema],
    ) -> Result<BrainResponse, EngineError> {
        let args = json!({
            "system": system,
            "messages": serde_json::to_value(messages).unwrap_or_default(),
            "tools": serde_json::to_value(tools).unwrap_or_default(),
        });
        let mut runtime = self.runtime.lock().await;
        let raw = runtime.call_registered_tool(&self.lib_path, &self.completion_fn, args)?;
        serde_json::from_value::<BrainResponse>(raw).map_err(|e| {
            EngineError::ToolError(format!(
                "Brain plugin '{}' response parse error: {}",
                self.backend_id, e
            ))
        })
    }
}

async fn find_plugin_brain(
    config: &Config,
    native_runtime: Option<&Arc<Mutex<NativeRuntime>>>,
    installed_plugins: &[InstalledPlugin],
) -> Option<Arc<dyn Brain>> {
    let native_runtime = native_runtime?;
    let requested_id = config.brains.plugin_backend.as_deref();
    for plugin in installed_plugins {
        if plugin.plugin_type != PluginType::Brain.as_str() {
            continue;
        }
        let Some(lib_path) = plugin.binary_path.as_deref() else {
            continue;
        };
        let catalog = ToolCatalog::from_json(plugin.config.as_deref()).ok()?;
        let Some(brain_backend) = catalog.brain_backend else {
            continue;
        };
        if let Some(requested) = requested_id {
            if !brain_backend.id.eq_ignore_ascii_case(requested) {
                continue;
            }
        }
        return Some(Arc::new(NativeBrainDriverBackend {
            backend_id: brain_backend.id,
            lib_path: lib_path.to_string(),
            completion_fn: brain_backend.completion_fn,
            runtime: Arc::clone(native_runtime),
        }));
    }
    None
}

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
    pub fn plugin_brain(&self) -> Option<Arc<dyn Brain>> {
        self.registry.plugin_brain()
    }

    pub async fn build(database: &Database, config: &Config) -> Result<Self, EngineError> {
        let resolved_loadout = config.resolved_loadout()?;
        let installed_plugins = database
            .installed_plugins()
            .list_plugins()
            .await
            .map_err(|error| EngineError::Database(error.to_string()))?;
        let installed_plugins = merge_discovered_driver_installs(config, installed_plugins);
        let installed_plugins = installed_plugins
            .into_iter()
            .filter(|plugin| plugin.enabled)
            .collect::<Vec<_>>();
        let installed_plugins =
            filter_installed_plugins_for_loadout(installed_plugins, &resolved_loadout);
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
                    Some(Arc::new(Mutex::new(WasmRuntime::new_with_config(
                        wasm_manifest,
                        Arc::new(crypto),
                        fs_guard,
                        config.clone(),
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
            resolved_loadout.builtin_selection(),
        )
        .await?;

        register_installed_plugin_schemas(&mut registry, native.as_ref(), &installed_plugins).await;

        if config.brains.enabled || config.brains.plugin_backend.is_some() {
            if let Some(plugin_brain) =
                find_plugin_brain(config, native.as_ref(), &installed_plugins).await
            {
                tracing::info!(backend = %plugin_brain.name(), "Registered plugin brain backend");
                registry.register_plugin_brain_backend(plugin_brain);
            }
        }

        // Wire browser tool when browser control is enabled and a profile is configured
        if config.browser.enabled {
            if let Some(profile) = config
                .browser
                .profiles
                .iter()
                .find(|profile| {
                    resolved_loadout.browser_profile.as_deref() == Some(profile.id.as_str())
                        && profile.enabled
                })
                .cloned()
                .or_else(|| {
                    config
                        .browser
                        .default_profile_id
                        .as_deref()
                        .and_then(|id| {
                            config
                                .browser
                                .profiles
                                .iter()
                                .find(|p| p.id == id && p.enabled)
                                .cloned()
                        })
                        .or_else(|| {
                            config
                                .browser
                                .profiles
                                .first()
                                .filter(|p| p.enabled)
                                .cloned()
                        })
                })
            {
                registry
                    .register_browser_backend(Arc::new(TokioMutex::new(BrowserTool::new(
                        profile,
                        config.core.data_dir.join("browser"),
                    ))))
                    .await;
            }
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

#[derive(Debug, Deserialize)]
struct InstalledBundlePackage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    artifact: Option<String>,
    #[serde(default)]
    runtime_config: Option<String>,
    #[serde(alias = "artifact_hash")]
    payload_hash: String,
    #[serde(alias = "artifact_signature")]
    payload_signature: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn filter_installed_plugins_for_loadout(
    installed_plugins: Vec<InstalledPlugin>,
    resolved_loadout: &ResolvedLoadout,
) -> Vec<InstalledPlugin> {
    installed_plugins
        .into_iter()
        .filter(|plugin| match PluginType::parse(&plugin.plugin_type) {
            Ok(PluginType::Plugin | PluginType::Channel) => {
                resolved_loadout.allows_plugin(plugin.name.as_str(), plugin.id.as_str())
            }
            Ok(PluginType::Brain | PluginType::Mcp) => true,
            Err(_) => false,
        })
        .collect()
}

fn merge_discovered_driver_installs(
    config: &Config,
    mut installed_plugins: Vec<InstalledPlugin>,
) -> Vec<InstalledPlugin> {
    let mut known_ids = installed_plugins
        .iter()
        .map(|plugin| plugin.id.clone())
        .collect::<HashSet<_>>();
    let mut known_names = installed_plugins
        .iter()
        .map(|plugin| plugin.name.clone())
        .collect::<HashSet<_>>();

    match discover_driver_installs(config, &mut known_ids, &mut known_names) {
        Ok(discovered) => installed_plugins.extend(discovered),
        Err(error) => warn!(
            "Failed to discover installed drivers from filesystem: {}",
            error
        ),
    }

    installed_plugins
}

fn discover_driver_installs(
    config: &Config,
    known_ids: &mut HashSet<String>,
    known_names: &mut HashSet<String>,
) -> Result<Vec<InstalledPlugin>, EngineError> {
    let drivers_root = expand_data_dir(&config.core.data_dir).join("drivers");
    if !drivers_root.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(&drivers_root).map_err(|error| {
        EngineError::Config(format!(
            "Failed to read driver install directory '{}': {}",
            drivers_root.display(),
            error
        ))
    })?;

    let mut discovered = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!(
                    "Skipping unreadable driver install entry in '{}': {}",
                    drivers_root.display(),
                    error
                );
                continue;
            }
        };
        let install_dir = entry.path();
        if !install_dir.is_dir() {
            continue;
        }

        match load_driver_install(&install_dir) {
            Ok(Some(plugin)) => {
                if known_ids.contains(&plugin.id) || known_names.contains(&plugin.name) {
                    continue;
                }
                known_ids.insert(plugin.id.clone());
                known_names.insert(plugin.name.clone());
                discovered.push(plugin);
            }
            Ok(None) => {}
            Err(error) => warn!(
                "Skipping installed driver bundle '{}': {}",
                install_dir.display(),
                error
            ),
        }
    }

    Ok(discovered)
}

fn load_driver_install(install_dir: &Path) -> Result<Option<InstalledPlugin>, EngineError> {
    let manifest_path = install_dir.join("manifest.json");
    let package_path = install_dir.join("plugin-package.json");

    if !manifest_path.exists() || !package_path.exists() {
        return Ok(None);
    }

    let manifest_raw = fs::read_to_string(&manifest_path).map_err(|error| {
        EngineError::Config(format!(
            "Failed to read installed manifest '{}': {}",
            manifest_path.display(),
            error
        ))
    })?;
    let manifest = Manifest::from_json(&manifest_raw)?;
    if manifest.plugin_type != PluginType::Plugin {
        return Ok(None);
    }

    let package_raw = fs::read_to_string(&package_path).map_err(|error| {
        EngineError::Config(format!(
            "Failed to read installed package metadata '{}': {}",
            package_path.display(),
            error
        ))
    })?;
    let package: InstalledBundlePackage = serde_json::from_str(&package_raw).map_err(|error| {
        EngineError::Config(format!(
            "Invalid installed package metadata '{}': {}",
            package_path.display(),
            error
        ))
    })?;

    let install_id = package
        .id
        .clone()
        .unwrap_or_else(|| default_plugin_id(&manifest.name));
    let binary_path = resolve_installed_driver_binary_path(install_dir, &package)?;
    let runtime_raw = load_optional_runtime_config(install_dir, package.runtime_config.as_deref())?;
    let installed_at = install_dir
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    Ok(Some(InstalledPlugin {
        id: install_id,
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        plugin_type: manifest.plugin_type.as_str().to_string(),
        trust_tier: manifest.trust_tier.as_i64(),
        manifest: manifest_raw,
        binary_path: Some(binary_path.to_string_lossy().to_string()),
        binary_hash: package.payload_hash,
        signature: package.payload_signature,
        enabled: package.enabled,
        installed_at,
        last_used: None,
        config: runtime_raw,
        provenance_source: Some("filesystem_scan".to_string()),
        provenance_registry: None,
        catalog_trust_badge: None,
    }))
}

fn load_optional_runtime_config(
    install_dir: &Path,
    relative: Option<&str>,
) -> Result<Option<String>, EngineError> {
    let runtime_path = match relative {
        Some(relative) => install_dir.join(relative),
        None => {
            let default_path = install_dir.join("runtime.json");
            if !default_path.exists() {
                return Ok(None);
            }
            default_path
        }
    };

    if !runtime_path.exists() {
        return Ok(None);
    }

    fs::read_to_string(&runtime_path)
        .map(Some)
        .map_err(|error| {
            EngineError::Config(format!(
                "Failed to read installed runtime config '{}': {}",
                runtime_path.display(),
                error
            ))
        })
}

fn resolve_installed_driver_binary_path(
    install_dir: &Path,
    package: &InstalledBundlePackage,
) -> Result<PathBuf, EngineError> {
    if let Some(artifact) = package.artifact.as_deref() {
        let binary_path = install_dir.join(artifact);
        if binary_path.exists() {
            return Ok(binary_path);
        }
        return Err(EngineError::Config(format!(
            "Installed driver artifact '{}' is missing",
            binary_path.display()
        )));
    }

    infer_native_artifact_path(install_dir).ok_or_else(|| {
        EngineError::Config(format!(
            "Installed driver bundle '{}' is missing a native artifact",
            install_dir.display()
        ))
    })
}

fn infer_native_artifact_path(install_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(install_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let extension = path.extension().and_then(|ext| ext.to_str());
        if matches!(extension, Some("dylib" | "so" | "dll")) {
            return Some(path);
        }
    }
    None
}

fn default_enabled() -> bool {
    true
}

fn default_plugin_id(name: &str) -> String {
    let mut id = String::new();
    let mut last_dash = false;

    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            id.push(lower);
            last_dash = false;
        } else if !last_dash {
            id.push('-');
            last_dash = true;
        }
    }

    let normalized = id.trim_matches('-').to_string();
    if normalized.is_empty() {
        let fallback = name
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if fallback.is_empty() {
            "plugin".to_string()
        } else {
            fallback
        }
    } else {
        normalized
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
            PluginType::Plugin | PluginType::Channel => {
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
            PluginType::Brain => {
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

pub(crate) fn sdk_plugin_entry_from_installed_plugin(
    plugin: &InstalledPlugin,
) -> Option<PluginEntry> {
    let manifest = Manifest::from_json(&plugin.manifest).ok()?;
    if !matches!(
        manifest.plugin_type,
        PluginType::Plugin | PluginType::Channel
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
    permissions.allowed_network_domains = manifest
        .permissions
        .network
        .iter()
        .map(|pattern| pattern.0.clone())
        .collect();
    permissions.allowed_secret_keys = manifest.permissions.secrets.clone();
    permissions.secret_host_patterns = manifest
        .permissions
        .host_patterns
        .iter()
        .map(|pattern| pattern.0.clone())
        .collect();
    permissions.memory_read = manifest.permissions.memory_read;
    permissions.memory_write = manifest.permissions.memory_write;
    permissions.wasm_max_memory_mb = manifest.permissions.wasm_max_memory_mb;
    permissions.wasm_fuel_limit = manifest.permissions.wasm_fuel_limit;
    permissions.max_execution_time = manifest.permissions.max_execution_time;

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
}

fn sdk_manifest_from_installed_plugins(installed_plugins: &[InstalledPlugin]) -> SdkManifest {
    let plugins = installed_plugins
        .iter()
        .filter_map(sdk_plugin_entry_from_installed_plugin)
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
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use crate::config::Config;
    use crate::runtime::mcp::SandboxProfile;
    use crate::storage::{Database, InstalledPlugin};

    use super::{merge_mcp_configs, sdk_plugin_entry_from_installed_plugin, RuntimeManager};

    fn write_driver_bundle(
        data_root: &Path,
        install_id: &str,
        manifest_name: &str,
        enabled: bool,
    ) -> (PathBuf, String, String, String) {
        let install_dir = data_root.join("drivers").join(install_id);
        fs::create_dir_all(&install_dir).expect("create driver install dir");

        let manifest = format!(
            r#"{{
                "name": "{manifest_name}",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Plugin",
                "permissions": {{
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                }},
                "trust_tier": "Official",
                "min_model": null,
                "description": "Filesystem-discovered native extension"
            }}"#
        );
        fs::write(install_dir.join("manifest.json"), &manifest).expect("write manifest");
        fs::write(
            install_dir.join("runtime.json"),
            r#"{
                "tools": [
                    {
                        "name": "vision_scan",
                        "description": "Scan the screen",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        },
                        "domains": ["all"]
                    }
                ]
            }"#,
        )
        .expect("write runtime");

        let binary_path = install_dir.join("vision-plus.dylib");
        fs::write(&binary_path, b"native driver bytes").expect("write binary");
        let payload_hash = "LOCAL_DEV_PAYLOAD_HASH".to_string();
        let payload_signature = "LOCAL_DEV_PAYLOAD_SIGNATURE".to_string();
        fs::write(
            install_dir.join("plugin-package.json"),
            format!(
                r#"{{
                    "id": "{install_id}",
                    "artifact": "vision-plus.dylib",
                    "runtime_config": "runtime.json",
                    "payload_hash": "{payload_hash}",
                    "payload_signature": "{payload_signature}",
                    "enabled": {enabled}
                }}"#
            ),
        )
        .expect("write package");

        (binary_path, manifest, payload_hash, payload_signature)
    }

    #[tokio::test]
    async fn runtime_build_registers_installed_plugin_schemas() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime.db"))
            .await
            .expect("database");

        let plugin = InstalledPlugin {
            id: "echo-plugin".to_string(),
            name: "echo-plugin".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 1,
            manifest: r#"{
                "name": "echo-plugin",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Plugin",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Echo plugin"
            }"#
            .to_string(),
            binary_path: Some("echo-plugin.wasm".to_string()),
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
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
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
    async fn runtime_build_prefers_builtin_core_tools_over_installed_official_system() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime-system.db"))
            .await
            .expect("database");

        let plugin = InstalledPlugin {
            id: "terminal".to_string(),
            name: "terminal".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 0,
            manifest: r#"{
                "name": "terminal",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Plugin",
                "permissions": {
                    "filesystem": ["workspace/**"],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Official",
                "min_model": null,
                "description": "Official terminal native extension"
            }"#
            .to_string(),
            binary_path: Some("terminal.dylib".to_string()),
            binary_hash: "abc123".to_string(),
            signature: "LOCAL_DEV_PAYLOAD_SIGNATURE".to_string(),
            enabled: true,
            installed_at: 1_710_000_000,
            last_used: None,
            config: Some(
                r#"{
                    "tools": [
                        {
                            "name": "run_command",
                            "description": "Execute an allowed terminal command.",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "command": {"type": "string"}
                                },
                                "required": ["command"]
                            },
                            "domains": ["shell", "git", "code", "all"]
                        }
                    ]
                }"#
                .to_string(),
            ),
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
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
        assert!(runtime.registry.terminal.is_some());

        let schema = runtime
            .registry
            .schemas_for("shell")
            .await
            .into_iter()
            .find(|schema| schema.name == "run_command")
            .expect("run_command schema");
        assert!(matches!(
            schema.source,
            crate::runtime::registry::ToolSource::Builtin
        ));
    }

    #[tokio::test]
    async fn runtime_build_registers_builtin_core_tools_even_without_installs() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime-no-builtins.db"))
            .await
            .expect("database");

        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.mcp.servers.clear();
        config.plugins.fs_editor = true;
        config.plugins.terminal = true;
        let runtime = RuntimeManager::build(&database, &config)
            .await
            .expect("runtime manager");
        let schemas = runtime.registry.schemas_for("all").await;

        assert!(runtime.registry.fs.is_some());
        assert!(runtime.registry.terminal.is_some());
        assert!(schemas.iter().any(|schema| schema.name == "read_file"));
        assert!(schemas.iter().any(|schema| schema.name == "run_command"));
    }

    #[tokio::test]
    async fn runtime_build_discovers_driver_from_filesystem_when_db_missing() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime-driver-discovery.db"))
            .await
            .expect("database");

        write_driver_bundle(data.path(), "vision-plus", "Vision Plus", true);

        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.core.data_dir = data.path().to_path_buf();
        config.mcp.servers.clear();

        let runtime = RuntimeManager::build(&database, &config)
            .await
            .expect("runtime manager");
        let schemas = runtime.registry.schemas_for("all").await;

        assert!(schemas.iter().any(|schema| schema.name == "vision_scan"));
    }

    #[tokio::test]
    async fn runtime_build_does_not_resurrect_disabled_driver_from_db() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime-driver-disabled.db"))
            .await
            .expect("database");

        let (binary_path, manifest, payload_hash, payload_signature) =
            write_driver_bundle(data.path(), "vision-plus", "Vision Plus", true);

        let driver = InstalledPlugin {
            id: "vision-plus".to_string(),
            name: "Vision Plus".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 0,
            manifest,
            binary_path: Some(binary_path.to_string_lossy().to_string()),
            binary_hash: payload_hash,
            signature: payload_signature,
            enabled: false,
            installed_at: 0,
            last_used: None,
            config: Some(
                r#"{
                    "tools": [
                        {
                            "name": "vision_scan",
                            "description": "Scan the screen",
                            "parameters": {
                                "type": "object",
                                "properties": {}
                            },
                            "domains": ["all"]
                        }
                    ]
                }"#
                .to_string(),
            ),
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
        };
        database
            .installed_plugins()
            .upsert_plugin(&driver)
            .await
            .expect("insert disabled driver");

        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.core.data_dir = data.path().to_path_buf();
        config.mcp.servers.clear();

        let runtime = RuntimeManager::build(&database, &config)
            .await
            .expect("runtime manager");
        let schemas = runtime.registry.schemas_for("all").await;

        assert!(!schemas.iter().any(|schema| schema.name == "vision_scan"));
    }

    #[tokio::test]
    async fn runtime_build_respects_explicit_profile_loadout() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let database = Database::new(&data.path().join("runtime-loadout.db"))
            .await
            .expect("database");

        let skill = InstalledPlugin {
            id: "echo-plugin".to_string(),
            name: "echo-plugin".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 1,
            manifest: r#"{
                "name": "echo-plugin",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Plugin",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Echo plugin"
            }"#
            .to_string(),
            binary_path: Some("echo-plugin.wasm".to_string()),
            binary_hash: "hash".to_string(),
            signature: "sig".to_string(),
            enabled: true,
            installed_at: 0,
            last_used: None,
            config: Some(
                r#"{
                    "tools": [
                        {
                            "name": "echo_text",
                            "description": "Echo a string",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "text": {"type": "string"}
                                }
                            },
                            "domains": ["all"]
                        }
                    ]
                }"#
                .to_string(),
            ),
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
        };
        let driver = InstalledPlugin {
            id: "vision-plus".to_string(),
            name: "vision-plus".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 0,
            manifest: r#"{
                "name": "vision-plus",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Plugin",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Official",
                "min_model": null,
                "description": "Vision native extension"
            }"#
            .to_string(),
            binary_path: Some("vision-plus.dylib".to_string()),
            binary_hash: "hash".to_string(),
            signature: "LOCAL_DEV_PAYLOAD_SIGNATURE".to_string(),
            enabled: true,
            installed_at: 0,
            last_used: None,
            config: Some(
                r#"{
                    "tools": [
                        {
                            "name": "vision_scan",
                            "description": "Scan the screen",
                            "parameters": {
                                "type": "object",
                                "properties": {}
                            },
                            "domains": ["all"]
                        }
                    ]
                }"#
                .to_string(),
            ),
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
        };

        database
            .installed_plugins()
            .upsert_plugin(&skill)
            .await
            .expect("insert skill");
        database
            .installed_plugins()
            .upsert_plugin(&driver)
            .await
            .expect("insert driver");

        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.mcp.servers.clear();
        config.active_profile = Some("desktop".to_string());
        config.profiles.insert(
            "desktop".to_string(),
            crate::config::ProfileConfig {
                loadout: "developer".to_string(),
                ..Default::default()
            },
        );
        config.loadouts.insert(
            "developer".to_string(),
            crate::config::LoadoutConfig {
                builtins: vec!["filesystem".to_string(), "terminal".to_string()],
                drivers: vec![],
                plugins: vec!["echo-plugin".to_string()],
            },
        );

        let runtime = RuntimeManager::build(&database, &config)
            .await
            .expect("runtime manager");
        let schemas = runtime.registry.schemas_for("all").await;

        assert!(runtime.registry.fs.is_some());
        assert!(runtime.registry.terminal.is_some());
        assert!(schemas.iter().any(|schema| schema.name == "echo_text"));
        assert!(!schemas.iter().any(|schema| schema.name == "vision_scan"));
        assert!(!schemas.iter().any(|schema| schema.name == "capture_screen"));
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

    #[test]
    fn sdk_plugin_entry_preserves_network_and_memory_permissions() {
        let plugin = InstalledPlugin {
            id: "net-plugin".to_string(),
            name: "net-plugin".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 1,
            manifest: r#"{
                "name": "net-plugin",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Plugin",
                "permissions": {
                    "filesystem": [],
                    "network": ["api.example.com", "*.example.net"],
                    "memory_read": true,
                    "memory_write": true,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "network plugin"
            }"#
            .to_string(),
            binary_path: Some("net-plugin.wasm".to_string()),
            binary_hash: "hash".to_string(),
            signature: "sig".to_string(),
            enabled: true,
            installed_at: 0,
            last_used: None,
            config: None,
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
        };

        let entry = sdk_plugin_entry_from_installed_plugin(&plugin).expect("entry");
        assert_eq!(
            entry.permissions.allowed_network_domains,
            vec!["api.example.com".to_string(), "*.example.net".to_string()]
        );
        assert!(entry.permissions.memory_read);
        assert!(entry.permissions.memory_write);
    }
}
