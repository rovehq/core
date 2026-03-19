//! MCP server spawner and lifecycle manager.

mod call;
mod lifecycle;
#[cfg(test)]
mod tests;

use super::sandbox::SandboxProfile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::RwLock;

use sdk::errors::EngineError;

const MAX_RESTART_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub profile: SandboxProfile,
    #[serde(default)]
    pub cached_tools: Vec<McpToolDescriptor>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "inputSchema", alias = "input_schema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct JsonRpcRequest {
    pub(super) jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<serde_json::Value>,
    pub(super) method: String,
    #[serde(default)]
    pub(super) params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct JsonRpcResponse {
    pub(super) jsonrpc: String,
    pub(super) id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct JsonRpcError {
    pub(super) code: i32,
    pub(super) message: String,
}

pub(super) struct McpServerInstance {
    #[allow(dead_code)]
    pub(super) config: McpServerConfig,
    pub(super) process: Child,
    pub(super) stdin: ChildStdin,
    pub(super) stdout: BufReader<ChildStdout>,
    pub(super) stderr: BufReader<ChildStderr>,
    pub(super) crash_count: u32,
    pub(super) last_activity: std::time::Instant,
}

pub struct McpSpawner {
    servers: Arc<RwLock<HashMap<String, McpServerInstance>>>,
    configs: HashMap<String, McpServerConfig>,
}

impl McpSpawner {
    pub fn new(configs: Vec<McpServerConfig>) -> Self {
        let config_map = configs
            .into_iter()
            .filter(|config| config.enabled)
            .map(|config| (config.name.clone(), config))
            .collect();

        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            configs: config_map,
        }
    }
}

pub struct McpServer {
    spawner: Arc<McpSpawner>,
    name: String,
}

impl McpServer {
    pub fn new(spawner: Arc<McpSpawner>, name: String) -> Self {
        Self { spawner, name }
    }

    pub async fn call_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, EngineError> {
        self.spawner.call_tool(&self.name, tool_name, params).await
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, EngineError> {
        self.spawner.list_tools(&self.name).await
    }

    pub async fn is_running(&self) -> bool {
        self.spawner.is_running(&self.name).await
    }

    pub async fn stop(&self) -> Result<(), EngineError> {
        self.spawner.stop_server(&self.name).await
    }
}
