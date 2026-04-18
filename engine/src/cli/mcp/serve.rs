use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tracing::warn;

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::runtime::registry::ToolSchema;
use crate::runtime::ToolRegistry;
use crate::storage::Database;
use sdk::TaskSource;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[allow(dead_code)]
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct McpToolDescriptor {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

pub async fn serve_stdio(config: &Config) -> Result<()> {
    let database = Database::new(&database_path(config))
        .await
        .context("failed to open database for MCP server")?;
    let runtime = crate::runtime::RuntimeManager::build(&database, config)
        .await
        .context("failed to build runtime for MCP server")?;

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = BufWriter::new(io::stdout());

    while let Some(line) = lines
        .next_line()
        .await
        .context("failed to read MCP request")?
    {
        if line.trim().is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                warn!("Ignoring malformed MCP request: {}", error);
                continue;
            }
        };

        if request.jsonrpc != "2.0" {
            if let Some(id) = request.id {
                write_response(
                    &mut stdout,
                    JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32600,
                            message: "invalid jsonrpc version".to_string(),
                        }),
                    },
                )
                .await?;
            }
            continue;
        }

        let Some(id) = request.id.clone() else {
            handle_notification(&request.method).await;
            continue;
        };

        let response = match handle_request(&runtime.registry, request).await {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: error_code_for(&error),
                    message: error,
                }),
            },
        };

        write_response(&mut stdout, response).await?;
    }

    stdout.flush().await.context("failed to flush MCP stdout")?;
    Ok(())
}

async fn handle_notification(method: &str) {
    if method != "notifications/initialized" {
        warn!("Ignoring unsupported MCP notification '{}'", method);
    }
}

async fn handle_request(
    registry: &ToolRegistry,
    request: JsonRpcRequest,
) -> std::result::Result<Value, String> {
    match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "rove",
                "version": env!("CARGO_PKG_VERSION"),
            }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools = registry
                .all_schemas()
                .await
                .into_iter()
                .map(tool_descriptor)
                .collect::<Vec<_>>();
            Ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = request
                .params
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "tools/call requires a non-empty tool name".to_string())?;
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = registry
                .call(name, arguments, "mcp-serve", &TaskSource::Cli)
                .await
                .map_err(|error| error.to_string())?;
            Ok(json!({
                "content": [
                    {
                        "type": "text",
                        "text": stringify_result(result),
                    }
                ],
                "isError": false,
            }))
        }
        other => Err(format!("unsupported MCP method '{}'", other)),
    }
}

fn tool_descriptor(schema: ToolSchema) -> McpToolDescriptor {
    McpToolDescriptor {
        name: schema.name,
        description: schema.description,
        input_schema: schema.parameters,
    }
}

fn stringify_result(result: Value) -> String {
    match result {
        Value::String(text) => text,
        other => serde_json::to_string(&other).unwrap_or_else(|_| "{}".to_string()),
    }
}

fn error_code_for(message: &str) -> i32 {
    if message.starts_with("unsupported MCP method") {
        -32601
    } else if message.contains("requires a non-empty tool name") {
        -32602
    } else {
        -32000
    }
}

async fn write_response(
    stdout: &mut BufWriter<io::Stdout>,
    response: JsonRpcResponse,
) -> Result<()> {
    let encoded = serde_json::to_string(&response).context("failed to serialize MCP response")?;
    stdout
        .write_all(encoded.as_bytes())
        .await
        .context("failed to write MCP response")?;
    stdout
        .write_all(b"\n")
        .await
        .context("failed to terminate MCP response")?;
    stdout
        .flush()
        .await
        .context("failed to flush MCP response")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{handle_request, stringify_result, tool_descriptor};
    use crate::runtime::ToolRegistry;

    #[tokio::test]
    async fn tools_list_includes_registered_builtin() {
        let mut registry = ToolRegistry::empty();
        registry.register_builtin_web_fetch().await;
        registry.register_builtin_web_search().await;

        let result = handle_request(
            &registry,
            super::JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::json!("1")),
                method: "tools/list".to_string(),
                params: serde_json::json!({}),
            },
        )
        .await
        .unwrap();

        let tools = result
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(tools.iter().any(|tool| {
            tool.get("name").and_then(serde_json::Value::as_str) == Some("web_fetch")
        }));
        assert!(tools.iter().any(|tool| {
            tool.get("name").and_then(serde_json::Value::as_str) == Some("web_search")
        }));
    }

    #[test]
    fn stringify_result_preserves_strings() {
        assert_eq!(stringify_result(serde_json::json!("hello")), "hello");
        assert_eq!(
            stringify_result(serde_json::json!({"ok": true})),
            r#"{"ok":true}"#
        );
    }

    #[test]
    fn tool_descriptor_uses_schema_fields() {
        let descriptor = tool_descriptor(crate::runtime::registry::ToolSchema {
            name: "demo".to_string(),
            description: "example".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            source: crate::runtime::registry::ToolSource::Builtin,
            domains: vec!["all".to_string()],
        });
        assert_eq!(descriptor.name, "demo");
        assert_eq!(descriptor.description, "example");
        assert_eq!(
            descriptor.input_schema,
            serde_json::json!({"type":"object"})
        );
    }
}
