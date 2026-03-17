use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::catalog::{derive_domains_from_name, McpToolInfo, WasmToolInfo};
use super::{FilesystemTool, TerminalTool, VisionTool};
use crate::api::mcp::McpSpawner;
use crate::runtime::WasmRuntime;

/// Registry of available tools that can be dispatched by the agent.
///
/// Holds optional references to each core tool. Only tools that are present
/// are advertised in the system prompt and available for dispatch.
pub struct ToolRegistry {
    pub fs: Option<FilesystemTool>,
    pub terminal: Option<TerminalTool>,
    pub vision: Option<VisionTool>,
    pub wasm_runtime: Option<Arc<Mutex<WasmRuntime>>>,
    pub wasm_tools: Vec<WasmToolInfo>,
    pub mcp_spawner: Option<Arc<McpSpawner>>,
    pub mcp_tools: Vec<McpToolInfo>,
}

impl ToolRegistry {
    /// Get the workspace path from the filesystem tool if available.
    pub fn workspace(&self) -> Option<&Path> {
        self.fs.as_ref().map(|fs| fs.workspace())
    }

    /// Create an empty registry with no tools enabled.
    pub fn empty() -> Self {
        Self {
            fs: None,
            terminal: None,
            vision: None,
            wasm_runtime: None,
            wasm_tools: Vec::new(),
            mcp_spawner: None,
            mcp_tools: Vec::new(),
        }
    }

    /// Auto-discover and register all plugin tools from a signed manifest.
    pub fn register_plugin_tools(&mut self, manifest: &sdk::manifest::Manifest) {
        for plugin in &manifest.plugins {
            for tool_entry in manifest
                .plugins
                .iter()
                .filter(|candidate| candidate.name == plugin.name)
                .take(1)
            {
                let name = tool_entry.name.clone();
                if self.wasm_tools.iter().any(|tool| tool.name == name) {
                    debug!("Plugin tool '{}' already registered, skipping", name);
                    continue;
                }

                let domains = derive_domains_from_name(&name);

                self.wasm_tools.push(WasmToolInfo {
                    name: name.clone(),
                    description: format!("WASM plugin '{}' (version {})", name, tool_entry.version),
                    parameters: serde_json::json!({"input": "string"}),
                    plugin_name: name,
                    domains,
                });
            }
        }
    }

    /// Register individual tools from a per-plugin `manifest.json` sidecar.
    pub fn register_tools_from_plugin_manifest(
        &mut self,
        plugin_name: &str,
        plugin_manifest_json: &str,
    ) {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(plugin_manifest_json) else {
            warn!("Failed to parse plugin manifest for '{}'", plugin_name);
            return;
        };

        let tools = match val.get("tools").and_then(|tool| tool.as_array()) {
            Some(tools) => tools.clone(),
            None => return,
        };

        self.wasm_tools.retain(|tool| {
            tool.plugin_name != plugin_name
                || tools
                    .iter()
                    .any(|entry| entry["name"].as_str() == Some(&tool.name))
        });

        let domains = derive_domains_from_name(plugin_name);

        for tool in tools {
            let name = match tool["name"].as_str() {
                Some(name) => name.to_string(),
                None => continue,
            };
            if self.wasm_tools.iter().any(|entry| entry.name == name) {
                continue;
            }

            let description = tool["description"]
                .as_str()
                .unwrap_or("WASM tool")
                .to_string();
            let parameters = tool
                .get("parameters")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            self.wasm_tools.push(WasmToolInfo {
                name,
                description,
                parameters,
                plugin_name: plugin_name.to_string(),
                domains: domains.clone(),
            });
        }
    }

    /// Register MCP spawner and discover available tools.
    pub fn register_mcp_spawner(&mut self, spawner: Arc<McpSpawner>) {
        self.mcp_spawner = Some(spawner);
    }

    /// Register an MCP tool from a server.
    pub fn register_mcp_tool(
        &mut self,
        server_name: &str,
        tool_name: &str,
        description: &str,
        parameters: serde_json::Value,
    ) {
        let name = format!("mcp_{}_{}", server_name, tool_name);

        if self.mcp_tools.iter().any(|tool| tool.name == name) {
            debug!("MCP tool '{}' already registered, skipping", name);
            return;
        }

        let domains = derive_domains_from_name(tool_name);

        self.mcp_tools.push(McpToolInfo {
            name,
            description: description.to_string(),
            parameters,
            server_name: server_name.to_string(),
            domains,
        });
    }
}
