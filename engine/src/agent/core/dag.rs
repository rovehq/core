use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use sdk::{Complexity, Route, SubagentSpec, TaskDomain, TaskSource};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;
use uuid::Uuid;

use crate::agent::{SubagentResult, SubagentRunner};
use crate::builtin_tools::registry::{ToolSchema, ToolSource};
use crate::conductor::{
    DagNodeExecution, DagNodeExecutor, DagRoutingPolicy, DagRunReport, DagRunner,
    DagSchedulingPolicy, HybridExecutor, PlanStep, StepRole,
};
use crate::gateway::Task;
use crate::llm::{Message, MessageRole};
use crate::remote::{RemoteManager, RemoteSendResult, RemoteTaskEvent};
use crate::security::secrets::scrub_text;
use crate::system::worker_presets;

use super::orchestration::OrchestrationDecision;
use super::prompt::TaskContext;
use super::{AgentCore, TaskResult};

struct AgentDagExecutor {
    router: Arc<crate::llm::router::LLMRouter>,
    task_repo: Arc<crate::storage::TaskRepository>,
    tools: Arc<crate::builtin_tools::ToolRegistry>,
    memory_system: Option<Arc<crate::conductor::MemorySystem>>,
    workspace_locks: Arc<crate::gateway::WorkspaceLocks>,
    parent_task_id: Uuid,
    source: TaskSource,
    domain: TaskDomain,
    complexity: Complexity,
    sensitive: bool,
    policy_after_write_commands: Vec<String>,
    config: Arc<crate::config::Config>,
}

#[async_trait]
impl DagNodeExecutor for AgentDagExecutor {
    async fn execute_node(
        &self,
        step: &PlanStep,
        dependency_context: &str,
        route: Route,
    ) -> Result<DagNodeExecution> {
        if let Some(remote_execution) = self
            .try_remote_execute(step, dependency_context, route)
            .await?
        {
            return Ok(remote_execution);
        }

        let spec = self.subagent_spec_for_step(step).await;
        let runner = SubagentRunner::new(
            spec,
            self.parent_task_id,
            self.domain,
            self.complexity,
            step.route_policy.clone(),
            self.sensitive,
            self.source.clone(),
            dependency_context.to_string(),
            step.expected_outcome.clone(),
            Arc::clone(&self.router),
            Arc::clone(&self.task_repo),
            Arc::clone(&self.tools),
            self.memory_system.clone(),
            Arc::clone(&self.workspace_locks),
            if matches!(step.role, StepRole::Executor) {
                self.policy_after_write_commands.clone()
            } else {
                Vec::new()
            },
        );

        match tokio::spawn(async move { runner.run().await })
            .await
            .context("subagent join failed")?
        {
            SubagentResult::Completed(output) => Ok(DagNodeExecution {
                step_id: step.id.clone(),
                output: output.output,
                route: output
                    .provider_used
                    .as_deref()
                    .map(provider_route)
                    .unwrap_or(route),
                duration_ms: 0,
                role: step.role.clone(),
            }),
            SubagentResult::TimedOut(output) => Err(anyhow!(
                "subagent timed out for step {}: {}",
                step.id,
                output.error.unwrap_or_else(|| "timeout".to_string())
            )),
            SubagentResult::Failed(output) => Err(anyhow!(
                "subagent failed for step {}: {}",
                step.id,
                output
                    .error
                    .unwrap_or_else(|| "unknown failure".to_string())
            )),
        }
    }
}

impl AgentDagExecutor {
    async fn subagent_spec_for_step(&self, step: &PlanStep) -> SubagentSpec {
        let tools_allowed = tools_allowed_for_step(step, self.domain, &self.tools).await;
        let preset_id = worker_preset_for_step_role(&step.role);
        let mut spec = worker_presets::subagent_spec_for_preset(
            preset_id,
            step.description.clone(),
            tools_allowed,
        )
        .expect("built-in DAG step role must map to a built-in worker preset");

        match self.complexity {
            Complexity::Simple => {}
            Complexity::Medium => spec.memory_budget += 100,
            Complexity::Complex => spec.memory_budget += 250,
        }

        spec
    }

