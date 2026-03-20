use anyhow::{anyhow, Context, Result};
use sdk::{Complexity, SubagentRole, SubagentSpec, TaskDomain, TaskSource};
use sha2::Digest;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::builtin_tools::ToolRegistry;
use crate::conductor::{MemorySystem, RoutePolicy};
use crate::gateway::WorkspaceLocks;
use crate::llm::{LLMResponse, Message, ToolCall};
use crate::llm::router::LLMRouter;
use crate::security::secrets::scrub_text;
use crate::storage::TaskRepository;
use sdk::errors::EngineError;

const MIN_MEMORY_BUDGET: usize = 256;

#[derive(Debug, Clone)]
pub struct SubagentOutput {
    pub task_id: String,
    pub role: SubagentRole,
    pub output: String,
    pub steps_taken: u32,
    pub tool_calls: Vec<String>,
    pub provider_used: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SubagentResult {
    Completed(SubagentOutput),
    TimedOut(SubagentOutput),
    Failed(SubagentOutput),
}

impl SubagentResult {
    pub fn output(&self) -> &SubagentOutput {
        match self {
            Self::Completed(output) | Self::TimedOut(output) | Self::Failed(output) => output,
        }
    }
}

#[derive(Clone)]
pub struct SubagentRunner {
    spec: SubagentSpec,
    parent_task_id: Uuid,
    domain: TaskDomain,
    complexity: Complexity,
    route_policy: RoutePolicy,
    sensitive: bool,
    source: TaskSource,
    dependency_context: String,
    expected_outcome: String,
    router: Arc<LLMRouter>,
    task_repo: Arc<TaskRepository>,
    tools: Arc<ToolRegistry>,
    memory_system: Option<Arc<MemorySystem>>,
    workspace_locks: Arc<WorkspaceLocks>,
    steering_after_write_commands: Vec<String>,
}

#[derive(Debug, Default)]
struct SubagentProgress {
    steps_taken: u32,
    tool_calls: Vec<String>,
    last_output: Option<String>,
    provider_used: Option<String>,
}

impl SubagentRunner {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mut spec: SubagentSpec,
        parent_task_id: Uuid,
        domain: TaskDomain,
        complexity: Complexity,
        route_policy: RoutePolicy,
        sensitive: bool,
        source: TaskSource,
        dependency_context: impl Into<String>,
        expected_outcome: impl Into<String>,
        router: Arc<LLMRouter>,
        task_repo: Arc<TaskRepository>,
        tools: Arc<ToolRegistry>,
        memory_system: Option<Arc<MemorySystem>>,
        workspace_locks: Arc<WorkspaceLocks>,
        steering_after_write_commands: Vec<String>,
    ) -> Self {
        spec.memory_budget = spec.memory_budget.max(MIN_MEMORY_BUDGET);
        Self {
            spec,
            parent_task_id,
            domain,
            complexity,
            route_policy,
            sensitive,
            source,
            dependency_context: dependency_context.into(),
            expected_outcome: expected_outcome.into(),
            router,
            task_repo,
            tools,
            memory_system,
            workspace_locks,
            steering_after_write_commands,
        }
    }

