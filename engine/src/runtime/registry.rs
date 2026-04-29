use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::config::Config;
use crate::hooks::{
    task_source_label, AfterToolCallPayload, BeforeAgentStartPayload, BeforeToolCallPayload,
    HookManager, MessageReceivedPayload, MessageSendingPayload, TextHookOutcome,
};
use sdk::brain::Brain;
use sdk::browser::BrowserBackend;

use crate::runtime::{
    FilesystemTool, McpSpawner, NativeRuntime, TerminalTool, VisionTool, WasmRuntime,
};
use crate::security::approvals;
use crate::security::command_executor::CommandExecutor;
use crate::security::injection_detector::InjectionDetector;
use crate::security::risk_assessor::{
    classify_terminal_command, Operation, OperationSource, RiskAssessor, RiskTier,
};
use crate::security::secrets::scrub_text;
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

#[derive(Debug, Clone, Copy)]
enum BrainBackendSource {
    Plugin,
}

impl BrainBackendSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
        }
    }
}

/// Registry of available tools across all execution surfaces.
pub struct ToolRegistry {
    pub fs: Option<FilesystemTool>,
    pub terminal: Option<TerminalTool>,
    pub vision: Option<VisionTool>,
    pub web_fetch_enabled: bool,
    pub web_search_enabled: bool,
    /// Browser backend — `None` when `browser.enabled` is false or no profile is configured.
    /// Accepts any type implementing `BrowserBackend` (e.g. CdpBackend, BrowshBackend).
    pub browser: Option<Arc<Mutex<dyn BrowserBackend>>>,
    /// Brain backend registered by an installed brain plugin.
    brain: Option<Arc<dyn Brain>>,
    brain_source: Option<BrainBackendSource>,
    pub wasm_runtime: Option<Arc<Mutex<WasmRuntime>>>,
    pub wasm_tools: Vec<WasmToolInfo>,
    pub mcp_spawner: Option<Arc<McpSpawner>>,
    pub mcp_tools: Vec<McpToolInfo>,
    native_runtime: Option<Arc<Mutex<NativeRuntime>>>,
    tools: Arc<RwLock<HashMap<String, ToolSchema>>>,
    risk_assessor: RiskAssessor,
    config: Arc<Config>,
    hooks: Arc<HookManager>,
}

impl ToolRegistry {
    pub fn empty() -> Self {
        Self::empty_with_config(Arc::new(Config::default()))
    }

    pub fn empty_with_config(config: Arc<Config>) -> Self {
        Self {
            fs: None,
            terminal: None,
            vision: None,
            web_fetch_enabled: false,
            web_search_enabled: false,
            browser: None,
            brain: None,
            brain_source: None,
            wasm_runtime: None,
            wasm_tools: Vec::new(),
            mcp_spawner: None,
            mcp_tools: Vec::new(),
            native_runtime: None,
            tools: Arc::new(RwLock::new(HashMap::new())),
            risk_assessor: RiskAssessor::new(),
            config,
            hooks: Arc::new(HookManager::disabled()),
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
            web_fetch_enabled: false,
            web_search_enabled: false,
            browser: None,
            brain: None,
            brain_source: None,
            wasm_runtime: wasm,
            wasm_tools: Vec::new(),
            mcp_spawner: mcp,
            mcp_tools: Vec::new(),
            native_runtime: native,
            tools: Arc::new(RwLock::new(HashMap::new())),
            risk_assessor: RiskAssessor::new(),
            hooks: Arc::new(HookManager::discover(&config)),
            config,
        }
    }

    pub fn workspace(&self) -> Option<&Path> {
        self.fs.as_ref().map(|fs| fs.workspace())
    }

    pub(crate) fn search_config(&self) -> &crate::config::SearchConfig {
        &self.config.search
    }

    pub async fn before_agent_start(
        &self,
        payload: BeforeAgentStartPayload,
    ) -> Result<TextHookOutcome, EngineError> {
        self.hooks.before_agent_start(payload).await
    }