    async fn try_remote_execute(
        &self,
        step: &PlanStep,
        dependency_context: &str,
        route: Route,
    ) -> Result<Option<DagNodeExecution>> {
        if !self.config.ws_client.enabled || matches!(self.source, TaskSource::Remote(_)) {
            return Ok(None);
        }

        let prompt = build_remote_step_prompt(step, dependency_context);
        let manager = RemoteManager::new(self.config.as_ref().clone());
        let execution_plan = if matches!(step.role, StepRole::Executor) {
            manager
                .plan_execution_bundle(&prompt, &self.policy_after_write_commands)
                .await?
        } else {
            None
        };
        let result = match manager
            .send_with_options(&prompt, remote_send_options_for_step(step, execution_plan))
            .await
        {
            Ok(result) => result,
            Err(error) => {
                debug!(
                    step_id = %step.id,
                    error = %scrub_text(&error.to_string()),
                    "Remote delegation unavailable for DAG step; falling back to local execution"
                );
                return Ok(None);
            }
        };

        self.persist_remote_step_events(step, &result).await?;

        let output = result
            .answer
            .clone()
            .or(result.message.clone())
            .unwrap_or_else(|| "remote step completed".to_string());
        let resolved_route = match result.provider.as_deref() {
            Some("executor-plan") | None => route,
            Some(provider) => provider_route(provider),
        };

        Ok(Some(DagNodeExecution {
            step_id: step.id.clone(),
            output,
            route: resolved_route,
            duration_ms: result.duration_ms.unwrap_or_default() as u64,
            role: step.role.clone(),
        }))
    }

    async fn persist_remote_step_events(
        &self,
        step: &PlanStep,
        result: &RemoteSendResult,
    ) -> Result<()> {
        let domain = self.domain.to_string().to_ascii_lowercase();
        let base_step = 70_000_i64 + (step.order as i64 * 100);
        let delegate_payload = serde_json::json!({
            "step_id": step.id,
            "remote_task_id": result.remote_task_id,
            "target_node": result.envelope.target_node,
            "status": result.status,
            "summary": scrub_text(&step.description),
            "execution_plan": result.envelope.execution_plan.is_some(),
        })
        .to_string();
        self.task_repo
            .insert_agent_event(
                &self.parent_task_id,
                "remote_delegate",
                &delegate_payload,
                base_step,
                Some(&domain),
            )
            .await?;

        for (index, event) in result.events.iter().enumerate() {
            let payload = remote_event_payload(step, result, event);
            self.task_repo
                .insert_agent_event(
                    &self.parent_task_id,
                    "remote_event",
                    &payload,
                    base_step + 1 + index as i64,
                    Some(&domain),
                )
                .await?;
        }

        let result_payload = serde_json::json!({
            "step_id": step.id,
            "remote_task_id": result.remote_task_id,
            "target_node": result.envelope.target_node,
            "status": result.status,
            "provider": result.provider,
            "duration_ms": result.duration_ms,
            "answer": result.answer.as_ref().map(|value| scrub_text(value)),
            "message": result.message.as_ref().map(|value| scrub_text(value)),
        })
        .to_string();
        self.task_repo
            .insert_agent_event(
                &self.parent_task_id,
                "remote_result",
                &result_payload,
                base_step + 99,
                Some(&domain),
            )
            .await?;

        Ok(())
    }
}