    pub async fn run(self) -> SubagentResult {
        let subtask_id = Uuid::new_v4();
        let progress = Arc::new(Mutex::new(SubagentProgress::default()));
        let timeout_secs = self.spec.timeout_secs;
        let fallback = self.clone();

        let mut join = tokio::spawn({
            let progress = Arc::clone(&progress);
            async move { self.run_inner(subtask_id, progress).await }
        });

        match tokio::time::timeout(Duration::from_secs(timeout_secs), &mut join).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_error)) => {
                fallback.failure_from_snapshot(
                    subtask_id,
                    &progress,
                    format!("subagent crashed: {}", scrub_text(&join_error.to_string())),
                )
                .await
            }
            Err(_) => {
                join.abort();
                fallback.timeout_from_snapshot(subtask_id, &progress).await
            }
        }
    }

    async fn run_inner(
        self,
        subtask_id: Uuid,
        progress: Arc<Mutex<SubagentProgress>>,
    ) -> SubagentResult {
        if let Err(error) = self.task_repo.create_task(&subtask_id, &self.spec.task).await {
            return self.failure_output(
                subtask_id,
                &progress,
                format!("failed to create subagent task: {}", scrub_text(&error.to_string())),
            )
            .await;
        }
        if let Err(error) = self
            .task_repo
            .update_task_status(&subtask_id, crate::storage::TaskStatus::Running)
            .await
        {
            return self.failure_output(
                subtask_id,
                &progress,
                format!("failed to start subagent task: {}", scrub_text(&error.to_string())),
            )
            .await;
        }

        if let Err(error) = self
            .insert_event(
                &subtask_id,
                "subagent_started",
                serde_json::json!({
                    "role": self.spec.role.as_str(),
                    "task": scrub_text(&self.spec.task),
                    "memory_budget": self.spec.memory_budget,
                    "max_steps": self.spec.max_steps,
                    "timeout_secs": self.spec.timeout_secs,
                }),
                0,
            )
            .await
        {
            return self.failure_output(
                subtask_id,
                &progress,
                format!("failed to persist subagent start: {}", scrub_text(&error.to_string())),
            )
            .await;
        }

        let allowed_tools = self.allowed_tools();
        let mut messages = vec![
            Message::system(self.build_system_prompt(&allowed_tools).await),
            Message::user(self.spec.task.clone()),
        ];
        let mut tool_call_counts: HashMap<u64, u32> = HashMap::new();

        for step_index in 1..=self.spec.max_steps {
            if let Err(error) = progress_step(&progress, None, None).await {
                return self.failure_output(subtask_id, &progress, error.to_string()).await;
            }

            let (response, provider_name) = match self.call_model(&messages).await {
                Ok(result) => result,
                Err(error) => {
                    return self.failure_output(
                        subtask_id,
                        &progress,
                        scrub_text(&error.to_string()),
                    )
                    .await;
                }
            };
            record_provider(&progress, &provider_name).await;

            match response {
                LLMResponse::ToolCall(tool_call) => {
                    if let Err(error) = self
                        .handle_tool_call(
                            &subtask_id,
                            &tool_call,
                            &allowed_tools,
                            step_index as i64,
                            &mut tool_call_counts,
                            &mut messages,
                            &progress,
                        )
                        .await
                    {
                        return self.failure_output(
                            subtask_id,
                            &progress,
                            scrub_text(&error.to_string()),
                        )
                        .await;
                    }
                }
                LLMResponse::FinalAnswer(answer) => {
                    let answer = scrub_text(&answer.content);
                    if let Err(error) = self
                        .insert_event(
                            &subtask_id,
                            "answer",
                            serde_json::json!({ "answer": answer.clone() }),
                            step_index as i64,
                        )
                        .await
                    {
                        return self.failure_output(
                            subtask_id,
                            &progress,
                            scrub_text(&error.to_string()),
                        )
                        .await;
                    }
                    if let Err(error) = self
                        .task_repo
                        .complete_task(
                            &subtask_id,
                            provider_name.as_str(),
                            0,
                        )
                        .await
                    {
                        return self.failure_output(
                            subtask_id,
                            &progress,
                            scrub_text(&error.to_string()),
                        )
                        .await;
                    }
                    let output = output_from_progress(
                        &progress,
                        subtask_id,
                        self.spec.role.clone(),
                        answer,
                        None,
                    )
                    .await;
                    return SubagentResult::Completed(output);
                }
            }
        }

        self.failure_output(
            subtask_id,
            &progress,
            format!("subagent exceeded max_steps={}", self.spec.max_steps),
        )
        .await
    }

    async fn handle_tool_call(
        &self,
        subtask_id: &Uuid,
        tool_call: &ToolCall,
        allowed_tools: &HashSet<String>,
        step_num: i64,
        tool_call_counts: &mut HashMap<u64, u32>,
        messages: &mut Vec<Message>,
        progress: &Arc<Mutex<SubagentProgress>>,
    ) -> Result<()> {
        if !allowed_tools.contains(&tool_call.name) {
            return Err(EngineError::ToolNotPermitted(tool_call.name.clone()).into());
        }

        record_tool_call(tool_call_counts, tool_call)?;
        record_progress_tool(progress, &tool_call.name).await;
        self.insert_event(
            subtask_id,
            "tool_call",
            serde_json::json!({
                "tool_name": tool_call.name,
                "tool_args": scrub_text(&tool_call.arguments),
                "tool_id": tool_call.id,
            }),
            step_num,
        )
        .await?;

        let tool_args: serde_json::Value =
            serde_json::from_str(&tool_call.arguments).unwrap_or_else(|_| serde_json::json!({}));
        let tool_result = self
            .execute_tool_call(subtask_id, &tool_call.name, tool_args.clone())
            .await?;

        messages.push(Message::assistant(
            serde_json::json!({
                "function": &tool_call.name,
                "arguments": tool_args,
            })
            .to_string(),
        ));
        messages.push(Message::tool_result(&tool_result, &tool_call.id));
        self.insert_event(
            subtask_id,
            "observation",
            serde_json::json!({ "observation": tool_result.clone() }),
            step_num,
        )
        .await?;
        record_progress_output(progress, &tool_result).await;

        if tool_call.name == "write_file" {
            self.run_after_write_commands(
                subtask_id,
                allowed_tools,
                step_num,
                tool_call_counts,
                messages,
                progress,
            )
            .await?;
        }

        Ok(())
    }

    async fn run_after_write_commands(
        &self,
        subtask_id: &Uuid,
        allowed_tools: &HashSet<String>,
        step_num: i64,
        tool_call_counts: &mut HashMap<u64, u32>,
        messages: &mut Vec<Message>,
        progress: &Arc<Mutex<SubagentProgress>>,
    ) -> Result<()> {
        for command in &self.steering_after_write_commands {
            let scripted = ToolCall::new(
                format!("subagent-steering-{}-{}", subtask_id, step_num),
                "run_command",
                serde_json::json!({ "command": command }).to_string(),
            );
            if !allowed_tools.contains("run_command") {
                return Err(EngineError::ToolNotPermitted("run_command".to_string()).into());
            }
            record_tool_call(tool_call_counts, &scripted)?;
            record_progress_tool(progress, &scripted.name).await;
            self.insert_event(
                subtask_id,
                "tool_call",
                serde_json::json!({
                    "tool_name": scripted.name,
                    "tool_args": scrub_text(&scripted.arguments),
                    "tool_id": scripted.id,
                }),
                step_num,
            )
            .await?;

            let tool_args: serde_json::Value =
                serde_json::from_str(&scripted.arguments).unwrap_or_else(|_| serde_json::json!({}));
            let tool_result = self
                .execute_tool_call(subtask_id, &scripted.name, tool_args.clone())
                .await?;

            messages.push(Message::assistant(
                serde_json::json!({
                    "function": &scripted.name,
                    "arguments": tool_args,
                })
                .to_string(),
            ));
            messages.push(Message::tool_result(&tool_result, &scripted.id));
            self.insert_event(
                subtask_id,
                "observation",
                serde_json::json!({ "observation": tool_result.clone() }),
                step_num,
            )
            .await?;
            record_progress_output(progress, &tool_result).await;
        }

        Ok(())
    }

    async fn call_model(&self, messages: &[Message]) -> Result<(LLMResponse, String)> {
        if let Some(model_override) = self.spec.model_override.as_deref() {
            return self
                .router
                .call_named_provider(model_override, messages)
                .await
                .map_err(anyhow::Error::from);
        }

        match self.route_policy {
            RoutePolicy::LocalOnly => {
                if !self.router.has_local_model().await {
                    return Err(EngineError::RouteUnavailable(
                        "subagent step requires a local model".to_string(),
                    )
                    .into());
                }
                self.router
                    .call_local_only(messages)
                    .await
                    .map_err(anyhow::Error::from)
            }
            RoutePolicy::CloudOnly => self
                .router
                .call_cloud_only(messages)
                .await
                .map_err(anyhow::Error::from),
            RoutePolicy::LocalPreferred => self
                .router
                .call_local_preferred(messages, Some(self.sensitive))
                .await
                .map_err(anyhow::Error::from),
            RoutePolicy::Inherit => self
                .router
                .call_with_sensitivity(messages, Some(self.sensitive))
                .await
                .map_err(anyhow::Error::from),
        }
    }

    async fn execute_tool_call(
        &self,
        subtask_id: &Uuid,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String> {
        let result = if matches!(tool_name, "write_file" | "delete_file") {
            if let Some(workspace) = self.tools.workspace() {
                let lock = self.workspace_locks.get_lock(workspace);
                let _guard = lock.lock().await;
                self.tools
                    .call(tool_name, args.clone(), &subtask_id.to_string(), &self.source)
                    .await
            } else {
                self.tools
                    .call(tool_name, args.clone(), &subtask_id.to_string(), &self.source)
                    .await
            }
        } else {
            self.tools
                .call(tool_name, args.clone(), &subtask_id.to_string(), &self.source)
                .await
        };

        let result = match result {
            Ok(value) => stringify_tool_result(value),
            Err(error) => return Err(anyhow!(error)),
        };
        self.record_tool_audit(subtask_id, tool_name, &args, &result).await;
        Ok(result)
    }

    async fn record_tool_audit(
        &self,
        subtask_id: &Uuid,
        tool_name: &str,
        args: &serde_json::Value,
        safe_result: &str,
    ) {
        let args_hash = {
            let raw = serde_json::to_vec(args).unwrap_or_default();
            let mut hasher = sha2::Sha256::new();
            hasher.update(raw);
            hex::encode(hasher.finalize())
        };
        let result_summary = if safe_result.len() > 100 {
            format!("{}...", &safe_result[..97])
        } else {
            safe_result.to_string()
        };

        let _ = self
            .task_repo
            .insert_agent_action(
                &subtask_id.to_string(),
                "subagent_tool_execution",
                tool_name,
                &args_hash,
                0,
                "subagent",
                &scrub_text(&result_summary),
            )
            .await;
    }

    async fn build_system_prompt(&self, allowed_tools: &HashSet<String>) -> String {
        let tool_block = build_tool_prompt(self.tools.schemas_named(allowed_tools).await);
        let memory_block = self.shared_memory_context().await;
        format!(
            "You are a constrained Rove subagent.\nRole: {}\nDomain: {:?}\nComplexity: {:?}\nExpected outcome: {}\n\n{}\n{}\n\nDependency context:\n{}\n\nRules:\n- Only use the allowed tools listed above.\n- Stay within your role boundary.\n- Keep the answer concise and specific to the assigned sub-task.\n- If you cannot complete the task with allowed tools, explain the blocker plainly.",
            self.spec.role.as_str(),
            self.domain,
            self.complexity,
            self.expected_outcome,
            role_rules(&self.spec.role),
            tool_block,
            if memory_block.is_empty() {
                "(none)".to_string()
            } else {
                memory_block
            }
                + &format!("\n\n{}", self.dependency_context)
        )
    }

    async fn shared_memory_context(&self) -> String {
        let Some(memory_system) = &self.memory_system else {
            return String::new();
        };

        let target_tokens = self.spec.memory_budget.saturating_div(2).max(128);
        let mut used_tokens = 0usize;
        let mut lines = Vec::new();
        let hits = match memory_system.query(&self.spec.task, &self.domain, None).await {
            Ok(hits) => hits,
            Err(_) => return String::new(),
        };

        for hit in hits {
            let line = format!("- [{}] {}", hit.source, scrub_text(&hit.content));
            let estimate = estimate_tokens(&line);
            if used_tokens + estimate > target_tokens {
                break;
            }
            used_tokens += estimate;
            lines.push(line);
        }

        if lines.is_empty() {
            String::new()
        } else {
            format!("Shared memory:\n{}", lines.join("\n"))
        }
    }

    fn allowed_tools(&self) -> HashSet<String> {
        self.spec.tools_allowed.iter().cloned().collect()
    }

    async fn insert_event(
        &self,
        task_id: &Uuid,
        event_type: &str,
        payload: serde_json::Value,
        step_num: i64,
    ) -> Result<()> {
        self.task_repo
            .insert_agent_event_with_parent(
                task_id,
                Some(&self.parent_task_id),
                event_type,
                &payload.to_string(),
                step_num,
                Some(&self.domain.to_string()),
            )
            .await
            .map(|_| ())
            .context("failed to write subagent event")
    }

    async fn timeout_from_snapshot(
        &self,
        subtask_id: Uuid,
        progress: &Arc<Mutex<SubagentProgress>>,
    ) -> SubagentResult {
        let message = format!("subagent timed out after {}s", self.spec.timeout_secs);
        let _ = self
            .insert_event(
                &subtask_id,
                "error",
                serde_json::json!({ "error": message.clone() }),
                self.spec.max_steps as i64,
            )
            .await;
        let _ = self.task_repo.fail_task(&subtask_id).await;
        SubagentResult::TimedOut(
            output_from_progress(progress, subtask_id, self.spec.role.clone(), String::new(), Some(message)).await,
        )
    }

    async fn failure_from_snapshot(
        &self,
        subtask_id: Uuid,
        progress: &Arc<Mutex<SubagentProgress>>,
        error: String,
    ) -> SubagentResult {
        let _ = self
            .insert_event(
                &subtask_id,
                "error",
                serde_json::json!({ "error": error.clone() }),
                self.spec.max_steps as i64,
            )
            .await;
        let _ = self.task_repo.fail_task(&subtask_id).await;
        SubagentResult::Failed(
            output_from_progress(progress, subtask_id, self.spec.role.clone(), String::new(), Some(error)).await,
        )
    }

    async fn failure_output(
        &self,
        subtask_id: Uuid,
        progress: &Arc<Mutex<SubagentProgress>>,
        error: String,
    ) -> SubagentResult {
        self.failure_from_snapshot(subtask_id, progress, error).await
    }
}

