use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use sdk::Route;
use std::collections::HashMap;
use std::time::Instant;
use tracing::warn;
use uuid::Uuid;

use crate::conductor::{
    DagNodeExecution, DagNodeExecutor, DagRoutingPolicy, DagRunner, DagRunReport, HybridExecutor,
    StepExecutionPolicy,
};
use crate::conductor::types::StepRole;
use crate::gateway::Task;
use crate::llm::{Message, MessageRole, ToolCall, LLMResponse};
use crate::security::secrets::scrub_text;

use super::prompt::TaskContext;
use super::{AgentCore, TaskResult, MAX_RESULT_SIZE};

const DAG_STEP_MAX_ITERATIONS: usize = 8;

struct AgentDagExecutor<'a> {
    agent: &'a AgentCore,
    task_id: Uuid,
}

#[async_trait]
impl DagNodeExecutor for AgentDagExecutor<'_> {
    async fn execute_node(
        &self,
        step: &crate::conductor::PlanStep,
        dependency_context: &str,
        route: Route,
    ) -> Result<DagNodeExecution> {
        let started_at = Instant::now();
        let policy = StepExecutionPolicy::for_step(step, route);
        let tool_prompt = self.agent.tools.system_prompt_for_query(&step.description);
        let system_prompt = format!(
            "{}\n\nSPECIALIST POLICY:\n{}",
            tool_prompt,
            policy.system_prompt(step, dependency_context)
        );

        let mut messages = vec![
            Message::system(system_prompt),
            Message::user(step.description.clone()),
        ];
        let mut tool_call_counts: HashMap<u64, u32> = HashMap::new();

        for _ in 0..DAG_STEP_MAX_ITERATIONS {
            let (response, provider_name) = self
                .agent
                .router
                .call_with_sensitivity(&messages, Some(self.agent.current_task_sensitive))
                .await
                .context("DAG step LLM call failed")?;

            match response {
                LLMResponse::ToolCall(tool_call) => {
                    record_step_tool_call(&self.task_id, &mut tool_call_counts, &tool_call)?;
                    let execution = self
                        .agent
                        .execute_tool_call(&self.task_id.to_string(), &tool_call)
                        .await?;

                    messages.push(self.agent.assistant_tool_message(&self.task_id, &tool_call));
                    messages.push(Message::tool_result(&execution.safe_result, &tool_call.id));
                }
                LLMResponse::FinalAnswer(answer) => {
                    if answer.content.len() > MAX_RESULT_SIZE {
                        return Err(anyhow!(
                            "DAG step {} produced oversized result ({} bytes)",
                            step.id,
                            answer.content.len()
                        ));
                    }

                    return Ok(DagNodeExecution {
                        step_id: step.id.clone(),
                        output: answer.content,
                        route: provider_route(&provider_name),
                        duration_ms: started_at.elapsed().as_millis() as u64,
                        role: step.role.clone(),
                    });
                }
            }
        }

        Err(anyhow!(
            "DAG step {} exceeded {} tool iterations",
            step.id,
            DAG_STEP_MAX_ITERATIONS
        ))
    }
}

impl AgentCore {
    pub(super) async fn execute_dag_task(
        &self,
        task_id: &Uuid,
        task: &Task,
        context: &TaskContext,
        start_time: Instant,
    ) -> Result<TaskResult> {
        let planner = HybridExecutor::new(self.router.clone(), self.router.local_brain());
        let planning_context = self.dag_planning_context(task);
        let plan = planner
            .plan_with_cloud(&task.input, &planning_context)
            .await
            .context("Failed to build DAG plan for complex task")?;

        let mut graph = crate::conductor::DagGraph::from_plan(
            task_id.to_string(),
            &plan,
            context.domain,
            context.complexity,
            context.route,
        );
        DagRoutingPolicy::new(self.router.local_brain().is_some()).assign_routes(&mut graph, &plan);

        let runner =
            DagRunner::with_persistence(self.task_repo.clone(), *task_id, context.domain_str.clone());
        let executor = AgentDagExecutor {
            agent: self,
            task_id: *task_id,
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
            .insert_agent_event(task_id, "answer", &answer_payload, 50_000, Some(&context.domain_str))
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

    fn dag_planning_context(&self, task: &Task) -> String {
        let mut lines = Vec::new();
        if let Some(workspace) = &task.workspace {
            lines.push(format!("Workspace: {}", workspace.display()));
        }

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

fn record_step_tool_call(
    task_id: &Uuid,
    tool_call_counts: &mut HashMap<u64, u32>,
    tool_call: &ToolCall,
) -> Result<()> {
    let key = stable_tool_hash(tool_call);
    let count = tool_call_counts.entry(key).or_default();
    *count += 1;

    if *count >= 3 {
        warn!(
            task_id = %task_id,
            tool = %tool_call.name,
            "DAG step repeated tool '{}' with identical arguments 3 times",
            tool_call.name
        );
        return Err(anyhow!(
            "Tool '{}' with identical arguments called 3 times in DAG step",
            tool_call.name
        ));
    }

    Ok(())
}

fn stable_tool_hash(tool_call: &ToolCall) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    tool_call.name.hash(&mut hasher);
    tool_call.arguments.hash(&mut hasher);
    hasher.finish()
}