impl AgentCore {
    pub(super) async fn execute_dag_task(
        &self,
        task_id: &Uuid,
        task: &Task,
        context: &TaskContext,
        orchestration: &OrchestrationDecision,
        start_time: Instant,
    ) -> Result<TaskResult> {
        let planner = HybridExecutor::new(self.router.clone(), self.router.local_brain());
        let planning_context = self.dag_planning_context(task, orchestration);
        let plan = planner
            .plan_with_cloud(&task.input, &planning_context)
            .await
            .context("Failed to build DAG plan for complex task")?;

        let mut graph = crate::conductor::DagGraph::from_plan(
            task_id.to_string(),
            &plan,
            context.domain,
            context.complexity,
            context.sensitive,
            context.route,
        );
        DagRoutingPolicy::new(self.router.local_brain().is_some()).assign_routes(&mut graph, &plan);

        let runner = DagRunner::with_persistence(
            self.task_repo.clone(),
            *task_id,
            context.domain_str.clone(),
        )
        .with_scheduling_policy(scheduling_policy_for(context, orchestration));
        let executor = AgentDagExecutor {
            router: self.router.clone(),
            task_repo: self.task_repo.clone(),
            tools: self.tools.clone(),
            memory_system: self.memory_system.clone(),
            workspace_locks: self.workspace_locks.clone(),
            parent_task_id: *task_id,
            source: task.source.clone(),
            domain: context.domain,
            complexity: context.complexity,
            sensitive: context.sensitive,
            policy_after_write_commands: self.policy_after_write_commands.clone(),
            config: self.config.clone(),
        };
        let report = runner
            .run(graph, &plan, &executor)
            .await
            .context("Failed to execute DAG task")?;

        if report.has_failures() {
            return Err(anyhow!(dag_failure_summary(&plan, &report)));
        }

        let answer = final_dag_answer(&plan, &report)?;
        let provider_used = summarize_routes(&report);
        let answer_payload = serde_json::json!({ "answer": scrub_text(&answer) }).to_string();
        self.task_repo
            .insert_agent_event(
                task_id,
                "answer",
                &answer_payload,
                50_000,
                Some(&context.domain_str),
            )
            .await
            .context("Failed to persist DAG final answer")?;

        Ok(TaskResult::success(
            task_id.to_string(),
            answer,
            provider_used,
            start_time.elapsed().as_millis() as i64,
            report.graph.waves.len().max(1),
            context.domain,
            context.sensitive,
        ))
    }

    fn dag_planning_context(&self, task: &Task, orchestration: &OrchestrationDecision) -> String {
        let mut lines = Vec::new();
        if let Some(workspace) = &task.workspace {
            lines.push(format!("Workspace: {}", workspace.display()));
        }
        lines.push(format!(
            "Orchestration strategy: {}",
            orchestration.summary()
        ));

        for message in self.memory.messages() {
            lines.push(format!(
                "[{}] {}",
                message_role(message),
                scrub_text(&message.content)
            ));
        }

        lines.join("\n\n")
    }
}

fn final_dag_answer(
    plan: &crate::conductor::ConductorPlan,
    report: &DagRunReport,
) -> Result<String> {
    let mut ordered_steps = plan.steps.iter().collect::<Vec<_>>();
    ordered_steps.sort_by_key(|step| step.order);

    for step in ordered_steps.iter().rev() {
        if step.role == StepRole::Verifier {
            if let Some(result) = report.results.get(&step.id) {
                return Ok(result.output.clone());
            }
        }
    }

    for step in ordered_steps.iter().rev() {
        if let Some(result) = report.results.get(&step.id) {
            return Ok(result.output.clone());
        }
    }

    Err(anyhow!("DAG execution produced no successful step output"))
}

fn summarize_routes(report: &DagRunReport) -> String {
    let mut labels = report
        .results
        .values()
        .map(|result| match result.route {
            Route::Local => "local",
            Route::Ollama => "ollama",
            Route::Cloud => "cloud",
        })
        .collect::<Vec<_>>();
    labels.sort_unstable();
    labels.dedup();

    format!("dag[{}]", labels.join(","))
}

fn dag_failure_summary(plan: &crate::conductor::ConductorPlan, report: &DagRunReport) -> String {
    let mut failures = Vec::new();

    for node in &report.graph.nodes {
        if !matches!(
            node.state,
            crate::conductor::DagNodeState::Failed | crate::conductor::DagNodeState::Blocked
        ) {
            continue;
        }

        let description = plan
            .steps
            .iter()
            .find(|step| step.id == node.step_id)
            .map(|step| step.description.as_str())
            .unwrap_or("unknown step");
        let reason = node.error.as_deref().unwrap_or("unknown error");
        failures.push(format!("{} ({description}): {reason}", node.step_id));
    }

    if failures.is_empty() {
        "DAG execution failed".to_string()
    } else {
        format!("DAG execution failed: {}", failures.join("; "))
    }
}