fn build_tool_prompt(schemas: Vec<crate::builtin_tools::registry::ToolSchema>) -> String {
    if schemas.is_empty() {
        return "Allowed tools: none".to_string();
    }

    let mut lines = vec![
        "Allowed tools:".to_string(),
        "To call a tool, respond with JSON only: {\"function\":\"tool_name\",\"arguments\":{...}}".to_string(),
    ];
    for schema in schemas {
        lines.push(format!(
            "- {}: {} | parameters={}",
            schema.name, schema.description, schema.parameters
        ));
    }
    lines.join("\n")
}

fn role_rules(role: &SubagentRole) -> &'static str {
    match role {
        SubagentRole::Researcher => {
            "Researcher: stay read-only, gather evidence, do not mutate files or execute write operations."
        }
        SubagentRole::Executor => {
            "Executor: you may mutate the workspace only through explicitly allowed tools and only when needed."
        }
        SubagentRole::Verifier => {
            "Verifier: stay read-only, validate prior work, and report failures clearly."
        }
        SubagentRole::Summariser => {
            "Summariser: stay read-only and produce a structured, concise synthesis."
        }
        SubagentRole::Custom(_) => "Custom role: respect the allowed tool scope strictly.",
    }
}

fn record_tool_call(
    counts: &mut HashMap<u64, u32>,
    tool_call: &ToolCall,
) -> Result<()> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    tool_call.name.hash(&mut hasher);
    tool_call.arguments.hash(&mut hasher);
    let count = counts.entry(hasher.finish()).or_default();
    *count += 1;
    if *count >= 3 {
        return Err(anyhow!(
            "tool '{}' with identical arguments repeated 3 times",
            tool_call.name
        ));
    }
    Ok(())
}

fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn stringify_tool_result(result: serde_json::Value) -> String {
    match result {
        serde_json::Value::String(text) => text,
        other => other.to_string(),
    }
}

async fn progress_step(
    progress: &Arc<Mutex<SubagentProgress>>,
    output: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    let mut progress = progress.lock().await;
    progress.steps_taken += 1;
    if let Some(output) = output {
        progress.last_output = Some(scrub_text(output));
    }
    if let Some(provider) = provider {
        progress.provider_used = Some(provider.to_string());
    }
    Ok(())
}

async fn record_provider(progress: &Arc<Mutex<SubagentProgress>>, provider: &str) {
    let mut progress = progress.lock().await;
    progress.provider_used = Some(provider.to_string());
}

async fn record_progress_tool(progress: &Arc<Mutex<SubagentProgress>>, tool_name: &str) {
    let mut progress = progress.lock().await;
    progress.tool_calls.push(tool_name.to_string());
}

async fn record_progress_output(progress: &Arc<Mutex<SubagentProgress>>, output: &str) {
    let mut progress = progress.lock().await;
    progress.last_output = Some(scrub_text(output));
}

async fn output_from_progress(
    progress: &Arc<Mutex<SubagentProgress>>,
    subtask_id: Uuid,
    role: SubagentRole,
    fallback_output: String,
    error: Option<String>,
) -> SubagentOutput {
    let progress = progress.lock().await;
    SubagentOutput {
        task_id: subtask_id.to_string(),
        role,
        output: if fallback_output.is_empty() {
            progress.last_output.clone().unwrap_or_default()
        } else {
            fallback_output
        },
        steps_taken: progress.steps_taken,
        tool_calls: progress.tool_calls.clone(),
        provider_used: progress.provider_used.clone(),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_tools::{FilesystemTool, ToolRegistry};
    use crate::config::LLMConfig;
    use crate::gateway::WorkspaceLocks;
    use crate::llm::{FinalAnswer, LLMError, LLMProvider};
    use crate::storage::Database;
    use crate::llm::Message;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;
    use tempfile::TempDir;

    struct MockProvider {
        name: String,
        responses: StdMutex<VecDeque<Result<LLMResponse, LLMError>>>,
        is_local: bool,
    }

    #[async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        fn is_local(&self) -> bool {
            self.is_local
        }

        fn estimated_cost(&self, _tokens: usize) -> f64 {
            0.0
        }

        async fn generate(&self, _messages: &[Message]) -> crate::llm::Result<LLMResponse> {
            match self.responses.lock().unwrap().pop_front() {
                Some(result) => result,
                None => Ok(LLMResponse::FinalAnswer(FinalAnswer::new("done"))),
            }
        }
    }

    struct PanicProvider;

    #[async_trait]
    impl LLMProvider for PanicProvider {
        fn name(&self) -> &str {
            "panic-cloud"
        }

        fn is_local(&self) -> bool {
            false
        }

        fn estimated_cost(&self, _tokens: usize) -> f64 {
            1.0
        }

        async fn generate(&self, _messages: &[Message]) -> crate::llm::Result<LLMResponse> {
            panic!("forced panic for subagent test");
        }
    }

    struct SlowProvider;

    #[async_trait]
    impl LLMProvider for SlowProvider {
        fn name(&self) -> &str {
            "slow-cloud"
        }

        fn is_local(&self) -> bool {
            false
        }

        fn estimated_cost(&self, _tokens: usize) -> f64 {
            1.0
        }

        async fn generate(&self, _messages: &[Message]) -> crate::llm::Result<LLMResponse> {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok(LLMResponse::FinalAnswer(FinalAnswer::new("late answer")))
        }
    }

    async fn test_runner(
        providers: Vec<Box<dyn LLMProvider>>,
        spec: SubagentSpec,
    ) -> (TempDir, SubagentRunner, Arc<TaskRepository>) {
        let temp_dir = TempDir::new().unwrap();
        let database = Database::new(&temp_dir.path().join("subagent.db"))
            .await
            .unwrap();
        let repo = Arc::new(database.tasks());
        let config = Arc::new(LLMConfig {
            default_provider: "mock-cloud".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });
        let router = Arc::new(LLMRouter::new(providers, config));

        let mut registry = ToolRegistry::empty();
        registry
            .register_builtin_filesystem(FilesystemTool::new(temp_dir.path().to_path_buf()).unwrap())
            .await;
        let runner = SubagentRunner::new(
            spec,
            Uuid::new_v4(),
            TaskDomain::Code,
            Complexity::Complex,
            RoutePolicy::Inherit,
            false,
            TaskSource::Cli,
            "dependency context",
            "expected outcome",
            router,
            Arc::clone(&repo),
            Arc::new(registry),
            None,
            Arc::new(WorkspaceLocks::new()),
            Vec::new(),
        );

        (temp_dir, runner, repo)
    }

    #[tokio::test]
    async fn researcher_cannot_write_files() {
        let provider: Box<dyn LLMProvider> = Box::new(MockProvider {
            name: "mock-cloud".to_string(),
            responses: StdMutex::new(VecDeque::from(vec![Ok(LLMResponse::ToolCall(
                ToolCall::new(
                    "call-1",
                    "write_file",
                    serde_json::json!({"path":"note.txt","content":"hello"}).to_string(),
                ),
            ))])),
            is_local: false,
        });
        let spec = SubagentSpec {
            role: SubagentRole::Researcher,
            task: "inspect file".to_string(),
            tools_allowed: vec!["read_file".to_string(), "list_dir".to_string()],
            memory_budget: 800,
            model_override: None,
            max_steps: 2,
            timeout_secs: 5,
        };

        let (_temp_dir, runner, _repo) = test_runner(vec![provider], spec).await;
        let result = runner.run().await;
        match result {
            SubagentResult::Failed(output) => {
                assert!(output.error.unwrap().contains("Tool not permitted"));
            }
            other => panic!("expected failure, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn subagent_timeout_returns_partial_result() {
        let spec = SubagentSpec {
            role: SubagentRole::Summariser,
            task: "summarise slowly".to_string(),
            tools_allowed: Vec::new(),
            memory_budget: 800,
            model_override: None,
            max_steps: 2,
            timeout_secs: 1,
        };

        let (_temp_dir, runner, _repo) = test_runner(vec![Box::new(SlowProvider)], spec).await;
        let result = runner.run().await;
        match result {
            SubagentResult::TimedOut(output) => {
                assert!(output.steps_taken >= 1);
            }
            other => panic!("expected timeout, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn subagent_crash_does_not_crash_orchestrator() {
        let spec = SubagentSpec {
            role: SubagentRole::Verifier,
            task: "verify".to_string(),
            tools_allowed: Vec::new(),
            memory_budget: 800,
            model_override: None,
            max_steps: 2,
            timeout_secs: 5,
        };

        let (_temp_dir, runner, _repo) = test_runner(vec![Box::new(PanicProvider)], spec).await;
        let result = runner.run().await;
        match result {
            SubagentResult::Failed(output) => {
                assert!(output.error.unwrap().contains("subagent crashed"));
            }
            other => panic!("expected failure, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn parent_task_id_set_on_all_subagent_events() {
        let parent_task_id = Uuid::new_v4();
        let provider: Box<dyn LLMProvider> = Box::new(MockProvider {
            name: "mock-cloud".to_string(),
            responses: StdMutex::new(VecDeque::from(vec![Ok(LLMResponse::FinalAnswer(
                FinalAnswer::new("done"),
            ))])),
            is_local: false,
        });
        let spec = SubagentSpec {
            role: SubagentRole::Summariser,
            task: "summarise".to_string(),
            tools_allowed: Vec::new(),
            memory_budget: 800,
            model_override: None,
            max_steps: 2,
            timeout_secs: 5,
        };

        let temp_dir = TempDir::new().unwrap();
        let database = Database::new(&temp_dir.path().join("subagent-events.db"))
            .await
            .unwrap();
        let repo = Arc::new(database.tasks());
        let config = Arc::new(LLMConfig {
            default_provider: "mock-cloud".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });
        let router = Arc::new(LLMRouter::new(vec![provider], config));
        let runner = SubagentRunner::new(
            spec,
            parent_task_id,
            TaskDomain::General,
            Complexity::Complex,
            RoutePolicy::Inherit,
            false,
            TaskSource::Cli,
            "context",
            "done",
            router,
            Arc::clone(&repo),
            Arc::new(ToolRegistry::empty()),
            None,
            Arc::new(WorkspaceLocks::new()),
            Vec::new(),
        );

        let result = runner.run().await;
        let task_id = result.output().task_id.clone();
        let events = repo.get_agent_events_by_parent(&parent_task_id.to_string()).await.unwrap();
        assert!(events.iter().all(|event| event.parent_task_id.as_deref() == Some(parent_task_id.to_string().as_str())));
        assert!(events.iter().any(|event| event.task_id == task_id));
    }

    #[tokio::test]
    async fn local_only_route_fails_without_local_model() {
        let spec = SubagentSpec {
            role: SubagentRole::Verifier,
            task: "verify locally".to_string(),
            tools_allowed: Vec::new(),
            memory_budget: 800,
            model_override: None,
            max_steps: 2,
            timeout_secs: 5,
        };

        let (_temp_dir, mut runner, _repo) = test_runner(Vec::new(), spec).await;
        runner.route_policy = RoutePolicy::LocalOnly;

        let result = runner.run().await;
        match result {
            SubagentResult::Failed(output) => {
                assert!(output
                    .error
                    .unwrap()
                    .contains("requires a local model"));
            }
            other => panic!("expected local-only failure, got {:?}", other),
        }
    }
}
