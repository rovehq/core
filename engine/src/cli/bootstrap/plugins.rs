use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::api::mcp::McpSpawner;
use crate::config::Config;
use crate::runtime::wasm::WasmRuntime;
use crate::security::crypto::CryptoModule;
use crate::security::fs_guard::FileSystemGuard;
use crate::storage::Database;
use crate::tools::{FilesystemTool, TerminalTool, ToolRegistry, VisionTool};

pub async fn build(database: &Database, config: &Config) -> Result<Arc<ToolRegistry>> {
    let mut tools = ToolRegistry::empty();
    tools.terminal = Some(TerminalTool::new(
        config.core.workspace.to_string_lossy().to_string(),
    ));
    tools.fs = Some(FilesystemTool::new(config.core.workspace.clone())?);

    if config.plugins.screenshot {
        tools.vision = Some(VisionTool::new(config.core.workspace.clone()));
    }

    let plugins = database.plugins().get_enabled_plugins().await?;
    if !plugins.is_empty() {
        let manifest = sdk::manifest::Manifest {
            version: "1.0.0".to_string(),
            team_public_key: String::new(),
            signature: String::new(),
            generated_at: String::new(),
            core_tools: Vec::new(),
            plugins: plugins
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

        match CryptoModule::new() {
            Ok(crypto) => {
                let fs_guard = Arc::new(FileSystemGuard::new(config.core.workspace.clone())?);
                let mut wasm_runtime = WasmRuntime::new(manifest, Arc::new(crypto), fs_guard);

                for plugin in &plugins {
                    tools.register_tools_from_plugin_manifest(&plugin.name, &plugin.manifest_json);
                    if let Err(error) = wasm_runtime.load_plugin(&plugin.name).await {
                        tracing::warn!("Failed to load plugin {}: {}", plugin.name, error);
                    }
                }

                tools.wasm_runtime = Some(Arc::new(Mutex::new(wasm_runtime)));
            }
            Err(error) => {
                tracing::warn!("Failed to initialize plugin crypto module: {}", error);
            }
        }
    }

    if !config.mcp.servers.is_empty() {
        let spawner = Arc::new(McpSpawner::new(config.mcp.servers.clone()));
        if !spawner.configured_servers().is_empty() {
            let keepalive = spawner.clone();
            tokio::spawn(async move {
                keepalive.keepalive_loop().await;
            });

            tools.register_mcp_spawner(spawner.clone());

            for server_name in spawner.configured_servers() {
                match spawner.list_tools(&server_name).await {
                    Ok(discovered) => {
                        for tool in discovered {
                            tools.register_mcp_tool(
                                &server_name,
                                &tool.name,
                                &tool.description,
                                tool.input_schema,
                            );
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            "Failed to discover MCP tools for {}: {}",
                            server_name,
                            error
                        );
                    }
                }
            }
        }
    }
    Ok(Arc::new(tools))
}