fn provider_route(provider_name: &str) -> Route {
    match provider_name {
        "local-brain" => Route::Local,
        "ollama" => Route::Ollama,
        _ => Route::Cloud,
    }
}

fn message_role(message: &Message) -> &'static str {
    match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Tool => "tool",
    }
}

fn build_remote_step_prompt(step: &PlanStep, dependency_context: &str) -> String {
    let role_prefix = match step.role {
        StepRole::Researcher => {
            "Remote researcher task. Stay read-only, gather facts, and return concise findings."
        }
        StepRole::Executor => {
            "Remote executor task. Perform the requested system/workspace action and summarize the result."
        }
        StepRole::Verifier => {
            "Remote verifier task. Validate the prior work, stay read-only, and explain pass/fail clearly."
        }
    };
    let mut parts = vec![role_prefix.to_string(), step.description.clone()];
    if !dependency_context.trim().is_empty() {
        parts.push(format!(
            "Dependency context:\n{}",
            dependency_context.trim()
        ));
    }
    if !step.expected_outcome.trim().is_empty() {
        parts.push(format!(
            "Expected outcome: {}",
            step.expected_outcome.trim()
        ));
    }
    parts.join("\n\n")
}

fn remote_send_options_for_step(
    step: &PlanStep,
    execution_plan: Option<sdk::RemoteExecutionPlan>,
) -> crate::remote::RemoteSendOptions {
    match step.role {
        StepRole::Researcher => crate::remote::RemoteSendOptions {
            node: Some("auto".to_string()),
            required_capabilities: vec!["remote-execution".to_string()],
            allow_executor_only: false,
            prefer_executor_only: false,
            execution_plan: None,
            ..crate::remote::RemoteSendOptions::default()
        },
        StepRole::Verifier => crate::remote::RemoteSendOptions {
            node: Some("auto".to_string()),
            required_capabilities: vec!["remote-execution".to_string()],
            allow_executor_only: false,
            prefer_executor_only: false,
            execution_plan: None,
            ..crate::remote::RemoteSendOptions::default()
        },
        StepRole::Executor => crate::remote::RemoteSendOptions {
            node: Some("auto".to_string()),
            required_capabilities: vec!["remote-execution".to_string()],
            allow_executor_only: true,
            prefer_executor_only: true,
            execution_plan,
            ..crate::remote::RemoteSendOptions::default()
        },
    }
}

fn remote_event_payload(
    step: &PlanStep,
    result: &RemoteSendResult,
    event: &RemoteTaskEvent,
) -> String {
    serde_json::json!({
        "step_id": step.id,
        "target_node": result.envelope.target_node,
        "remote_task_id": result.remote_task_id,
        "remote_event_type": event.event_type,
        "remote_step_num": event.step_num,
        "remote_domain": event.domain,
        "payload": scrub_text(&event.payload),
    })
    .to_string()
}

fn worker_preset_for_step_role(role: &StepRole) -> &'static str {
    match role {
        StepRole::Researcher => "researcher",
        StepRole::Executor => "executor",
        StepRole::Verifier => "verifier",
    }
}

fn scheduling_policy_for(
    context: &TaskContext,
    orchestration: &OrchestrationDecision,
) -> DagSchedulingPolicy {
    let mut policy = match (context.domain, context.complexity) {
        (TaskDomain::Browser | TaskDomain::Data, Complexity::Complex) => DagSchedulingPolicy {
            max_parallel_total: 4,
            max_parallel_researchers: 3,
            max_parallel_verifiers: 2,
            max_parallel_executors: 1,
        },
        (_, Complexity::Complex) => DagSchedulingPolicy {
            max_parallel_total: 3,
            max_parallel_researchers: 2,
            max_parallel_verifiers: 2,
            max_parallel_executors: 1,
        },
        _ => DagSchedulingPolicy {
            max_parallel_total: 2,
            max_parallel_researchers: 1,
            max_parallel_verifiers: 1,
            max_parallel_executors: 1,
        },
    };

    if orchestration.estimated_steps >= 4 {
        policy.max_parallel_total = policy.max_parallel_total.max(4);
        policy.max_parallel_researchers = policy.max_parallel_researchers.max(2);
    }

    if orchestration
        .reasons
        .iter()
        .any(|reason| reason == "post-write verification")
    {
        policy.max_parallel_executors = 1;
    }

    policy
}

