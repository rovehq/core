use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, warn};

use crate::config::Config;
use crate::runtime::{
    FilesystemTool, McpSpawner, NativeRuntime, TerminalTool, VisionTool, WasmRuntime,
};
use crate::security::command_executor::CommandExecutor;
use crate::security::injection_detector::InjectionDetector;
use crate::security::risk_assessor::{
    classify_terminal_command, Operation, OperationSource, RiskAssessor, RiskTier,
};
use crate::tools::catalog::{derive_domains_from_name, McpToolInfo, WasmToolInfo};
use sdk::errors::EngineError;
use sdk::TaskSource;

/// The execution surface that owns a tool.
#[derive(Debug, Clone)]
pub enum ToolSource {
    Builtin,
    Native { lib_path: String },
    Wasm { plugin_id: String },
    Mcp { server_name: String },
}

/// Compressed schema sent to the LLM for this tool.
#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub source: ToolSource,
    pub domains: Vec<String>,
}

/// Registry of available tools across all execution surfaces.
pub struct ToolRegistry {
    pub fs: Option<FilesystemTool>,
    pub terminal: Option<TerminalTool>,
    pub vision: Option<VisionTool>,
    pub wasm_runtime: Option<Arc<Mutex<WasmRuntime>>>,
    pub wasm_tools: Vec<WasmToolInfo>,
    pub mcp_spawner: Option<Arc<McpSpawner>>,
    pub mcp_tools: Vec<McpToolInfo>,
    native_runtime: Option<Arc<Mutex<NativeRuntime>>>,
    tools: Arc<RwLock<HashMap<String, ToolSchema>>>,
    risk_assessor: RiskAssessor,
    config: Arc<Config>,
}

impl ToolRegistry {
    pub fn empty() -> Self {
        Self {
            fs: None,
            terminal: None,
            vision: None,
            wasm_runtime: None,
            wasm_tools: Vec::new(),
            mcp_spawner: None,
            mcp_tools: Vec::new(),
            native_runtime: None,
            tools: Arc::new(RwLock::new(HashMap::new())),
            risk_assessor: RiskAssessor::new(),
            config: Arc::new(Config::default()),
        }
    }

    pub fn new(
        config: Arc<Config>,
        native: Option<Arc<Mutex<NativeRuntime>>>,
        wasm: Option<Arc<Mutex<WasmRuntime>>>,
        mcp: Option<Arc<McpSpawner>>,
    ) -> Self {
        Self {
            fs: None,
            terminal: None,
            vision: None,
            wasm_runtime: wasm,
            wasm_tools: Vec::new(),
            mcp_spawner: mcp,
            mcp_tools: Vec::new(),
            native_runtime: native,
            tools: Arc::new(RwLock::new(HashMap::new())),
            risk_assessor: RiskAssessor::new(),
            config,
        }
    }

    pub fn workspace(&self) -> Option<&Path> {
        self.fs.as_ref().map(|fs| fs.workspace())
    }

    pub async fn register(&self, schema: ToolSchema) {
        self.tools.write().await.insert(schema.name.clone(), schema);
    }

    pub async fn register_builtin_filesystem(&mut self, tool: FilesystemTool) {
        self.fs = Some(tool);
        self.register(ToolSchema {
            name: "read_file".to_string(),
            description: "Read the contents of a file.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "read".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "write_file".to_string(),
            description: "Write content to a file.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "write".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "delete_file".to_string(),
            description: "Delete a file.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "write".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "list_dir".to_string(),
            description: "List files in a directory.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "read".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "file_exists".to_string(),
            description: "Check whether a path exists.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "read".to_string(), "all".to_string()],
        })
        .await;
    }

    pub async fn register_builtin_terminal(&mut self, tool: TerminalTool) {
        self.terminal = Some(tool);
        self.register(ToolSchema {
            name: "run_command".to_string(),
            description: "Execute an allowed terminal command.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
            source: ToolSource::Builtin,
            domains: vec!["shell".to_string(), "git".to_string(), "code".to_string(), "all".to_string()],
        })
        .await;
    }

    pub async fn register_builtin_vision(&mut self, tool: VisionTool) {
        self.vision = Some(tool);
        self.register(ToolSchema {
            name: "capture_screen".to_string(),
            description: "Capture a screenshot.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"output_file":{"type":"string"}}}),
            source: ToolSource::Builtin,
            domains: vec!["vision".to_string(), "all".to_string()],
        })
        .await;
    }

