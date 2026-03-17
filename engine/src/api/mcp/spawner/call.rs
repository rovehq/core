use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tracing::{debug, warn};

use sdk::errors::EngineError;

use super::{JsonRpcRequest, JsonRpcResponse, McpSpawner, MAX_RESTART_ATTEMPTS};

impl McpSpawner {
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, EngineError> {
        match self
            .call_tool_once(server_name, tool_name, params.clone())
            .await
        {
            Ok(result) => Ok(result),
            Err(error) if error.to_string().contains("connection lost") => {
                warn!(server = server_name, "Retrying after crash");
                self.call_tool_once(server_name, tool_name, params).await
            }
            Err(error) => Err(error),
        }
    }

    async fn call_tool_once(
        &self,
        server_name: &str,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, EngineError> {
        self.start_server(server_name).await?;

        debug!(server = server_name, tool = tool_name, "Calling MCP tool");

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::String(uuid::Uuid::new_v4().to_string()),
            method: tool_name.to_string(),
            params,
        };
        let request_json = serde_json::to_string(&request).map_err(|error| {
            EngineError::Plugin(format!("failed to serialize request: {}", error))
        })?;

        let mut servers = self.servers.write().await;
        let instance = servers
            .get_mut(server_name)
            .ok_or_else(|| EngineError::Plugin(format!("server {} not running", server_name)))?;

        instance
            .stdin
            .write_all(request_json.as_bytes())
            .await
            .map_err(EngineError::Io)?;
        instance
            .stdin
            .write_all(b"\n")
            .await
            .map_err(EngineError::Io)?;
        instance.stdin.flush().await.map_err(EngineError::Io)?;

        let mut response_line = String::new();
        let read_result = instance.stdout.read_line(&mut response_line).await;
        instance.last_activity = std::time::Instant::now();

        match read_result {
            Ok(0) | Err(_) => {
                warn!(server = server_name, "MCP server connection lost");
                instance.crash_count += 1;
                let crash_count = instance.crash_count;
                drop(servers);

                if crash_count >= MAX_RESTART_ATTEMPTS {
                    return Err(EngineError::Plugin(format!(
                        "MCP server {} crashed {} times, refusing to restart",
                        server_name, crash_count
                    )));
                }

                warn!(
                    server = server_name,
                    attempt = crash_count,
                    "Attempting to restart MCP server"
                );
                self.stop_server(server_name).await?;
                self.start_server(server_name).await?;
                return Err(EngineError::Plugin("connection lost".to_string()));
            }
            Ok(_) => {}
        }

        if response_line.is_empty() {
            return Err(EngineError::Plugin(format!(
                "MCP server {} returned empty response",
                server_name
            )));
        }

        let detector = crate::injection_detector::InjectionDetector::new().map_err(|error| {
            EngineError::Plugin(format!("failed to create injection detector: {}", error))
        })?;
        if let Some(warning) = detector.scan(&response_line) {
            warn!(
                server = server_name,
                tool = tool_name,
                pattern = warning.matched_pattern,
                position = warning.position,
                "Injection detected in MCP response"
            );
            return Err(EngineError::Plugin(
                "injection detected in MCP response".to_string(),
            ));
        }

        let response: JsonRpcResponse = serde_json::from_str(&response_line).map_err(|error| {
            EngineError::Plugin(format!("failed to parse JSON-RPC response: {}", error))
        })?;
        if let Some(error) = response.error {
            return Err(EngineError::Plugin(format!(
                "MCP tool error: {} (code {})",
                error.message, error.code
            )));
        }

        response
            .result
            .ok_or_else(|| EngineError::Plugin("MCP response missing result field".to_string()))
    }
}