async fn tools_allowed_for_step(
    step: &PlanStep,
    domain: TaskDomain,
    tools: &crate::builtin_tools::ToolRegistry,
) -> Vec<String> {
    select_role_tool_catalog(&step.role, domain, tools.all_schemas().await)
}

fn select_role_tool_catalog(
    role: &StepRole,
    domain: TaskDomain,
    schemas: Vec<ToolSchema>,
) -> Vec<String> {
    let allowed_domain_tags = allowed_domain_tags(role, domain);
    let mut names = schemas
        .into_iter()
        .filter(|schema| {
            schema.domains.is_empty()
                || schema
                    .domains
                    .iter()
                    .any(|tag| allowed_domain_tags.iter().any(|allowed| tag == allowed))
        })
        .filter(|schema| role_allows_schema(role, schema))
        .map(|schema| schema.name)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn allowed_domain_tags(role: &StepRole, domain: TaskDomain) -> Vec<&'static str> {
    let mut tags = vec![task_domain_tag(domain), "all", "filesystem"];
    match role {
        StepRole::Researcher => tags.extend(["read", "browser", "data", "vision"]),
        StepRole::Verifier => tags.extend(["read", "vision"]),
        StepRole::Executor => tags.extend(["read", "write", "shell", "git", "code", "vision"]),
    }
    tags
}

fn task_domain_tag(domain: TaskDomain) -> &'static str {
    match domain {
        TaskDomain::Code => "code",
        TaskDomain::Git => "git",
        TaskDomain::Shell => "shell",
        TaskDomain::Browser => "browser",
        TaskDomain::Data => "data",
        TaskDomain::General => "general",
    }
}

fn role_allows_schema(role: &StepRole, schema: &ToolSchema) -> bool {
    match role {
        StepRole::Researcher => !is_destructive_schema(schema) && !is_shell_schema(schema),
        StepRole::Verifier => !is_destructive_schema(schema) && !is_shell_schema(schema),
        StepRole::Executor => true,
    }
}

fn is_shell_schema(schema: &ToolSchema) -> bool {
    schema.name == "run_command"
        || schema
            .domains
            .iter()
            .any(|domain| matches!(domain.as_str(), "shell" | "git"))
}