    pub async fn register_tools_from_plugin_manifest(
        &mut self,
        plugin_name: &str,
        plugin_manifest_json: &str,
    ) {
        let Ok(val) = serde_json::from_str::<Value>(plugin_manifest_json) else {
            warn!("Failed to parse plugin manifest for '{}'", plugin_name);
            return;
        };

        let tools = match val.get("tools").and_then(Value::as_array) {
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
            let Some(name) = tool["name"].as_str().map(ToOwned::to_owned) else {
                continue;
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
                .unwrap_or_else(|| serde_json::json!({}));

            self.wasm_tools.push(WasmToolInfo {
                name: name.clone(),
                description: description.clone(),
                parameters: parameters.clone(),
                plugin_name: plugin_name.to_string(),
                domains: domains.clone(),
            });

            self.register(ToolSchema {
                name,
                description,
                parameters,
                source: ToolSource::Wasm {
                    plugin_id: plugin_name.to_string(),
                },
                domains: domains.clone(),
            })
            .await;
        }
    }

    pub async fn register_wasm_tool(
        &mut self,
        plugin_name: &str,
        tool_name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
        domains: Vec<String>,
    ) {
        let name = tool_name.into();
        if self.wasm_tools.iter().any(|entry| entry.name == name) {
            debug!("WASM tool '{}' already registered, skipping", name);
            return;
        }

        let description = description.into();
        self.wasm_tools.push(WasmToolInfo {
            name: name.clone(),
            description: description.clone(),
            parameters: parameters.clone(),
            plugin_name: plugin_name.to_string(),
            domains: domains.clone(),
        });

        self.register(ToolSchema {
            name,
            description,
            parameters,
            source: ToolSource::Wasm {
                plugin_id: plugin_name.to_string(),
            },
            domains,
        })
        .await;
    }

    pub fn register_mcp_spawner(&mut self, spawner: Arc<McpSpawner>) {
        self.mcp_spawner = Some(spawner);
    }

    pub async fn register_mcp_tool(
        &mut self,
        server_name: &str,
        tool_name: &str,
        description: &str,
        parameters: Value,
    ) {
        let name = format!("mcp_{}_{}", server_name, tool_name);
        if self.mcp_tools.iter().any(|tool| tool.name == name) {
            debug!("MCP tool '{}' already registered, skipping", name);
            return;
        }

        let domains = derive_domains_from_name(tool_name);

        self.mcp_tools.push(McpToolInfo {
            name: name.clone(),
            description: description.to_string(),
            parameters: parameters.clone(),
            server_name: server_name.to_string(),
            domains: domains.clone(),
        });

        self.register(ToolSchema {
            name,
            description: description.to_string(),
            parameters,
            source: ToolSource::Mcp {
                server_name: server_name.to_string(),
            },
            domains,
        })
        .await;
    }

    pub async fn register_native_tool(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
        lib_path: impl Into<String>,
        domains: Vec<String>,
    ) {
        self.register(ToolSchema {
            name: name.into(),
            description: description.into(),
            parameters,
            source: ToolSource::Native {
                lib_path: lib_path.into(),
            },
            domains,
        })
        .await;
    }

    pub async fn schemas_for(&self, domain: &str) -> Vec<ToolSchema> {
        self.tools
            .read()
            .await
            .values()
            .filter(|schema| {
                schema.domains.is_empty()
                    || schema
                        .domains
                        .iter()
                        .any(|candidate| candidate == domain || candidate == "all")
            })
            .cloned()
            .collect()
    }

    pub async fn call(
        &self,
        name: &str,
        args: Value,
        task_id: &str,
        source: &TaskSource,
    ) -> Result<Value, EngineError> {
        let schema = self
            .tools
            .read()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;

        self.check_paths(&args)?;
        self.check_command_gate(name, &args)?;
        self.assess_risk(task_id, name, &args, source).await?;

        let raw_result = match &schema.source {
            ToolSource::Builtin => self.call_builtin(name, args).await?,
            ToolSource::Native { lib_path } => self.call_native(lib_path, name, args).await?,
            ToolSource::Wasm { plugin_id } => self.call_wasm(plugin_id, name, args).await?,
            ToolSource::Mcp { server_name } => {
                let remote_name = self.remote_mcp_tool_name(server_name, name);
                self.call_mcp(server_name, &remote_name, args).await?
            }
        };

        self.scrub_output(raw_result)
    }

    fn remote_mcp_tool_name(&self, server_name: &str, registry_name: &str) -> String {
        registry_name
            .strip_prefix(&format!("mcp_{}_", server_name))
            .unwrap_or(registry_name)
            .to_string()
    }

    fn check_paths(&self, args: &Value) -> Result<(), EngineError> {
        let Some(fs) = &self.fs else {
            return Ok(());
        };

        for candidate in self.extract_path_like_values(args) {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            fs.validate_candidate_path(Path::new(candidate))?;
        }

        Ok(())
    }

    fn check_command_gate(&self, tool_name: &str, args: &Value) -> Result<(), EngineError> {
        if tool_name != "run_command" {
            return Ok(());
        }

        let Some(command) = args.get("command").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(parts) = shlex::split(command) else {
            return Err(EngineError::ToolError(
                "invalid shell-style quoting in command".to_string(),
            ));
        };
        if parts.is_empty() {
            return Err(EngineError::ToolError("empty command".to_string()));
        }

        let executor = CommandExecutor::new();
        executor
            .validate(&parts[0], &parts[1..])
            .map_err(|error| match error {
                crate::security::command_executor::CommandError::CommandNotAllowed(cmd) => {
                    EngineError::CommandNotAllowed(cmd)
                }
                crate::security::command_executor::CommandError::ShellInjectionAttempt => {
                    EngineError::ShellInjectionAttempt
                }
                crate::security::command_executor::CommandError::ShellMetacharactersDetected(
                    arg,
                ) => EngineError::ShellMetacharactersDetected(arg),
                crate::security::command_executor::CommandError::DangerousPipeDetected => {
                    EngineError::DangerousPipeDetected
                }
                crate::security::command_executor::CommandError::ExecutionFailed(error) => {
                    EngineError::Io(error)
                }
            })
    }

    async fn assess_risk(
        &self,
        task_id: &str,
        tool_name: &str,
        args: &Value,
        source: &TaskSource,
    ) -> Result<(), EngineError> {
        let operation = Operation::new(
            self.tool_operation_name(tool_name, args),
            self.string_args(args),
            operation_source_from_task_source(source),
        );
        let tier = self.risk_assessor.assess(&operation)?;

        match tier {
            RiskTier::Tier0 => Ok(()),
            RiskTier::Tier1 => self.confirm_tier1(task_id, tool_name).await,
            RiskTier::Tier2 => self.confirm_tier2(task_id, tool_name).await,
        }
    }

    async fn confirm_tier1(&self, _task_id: &str, tool_name: &str) -> Result<(), EngineError> {
        if !self.config.security.confirm_tier1 || self.config.security.max_risk_tier < 1 {
            return Ok(());
        }

        println!("\n[Tier 1 Risk] The agent wants to run: {}", tool_name);
        println!(
            "Auto-approving in {} seconds. Press ENTER to approve now...",
            self.config.security.confirm_tier1_delay
        );

        let mut reader = BufReader::new(tokio::io::stdin());
        let mut buf = String::new();
        match tokio::time::timeout(
            Duration::from_secs(self.config.security.confirm_tier1_delay),
            reader.read_line(&mut buf),
        )
        .await
        {
            Ok(Ok(_)) | Ok(Err(_)) | Err(_) => Ok(()),
        }
    }

    async fn confirm_tier2(&self, _task_id: &str, tool_name: &str) -> Result<(), EngineError> {
        if self.config.security.max_risk_tier < 2 {
            return Err(EngineError::OperationAbortedByUser);
        }
        if !self.config.security.require_explicit_tier2 {
            return Ok(());
        }

        println!(
            "\n[Tier 2 Risk] The agent wants to run a potentially dangerous operation: {}",
            tool_name
        );
        println!("To approve, type 'Y' and press ENTER. Any other input aborts.");

        let mut reader = BufReader::new(tokio::io::stdin());
        let mut buf = String::new();
        match reader.read_line(&mut buf).await {
            Ok(_)
                if buf.trim().eq_ignore_ascii_case("y")
                    || buf.trim().eq_ignore_ascii_case("yes") =>
            {
                Ok(())
            }
            Ok(_) => Err(EngineError::OperationAbortedByUser),
            Err(_) => Err(EngineError::OperationAbortedByUser),
        }
    }

    fn string_args(&self, args: &Value) -> Vec<String> {
        match args {
            Value::Object(map) => map
                .values()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn tool_operation_name(&self, tool_name: &str, args: &Value) -> &'static str {
        match tool_name {
            "read_file" | "list_dir" | "file_exists" => "read_file",
            "write_file" => "write_file",
            "delete_file" => "delete_file",
            "run_command" => args
                .get("command")
                .and_then(Value::as_str)
                .map(classify_terminal_command)
                .unwrap_or("execute_command"),
            "capture_screen" => "read_file",
            _ => "execute_task",
        }
    }

    fn extract_path_like_values(&self, args: &Value) -> Vec<String> {
        const PATH_KEYS: &[&str] = &["path", "output_file", "workspace", "cwd", "file"];
        match args {
            Value::Object(map) => PATH_KEYS
                .iter()
                .filter_map(|key| map.get(*key))
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
            _ => Vec::new(),
        }
    }

    async fn call_builtin(&self, name: &str, args: Value) -> Result<Value, EngineError> {
        match name {
            "read_file" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                fs.read_file(path)
                    .await
                    .map(Value::String)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "write_file" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                fs.write_file(path, content)
                    .await
                    .map(Value::String)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "delete_file" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                fs.delete_file(path)
                    .await
                    .map(Value::String)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "list_dir" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
                fs.list_dir(path)
                    .await
                    .map(Value::String)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "file_exists" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                fs.file_exists(path)
                    .await
                    .map(Value::Bool)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "run_command" => {
                let terminal = self
                    .terminal
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let command = args
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                terminal
                    .execute(command)
                    .await
                    .map(Value::String)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "capture_screen" => {
                let vision = self
                    .vision
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let output_file = args
                    .get("output_file")
                    .and_then(Value::as_str)
                    .unwrap_or("screenshot.png");
                vision
                    .capture_screen(output_file)
                    .await
                    .map(|path| Value::String(path.display().to_string()))
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            _ => Err(EngineError::ToolNotFound(name.to_string())),
        }
    }

    async fn call_wasm(
        &self,
        plugin_id: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, EngineError> {
        let wasm_runtime = self
            .wasm_runtime
            .as_ref()
            .ok_or_else(|| EngineError::ToolNotLoaded(tool_name.to_string()))?;
        let mut runtime = wasm_runtime.lock().await;
        let input =
            serde_json::to_vec(&args).map_err(|error| EngineError::ToolError(error.to_string()))?;
        let output = runtime.call_plugin(plugin_id, tool_name, &input).await?;
        let as_text = String::from_utf8_lossy(&output).into_owned();
        match serde_json::from_str::<Value>(&as_text) {
            Ok(value) => Ok(value),
            Err(_) => Ok(Value::String(as_text)),
        }
    }

    async fn call_native(
        &self,
        lib_path: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, EngineError> {
        let native_runtime = self
            .native_runtime
            .as_ref()
            .ok_or_else(|| EngineError::ToolNotLoaded(tool_name.to_string()))?;
        let mut runtime = native_runtime.lock().await;
        runtime.call_registered_tool(lib_path, tool_name, args)
    }

    async fn call_mcp(
        &self,
        server_name: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, EngineError> {
        let spawner = self
            .mcp_spawner
            .as_ref()
            .ok_or_else(|| EngineError::ToolNotLoaded(tool_name.to_string()))?;
        spawner.call_tool(server_name, tool_name, args).await
    }

    fn scrub_output(&self, value: Value) -> Result<Value, EngineError> {
        let detector =
            InjectionDetector::new().map_err(|error| EngineError::ToolError(error.to_string()))?;
        match value {
            Value::String(text) => Ok(Value::String(detector.sanitize(&text))),
            other => {
                let serialized = other.to_string();
                if detector.scan(&serialized).is_some() {
                    Ok(Value::String(detector.sanitize(&serialized)))
                } else {
                    Ok(other)
                }
            }
        }
    }
}

fn operation_source_from_task_source(source: &TaskSource) -> OperationSource {
    match source {
        TaskSource::Cli => OperationSource::Local,
        TaskSource::Telegram(_) | TaskSource::WebUI | TaskSource::Remote(_) => {
            OperationSource::Remote
        }
    }
}
