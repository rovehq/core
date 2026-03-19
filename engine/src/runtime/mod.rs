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
use tokio::sync::Mutex;
use tracing::warn;

use crate::config::Config;
use crate::security::crypto::CryptoModule;
use crate::security::fs_guard::FileSystemGuard;
use crate::storage::Database;

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
        let enabled_plugins = database
            .plugins()
            .get_enabled_plugins()
            .await
            .map_err(|error| EngineError::Database(error.to_string()))?;

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

        let wasm = if enabled_plugins.is_empty() {
            None
        } else {
            match CryptoModule::new() {
                Ok(crypto) => {
                    let fs_guard = Arc::new(FileSystemGuard::new(config.core.workspace.clone())?);
                    let manifest = SdkManifest {
                        version: "1.0.0".to_string(),
                        team_public_key: String::new(),
                        signature: String::new(),
                        generated_at: String::new(),
                        core_tools: Vec::new(),
                        plugins: enabled_plugins
                            .iter()
                            .map(|plugin| sdk::manifest::PluginEntry {
                                name: plugin.name.clone(),
                                version: plugin.version.clone(),
                                path: plugin.wasm_path.clone(),
                                hash: plugin.wasm_hash.clone(),
                                permissions: sdk::manifest::PluginPermissions::default(),
                                allowed_imports: vec![
                                    "extism:host/env".to_string(),
                                    "wasi_snapshot_preview1".to_string(),
                                ],
                                trust_tier: 0,
                            })
                            .collect(),
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

        let mcp = if config.mcp.servers.is_empty() {
            None
        } else {
            let spawner = Arc::new(McpSpawner::new(config.mcp.servers.clone()));
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

        for plugin in &enabled_plugins {
            registry
                .register_tools_from_plugin_manifest(&plugin.name, &plugin.manifest_json)
                .await;
        }

        if let Some(spawner) = &mcp {
            registry.register_mcp_spawner(Arc::clone(spawner));
            for server in &config.mcp.servers {
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