fn is_destructive_schema(schema: &ToolSchema) -> bool {
    if matches!(schema.source, ToolSource::Builtin)
        && matches!(
            schema.name.as_str(),
            "write_file" | "delete_file" | "run_command"
        )
    {
        return true;
    }

    let haystack = format!(
        "{} {}",
        schema.name.to_ascii_lowercase(),
        schema.description.to_ascii_lowercase()
    );
    const MUTATING_TOKENS: [&str; 12] = [
        "write", "delete", "remove", "create", "update", "commit", "merge", "publish", "apply",
        "send", "post", "mutate",
    ];

    MUTATING_TOKENS.iter().any(|token| haystack.contains(token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_tools::ToolRegistry;
    use crate::conductor::StepType;
    use crate::config::{Config, LLMConfig};
    use crate::db::Database;
    use crate::gateway::WorkspaceLocks;
    use crate::llm::router::LLMRouter;
    use crate::storage::TaskRepository;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn build_test_executor(
        temp: &TempDir,
        config: Config,
        parent_task_id: Uuid,
        domain: TaskDomain,
    ) -> (Arc<TaskRepository>, AgentDagExecutor) {
        let db_path = temp.path().join(format!("{}.db", parent_task_id));
        let db = Database::new(&db_path).await.expect("database");
        let pool = db.pool().clone();
        let task_repo = Arc::new(TaskRepository::new(pool));
        task_repo
            .create_task(&parent_task_id, "remote dag parent")
            .await
            .expect("create parent task");

        let llm_config = Arc::new(LLMConfig {
            default_provider: "mock".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });
        let router = Arc::new(LLMRouter::new(vec![], llm_config));
        let executor = AgentDagExecutor {
            router,
            task_repo: task_repo.clone(),
            tools: Arc::new(ToolRegistry::empty()),
            memory_system: None,
            workspace_locks: Arc::new(WorkspaceLocks::new()),
            parent_task_id,
            source: TaskSource::Cli,
            domain,
            complexity: Complexity::Medium,
            sensitive: false,
            policy_after_write_commands: Vec::new(),
            config: Arc::new(config),
        };
        (task_repo, executor)
    }

    fn schema(name: &str, description: &str, domains: &[&str]) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: description.to_string(),
            parameters: json!({}),
            source: ToolSource::Builtin,
            domains: domains.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn researcher_catalog_filters_mutating_tools() {
        let tools = select_role_tool_catalog(
            &StepRole::Researcher,
            TaskDomain::Code,
            vec![
                schema("read_file", "Read file", &["all"]),
                schema("write_file", "Write file", &["all"]),
                ToolSchema {
                    name: "mcp_github_search_issues".to_string(),
                    description: "Search GitHub issues".to_string(),
                    parameters: json!({}),
                    source: ToolSource::Mcp {
                        server_name: "github".to_string(),
                    },
                    domains: vec!["code".to_string()],
                },
                ToolSchema {
                    name: "mcp_github_create_issue".to_string(),
                    description: "Create a GitHub issue".to_string(),
                    parameters: json!({}),
                    source: ToolSource::Mcp {
                        server_name: "github".to_string(),
                    },
                    domains: vec!["code".to_string()],
                },
            ],
        );

        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"mcp_github_search_issues".to_string()));
        assert!(!tools.contains(&"write_file".to_string()));
        assert!(!tools.contains(&"mcp_github_create_issue".to_string()));
    }

    #[tokio::test]
    async fn executor_step_can_delegate_to_remote_node() {
        let temp = TempDir::new().expect("temp dir");
        let parent_task_id = Uuid::new_v4();

        let mut config = Config::default();
        config.core.workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        config.core.data_dir = temp.path().join("data");
        config.ws_client.enabled = true;
        config.ws_client.auth_token = Some("remote-token".to_string());

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "remote-dag-step",
                "status": "completed",
                "answer": "remote executor answer",
                "provider": "executor-plan",
                "duration_ms": 9,
                "message": null
            })))
            .mount(&server)
            .await;

        let manager = RemoteManager::new(config.clone());
        manager
            .pair(
                "office-mac",
                Some(&server.uri()),
                None,
                true,
                &["workspace".to_string()],
                &[
                    "remote-execution".to_string(),
                    "system-execution".to_string(),
                ],
            )
            .await
            .expect("pair");
        manager.trust("office-mac").expect("trust");

        let (task_repo, executor) =
            build_test_executor(&temp, config, parent_task_id, TaskDomain::Code).await;
        let step = PlanStep {
            id: "step_1".to_string(),
            order: 1,
            step_type: StepType::Execute,
            role: StepRole::Executor,
            parallel_safe: false,
            route_policy: crate::conductor::RoutePolicy::Inherit,
            dependencies: Vec::new(),
            description: "find test.txt in the repo".to_string(),
            expected_outcome: "Path to test.txt".to_string(),
        };

        let execution = executor
            .try_remote_execute(&step, "", Route::Local)
            .await
            .expect("delegate result")
            .expect("remote delegation should occur");

        assert_eq!(execution.output, "remote executor answer");
        let requests = server.received_requests().await.expect("received requests");
        let execute_request = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/api/v1/remote/execute"
            })
            .expect("remote execute request");
        let request_body: serde_json::Value =
            serde_json::from_slice(&execute_request.body).expect("request body");
        assert_eq!(
            request_body
                .get("plan")
                .and_then(|value| value.get("steps"))
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("tool_name"))
                .and_then(|value| value.as_str()),
            Some("run_command")
        );
        let events = task_repo
            .get_agent_events(&parent_task_id.to_string())
            .await
            .expect("events");
        assert!(events
            .iter()
            .any(|event| event.event_type == "remote_delegate"));
        assert!(events
            .iter()
            .any(|event| event.event_type == "remote_result"));
    }

    #[tokio::test]
    async fn researcher_step_can_delegate_to_remote_full_node() {
        let temp = TempDir::new().expect("temp dir");
        let parent_task_id = Uuid::new_v4();

        let mut config = Config::default();
        config.core.workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        config.core.data_dir = temp.path().join("data");
        config.ws_client.enabled = true;
        config.ws_client.auth_token = Some("remote-token".to_string());

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "remote-research-step",
                "status": "completed",
                "answer": "remote research answer",
                "provider": "ollama",
                "duration_ms": 7,
                "message": null
            })))
            .mount(&server)
            .await;

        let manager = RemoteManager::new(config.clone());
        manager
            .pair(
                "research-mac",
                Some(&server.uri()),
                None,
                false,
                &["workspace".to_string()],
                &["remote-execution".to_string()],
            )
            .await
            .expect("pair");
        manager.trust("research-mac").expect("trust");

        let (_task_repo, executor) =
            build_test_executor(&temp, config, parent_task_id, TaskDomain::General).await;
        let step = PlanStep {
            id: "step_research".to_string(),
            order: 1,
            step_type: StepType::Research,
            role: StepRole::Researcher,
            parallel_safe: true,
            route_policy: crate::conductor::RoutePolicy::Inherit,
            dependencies: Vec::new(),
            description: "inspect the workspace and summarize where test files live".to_string(),
            expected_outcome: "List of candidate test file locations".to_string(),
        };

        let execution = executor
            .try_remote_execute(&step, "", Route::Local)
            .await
            .expect("delegate result")
            .expect("remote delegation should occur");

        assert_eq!(execution.output, "remote research answer");
        let requests = server.received_requests().await.expect("received requests");
        let execute_request = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/api/v1/remote/execute"
            })
            .expect("remote execute request");
        let request_body: serde_json::Value =
            serde_json::from_slice(&execute_request.body).expect("request body");
        assert!(request_body
            .get("plan")
            .map(serde_json::Value::is_null)
            .unwrap_or(true));
    }

    #[tokio::test]
    async fn verifier_step_can_delegate_to_remote_full_node() {
        let temp = TempDir::new().expect("temp dir");
        let parent_task_id = Uuid::new_v4();

        let mut config = Config::default();
        config.core.workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        config.core.data_dir = temp.path().join("data");
        config.ws_client.enabled = true;
        config.ws_client.auth_token = Some("remote-token".to_string());

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "remote-verify-step",
                "status": "completed",
                "answer": "remote verifier answer",
                "provider": "ollama",
                "duration_ms": 6,
                "message": null
            })))
            .mount(&server)
            .await;

        let manager = RemoteManager::new(config.clone());
        manager
            .pair(
                "verify-mac",
                Some(&server.uri()),
                None,
                false,
                &["workspace".to_string()],
                &["remote-execution".to_string()],
            )
            .await
            .expect("pair");
        manager.trust("verify-mac").expect("trust");

        let (_task_repo, executor) =
            build_test_executor(&temp, config, parent_task_id, TaskDomain::General).await;
        let step = PlanStep {
            id: "step_verify".to_string(),
            order: 1,
            step_type: StepType::Verify,
            role: StepRole::Verifier,
            parallel_safe: true,
            route_policy: crate::conductor::RoutePolicy::Inherit,
            dependencies: Vec::new(),
            description: "verify whether test.txt exists and report the result".to_string(),
            expected_outcome: "Verification report".to_string(),
        };

        let execution = executor
            .try_remote_execute(&step, "", Route::Local)
            .await
            .expect("delegate result")
            .expect("remote delegation should occur");

        assert_eq!(execution.output, "remote verifier answer");
        let requests = server.received_requests().await.expect("received requests");
        let execute_request = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/api/v1/remote/execute"
            })
            .expect("remote execute request");
        let request_body: serde_json::Value =
            serde_json::from_slice(&execute_request.body).expect("request body");
        assert!(request_body
            .get("plan")
            .map(serde_json::Value::is_null)
            .unwrap_or(true));
    }
}