    pub async fn message_received(
        &self,
        payload: MessageReceivedPayload,
    ) -> Result<TextHookOutcome, EngineError> {
        self.hooks.message_received(payload).await
    }

    pub async fn message_sending(&self, payload: MessageSendingPayload) {
        self.hooks.message_sending(payload).await;
    }

    pub async fn register(&self, schema: ToolSchema) {
        let mut tools = self.tools.write().await;
        if let Some(existing) = tools.get(&schema.name) {
            if matches!(existing.source, ToolSource::Builtin) {
                return;
            }
        }
        tools.insert(schema.name.clone(), schema);
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
        self.register(ToolSchema {
            name: "glob_files".to_string(),
            description: "Find files matching a glob pattern within the workspace.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Glob such as src/**/*.rs"},"path":{"type":"string","description":"Optional directory to search under"},"max_results":{"type":"integer","description":"Maximum number of paths to return"}},"required":["pattern"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "search".to_string(), "read".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "grep_files".to_string(),
            description: "Search file contents with a regex and return matching lines.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string","description":"Regex to search for"},"path":{"type":"string","description":"Optional directory or file to search under"},"file_pattern":{"type":"string","description":"Optional glob to limit searched files"},"max_results":{"type":"integer","description":"Maximum number of matches to return"}},"required":["pattern"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "search".to_string(), "read".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "append_to_file".to_string(),
            description: "Append text to the end of a file. Creates the file if it does not exist.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "write".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "patch_file".to_string(),
            description: "Edit a file by replacing an exact string with a new string. old_string must appear exactly once in the file. Use read_file first to see the current content.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"old_string":{"type":"string","description":"Exact text to find (must match exactly once)"},"new_string":{"type":"string","description":"Text to replace it with"}},"required":["path","old_string","new_string"]}),
            source: ToolSource::Builtin,
            domains: vec!["filesystem".to_string(), "write".to_string(), "edit".to_string(), "all".to_string()],
        })
        .await;
    }

    pub async fn register_builtin_web_fetch(&mut self) {
        self.web_fetch_enabled = true;
        self.register(ToolSchema {
            name: "web_fetch".to_string(),
            description: "Fetch a web page or text resource and return readable text.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"url":{"type":"string","description":"HTTP or HTTPS URL to fetch"},"max_chars":{"type":"integer","description":"Optional maximum number of characters to return"}},"required":["url"]}),
            source: ToolSource::Builtin,
            domains: vec!["web".to_string(), "research".to_string(), "read".to_string(), "all".to_string()],
        })
        .await;
    }

    pub async fn register_builtin_web_search(&mut self) {
        self.web_search_enabled = true;
        self.register(ToolSchema {
            name: "web_search".to_string(),
            description: "Search the web and return compact ranked results without fetching full pages.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"integer","description":"Maximum number of results to return"},"provider":{"type":"string","description":"Optional provider override, e.g. searxng"}},"required":["query"]}),
            source: ToolSource::Builtin,
            domains: vec!["web".to_string(), "research".to_string(), "search".to_string(), "all".to_string()],
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

    pub async fn browser_runtime_status(&self) -> sdk::BrowserRuntimeStatus {
        match &self.browser {
            Some(backend) => {
                let guard = backend.lock().await;
                sdk::BrowserRuntimeStatus {
                    registered: true,
                    connected: guard.is_connected(),
                    backend_name: Some(guard.backend_name().to_string()),
                    source: None,
                    warnings: Vec::new(),
                }
            }
            None => sdk::BrowserRuntimeStatus::default(),
        }
    }

    pub async fn register_browser_backend(&mut self, tool: Arc<Mutex<dyn BrowserBackend>>) {
        self.browser = Some(tool);
        self.register(ToolSchema {
            name: "browse_url".to_string(),
            description: "Navigate the browser to a URL and return the page title.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"url":{"type":"string","description":"The URL to navigate to"}},"required":["url"]}),
            source: ToolSource::Builtin,
            domains: vec!["browser".to_string(), "web".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "read_page_text".to_string(),
            description: "Get the visible text content of the current browser page.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
            source: ToolSource::Builtin,
            domains: vec!["browser".to_string(), "web".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "click_element".to_string(),
            description: "Click the first DOM element matching a CSS selector on the current page.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"selector":{"type":"string","description":"CSS selector of the element to click"}},"required":["selector"]}),
            source: ToolSource::Builtin,
            domains: vec!["browser".to_string(), "web".to_string(), "all".to_string()],
        })
        .await;
        self.register(ToolSchema {
            name: "fill_form_field".to_string(),
            description: "Set the value of an input or textarea matching a CSS selector.".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"selector":{"type":"string","description":"CSS selector of the form field"},"value":{"type":"string","description":"Value to fill in"}},"required":["selector","value"]}),
            source: ToolSource::Builtin,
            domains: vec!["browser".to_string(), "web".to_string(), "form".to_string(), "all".to_string()],
        })
        .await;
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

    pub fn register_plugin_brain_backend(&mut self, brain: Arc<dyn Brain>) {
        self.brain = Some(brain);
        self.brain_source = Some(BrainBackendSource::Plugin);
    }

    pub fn plugin_brain(&self) -> Option<Arc<dyn Brain>> {
        self.brain.clone()
    }

    pub fn brain_runtime_status(&self) -> serde_json::Value {
        match &self.brain {
            Some(brain) => serde_json::json!({
                "active": true,
                "backend_name": brain.name(),
                "source": self.brain_source.map(|s| s.as_str()),
            }),
            None => serde_json::json!({ "active": false }),
        }
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
        let name = name.into();
        if self
            .tools
            .read()
            .await
            .get(&name)
            .is_some_and(|existing| matches!(existing.source, ToolSource::Builtin))
        {
            debug!(
                "Skipping native registration for '{}' because a builtin tool with that name already exists",
                name
            );
            return;
        }

        self.register(ToolSchema {
            name,
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

    pub async fn schemas_named(&self, allowed: &HashSet<String>) -> Vec<ToolSchema> {
        let mut schemas = self
            .tools
            .read()
            .await
            .values()
            .filter(|schema| allowed.contains(&schema.name))
            .cloned()
            .collect::<Vec<_>>();
        schemas.sort_by(|left, right| left.name.cmp(&right.name));
        schemas
    }

    pub async fn all_schemas(&self) -> Vec<ToolSchema> {
        let mut schemas = self
            .tools
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        schemas.sort_by(|left, right| left.name.cmp(&right.name));
        schemas
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

        let hook_outcome = self
            .hooks
            .before_tool_call(BeforeToolCallPayload {
                event: "BeforeToolCall",
                task_id: task_id.to_string(),
                tool_name: name.to_string(),
                args,
                task_source: task_source_label(source),
                workspace: self.config.core.workspace.display().to_string(),
            })
            .await?;
        let args = hook_outcome.args;

        self.check_paths(&args)?;
        self.check_command_gate(name, &args)?;
        self.assess_risk(task_id, name, &args, source).await?;

        let args_for_after = args.clone();
        let raw_result = match &schema.source {
            ToolSource::Builtin => self.call_builtin(name, args).await?,
            ToolSource::Native { lib_path } => self.call_native(lib_path, name, args).await?,
            ToolSource::Wasm { plugin_id } => self.call_wasm(plugin_id, name, args).await?,
            ToolSource::Mcp { server_name } => {
                let remote_name = self.remote_mcp_tool_name(server_name, name);
                self.call_mcp(server_name, &remote_name, args).await?
            }
        };

        let safe_result = self.scrub_output(raw_result)?;
        self.hooks
            .after_tool_call(AfterToolCallPayload {
                event: "AfterToolCall",
                task_id: task_id.to_string(),
                tool_name: name.to_string(),
                args: args_for_after,
                result: safe_result.clone(),
                task_source: task_source_label(source),
                workspace: self.config.core.workspace.display().to_string(),
            })
            .await;

        Ok(safe_result)
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
        let risk_tier = match tier {
            RiskTier::Tier0 => 0,
            RiskTier::Tier1 => 1,
            RiskTier::Tier2 => 2,
        };

        match approvals::evaluate(&self.config, tool_name, args, source, risk_tier)
            .map_err(|error| EngineError::Config(error.to_string()))?
        {
            approvals::ApprovalDecision::AutoAllow { reason } => {
                tracing::info!(
                    task_id = %task_id,
                    tool_name = %tool_name,
                    reason = %reason,
                    "Approval engine auto-allowed tool invocation"
                );
                return Ok(());
            }
            approvals::ApprovalDecision::RequireApproval { reason } => {
                if let Some(reason) = reason.filter(|value| !value.trim().is_empty()) {
                    tracing::info!(
                        task_id = %task_id,
                        tool_name = %tool_name,
                        reason = %reason,
                        "Approval engine requires explicit approval"
                    );
                }
            }
        }

        match tier {
            RiskTier::Tier0 => Ok(()),
            RiskTier::Tier1 => self.confirm_tier1(task_id, tool_name, source).await,
            RiskTier::Tier2 => self.confirm_tier2(task_id, tool_name, source).await,
        }
    }

    async fn confirm_tier1(
        &self,
        task_id: &str,
        tool_name: &str,
        source: &TaskSource,
    ) -> Result<(), EngineError> {
        if !self.config.security.confirm_tier1 || self.config.security.max_risk_tier < 1 {
            return Ok(());
        }

        if should_use_daemon_approval(source)
            || matches!(
                self.config.daemon.profile,
                crate::config::DaemonProfile::Headless | crate::config::DaemonProfile::Edge
            )
        {
            let approved = approvals::request_approval(
                task_id,
                tool_name,
                1,
                format!(
                    "Approve Tier 1 operation `{tool_name}`. It will auto-approve after {} seconds.",
                    self.config.security.confirm_tier1_delay
                ),
                Some(Duration::from_secs(self.config.security.confirm_tier1_delay)),
                true,
            )
            .await;
            return if approved {
                Ok(())
            } else {
                Err(EngineError::OperationAbortedByUser)
            };
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

    async fn confirm_tier2(
        &self,
        task_id: &str,
        tool_name: &str,
        source: &TaskSource,
    ) -> Result<(), EngineError> {
        if self.config.security.max_risk_tier < 2 {
            return Err(EngineError::OperationAbortedByUser);
        }
        if !self.config.security.require_explicit_tier2 {
            return Ok(());
        }

        if should_use_daemon_approval(source)
            || matches!(
                self.config.daemon.profile,
                crate::config::DaemonProfile::Headless | crate::config::DaemonProfile::Edge
            )
        {
            let approved = approvals::request_approval(
                task_id,
                tool_name,
                2,
                format!("Approve Tier 2 operation `{tool_name}`. Explicit approval is required."),
                None,
                false,
            )
            .await;
            return if approved {
                Ok(())
            } else {
                Err(EngineError::OperationAbortedByUser)
            };
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
            "read_file" | "list_dir" | "file_exists" | "glob_files" | "grep_files"
            | "web_fetch" | "web_search" => "read_file",
            "write_file" | "append_to_file" | "patch_file" => "write_file",
            "delete_file" => "delete_file",
            "run_command" => args
                .get("command")
                .and_then(Value::as_str)
                .map(classify_terminal_command)
                .unwrap_or("execute_command"),
            "capture_screen" | "browse_url" | "read_page_text" => "read_file",
            "click_element" | "fill_form_field" => "execute_task",
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
            "web_fetch" => {
                let url = args.get("url").and_then(Value::as_str).unwrap_or_default();
                if url.is_empty() {
                    return Err(EngineError::ToolError(
                        "web_fetch requires a non-empty url".to_string(),
                    ));
                }
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(EngineError::ToolError(
                        "web_fetch only supports http:// and https:// URLs".to_string(),
                    ));
                }
                let max_chars = args
                    .get("max_chars")
                    .and_then(Value::as_u64)
                    .unwrap_or(20_000) as usize;
                let result = crate::system::knowledge::fetch_url_text(url, max_chars)
                    .await
                    .map_err(|error| EngineError::ToolError(error.to_string()))?;
                serde_json::to_value(result)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "web_search" => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
                let provider = args.get("provider").and_then(Value::as_str);
                let result = crate::system::web_search::search_web(
                    &self.config.search,
                    query,
                    limit,
                    provider,
                )
                .await
                .map_err(|error| EngineError::ToolError(error.to_string()))?;
                serde_json::to_value(result)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
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
            "append_to_file" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                fs.append_to_file(path, content)
                    .await
                    .map(Value::String)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "patch_file" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
                let old = args
                    .get("old_string")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let new = args
                    .get("new_string")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                fs.patch_file(path, old, new)
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
            "glob_files" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let pattern = args
                    .get("pattern")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let path = args.get("path").and_then(Value::as_str);
                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(200) as usize;
                fs.glob_files(pattern, path, max_results)
                    .await
                    .map(Value::String)
                    .map_err(|error| EngineError::ToolError(error.to_string()))
            }
            "grep_files" => {
                let fs = self
                    .fs
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound(name.to_string()))?;
                let pattern = args
                    .get("pattern")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let path = args.get("path").and_then(Value::as_str);
                let file_pattern = args.get("file_pattern").and_then(Value::as_str);
                let max_results = args
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .unwrap_or(100) as usize;
                fs.grep_files(pattern, path, file_pattern, max_results)
                    .await
                    .map(Value::String)
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
            "browse_url" => {
                let browser = self
                    .browser
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound("browse_url".to_string()))?;
                let url = args.get("url").and_then(Value::as_str).unwrap_or_default();
                browser.lock().await.navigate(url).await.map(Value::String)
            }
            "read_page_text" => {
                let browser = self
                    .browser
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound("read_page_text".to_string()))?;
                browser.lock().await.page_text().await.map(Value::String)
            }
            "click_element" => {
                let browser = self
                    .browser
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound("click_element".to_string()))?;
                let selector = args
                    .get("selector")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                browser
                    .lock()
                    .await
                    .click(selector)
                    .await
                    .map(Value::String)
            }
            "fill_form_field" => {
                let browser = self
                    .browser
                    .as_ref()
                    .ok_or_else(|| EngineError::ToolNotFound("fill_form_field".to_string()))?;
                let selector = args
                    .get("selector")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let value = args
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                browser
                    .lock()
                    .await
                    .fill_field(selector, value)
                    .await
                    .map(Value::String)
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
            Value::String(text) => Ok(Value::String(scrub_text(&detector.sanitize(&text)))),
            other => {
                let serialized = other.to_string();
                if detector.scan(&serialized).is_some() {
                    Ok(Value::String(scrub_text(&detector.sanitize(&serialized))))
                } else {
                    let scrubbed = scrub_text(&serialized);
                    if scrubbed != serialized {
                        Ok(Value::String(scrubbed))
                    } else {
                        Ok(other)
                    }
                }
            }
        }
    }
}

fn should_use_daemon_approval(source: &TaskSource) -> bool {
    // CLI tasks always use the terminal/stdout path so the user sees the prompt.
    // Non-terminal stdin (piped) auto-approves on EOF — still visible on stdout.
    !matches!(source, TaskSource::Cli)
}

fn operation_source_from_task_source(source: &TaskSource) -> OperationSource {
    match source {
        TaskSource::Cli | TaskSource::Subagent(_) => OperationSource::Local,
        TaskSource::Telegram(_)
        | TaskSource::Channel(_)
        | TaskSource::WebUI
        | TaskSource::Remote(_) => OperationSource::Remote,
    }
}
