use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tracing::warn;

use crate::api::gateway::{Gateway, GatewayConfig};
use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::runtime::registry::ToolSchema;
use crate::runtime::ToolRegistry;
use crate::storage::{Database, PendingTaskStatus};
use sdk::TaskSource;

const EXECUTE_AGENT_TOOL: &str = "rove.execute_agent";
const DEFAULT_WAIT_SECS: u64 = 300;
const MAX_WAIT_SECS: u64 = 3600;

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
    let db = Arc::new(database);
    let gateway = Arc::new(
        Gateway::new(Arc::clone(&db), GatewayConfig::from_config(config))
            .context("failed to start gateway for MCP server")?,
    );
    Arc::clone(&gateway).start();

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

        let response = match handle_request(&runtime.registry, &gateway, &db, request).await {
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
    gateway: &Arc<Gateway>,
    database: &Arc<Database>,
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
        "tools/list" => Ok(list_tools_response(registry).await),
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

            if name == EXECUTE_AGENT_TOOL {
                return run_execute_agent(gateway, database, &arguments).await;
            }

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

async fn list_tools_response(registry: &ToolRegistry) -> Value {
    let mut tools: Vec<McpToolDescriptor> = registry
        .all_schemas()
        .await
        .into_iter()
        .map(tool_descriptor)
        .collect();
    tools.push(execute_agent_tool_descriptor());
    json!({ "tools": tools })
}

fn execute_agent_tool_descriptor() -> McpToolDescriptor {
    McpToolDescriptor {
        name: EXECUTE_AGENT_TOOL.to_string(),
        description: "Submit a prompt to the Rove agent loop and wait for the final answer. \
                      Use this to delegate work to Rove from another LLM."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "Natural-language task for the Rove agent.",
                },
                "agent_id": {
                    "type": "string",
                    "description": "Optional agent id to route the task through a specific agent.",
                },
                "wait_seconds": {
                    "type": "integer",
                    "description": "Maximum seconds to wait for completion (default 300, max 3600).",
                    "minimum": 1,
                    "maximum": MAX_WAIT_SECS,
                }
            },
            "required": ["input"],
        }),
    }
}

async fn run_execute_agent(
    gateway: &Arc<Gateway>,
    database: &Arc<Database>,
    arguments: &Value,
) -> std::result::Result<Value, String> {
    let input = arguments
        .get("input")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "rove.execute_agent requires a non-empty `input`".to_string())?;

    let wait_secs = arguments
        .get("wait_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_WAIT_SECS)
        .clamp(1, MAX_WAIT_SECS);

    let execution_profile = match arguments
        .get("agent_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(agent_id) => {
            let repo = crate::system::specs::SpecRepository::new()
                .map_err(|error| format!("failed to load agent registry: {}", error))?;
            Some(
                crate::cli::agents::execution_profile_for_agent(&repo, agent_id)
                    .map_err(|error| format!("invalid agent_id '{}': {}", agent_id, error))?,
            )
        }
        None => None,
    };

    let task_id = gateway
        .submit_cli(input, None, execution_profile.as_ref())
        .await
        .map_err(|error| format!("failed to submit task: {}", error))?;

    let completion = wait_for_task_completion(database, &task_id, Duration::from_secs(wait_secs))
        .await
        .map_err(|error| format!("failed to poll task '{}': {}", task_id, error))?;

    Ok(encode_completion(task_id, completion))
}

enum McpCompletion {
    Running,
    Done { answer: String },
    Failed { error: String },
    Missing,
}

async fn wait_for_task_completion(
    database: &Arc<Database>,
    task_id: &str,
    timeout: Duration,
) -> anyhow::Result<McpCompletion> {
    let deadline = Instant::now() + timeout;
    loop {
        let pending = database.pending_tasks().get_task(task_id).await?;
        let Some(pending) = pending else {
            return Ok(McpCompletion::Missing);
        };
        match pending.status {
            PendingTaskStatus::Done => {
                let answer = database
                    .tasks()
                    .get_latest_answer(task_id)
                    .await?
                    .unwrap_or_else(|| "Task completed".to_string());
                return Ok(McpCompletion::Done { answer });
            }
            PendingTaskStatus::Failed => {
                return Ok(McpCompletion::Failed {
                    error: pending.error.unwrap_or_else(|| "Task failed".to_string()),
                });
            }
            PendingTaskStatus::Pending | PendingTaskStatus::Running => {
                if Instant::now() >= deadline {
                    return Ok(McpCompletion::Running);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

fn encode_completion(task_id: String, completion: McpCompletion) -> Value {
    match completion {
        McpCompletion::Done { answer } => json!({
            "content": [{ "type": "text", "text": answer }],
            "isError": false,
            "taskId": task_id,
            "status": "completed",
        }),
        McpCompletion::Failed { error } => json!({
            "content": [{ "type": "text", "text": error }],
            "isError": true,
            "taskId": task_id,
            "status": "failed",
        }),
        McpCompletion::Running => json!({
            "content": [{
                "type": "text",
                "text": format!("Task {} still running. Poll /v1/tasks/{}/status or re-call with larger wait_seconds.", task_id, task_id)
            }],
            "isError": false,
            "taskId": task_id,
            "status": "running",
        }),
        McpCompletion::Missing => json!({
            "content": [{ "type": "text", "text": format!("Task {} was not persisted", task_id) }],
            "isError": true,
            "taskId": task_id,
            "status": "missing",
        }),
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
    use super::{list_tools_response, stringify_result, tool_descriptor, EXECUTE_AGENT_TOOL};
    use crate::runtime::ToolRegistry;

    #[tokio::test]
    async fn tools_list_includes_registered_builtin() {
        let mut registry = ToolRegistry::empty();
        registry.register_builtin_web_fetch().await;
        registry.register_builtin_web_search().await;

        let result = list_tools_response(&registry).await;

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

    #[tokio::test]
    async fn tools_list_exposes_execute_agent_meta_tool() {
        let registry = ToolRegistry::empty();
        let result = list_tools_response(&registry).await;
        let tools = result
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(tools.iter().any(|tool| {
            tool.get("name").and_then(serde_json::Value::as_str) == Some(EXECUTE_AGENT_TOOL)
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
