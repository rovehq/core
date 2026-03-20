use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures::future::join_all;
use sdk::Route;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::security::secrets::scrub_text;
use crate::storage::{StepType as StorageStepType, TaskRepository};

use super::graph::{DagGraph, DagNodeState, DagWave};
use super::types::{ConductorPlan, PlanStep, StepRole, StepType};

#[derive(Debug, Clone)]
pub struct DagNodeExecution {
    pub step_id: String,
    pub output: String,
    pub route: Route,
    pub duration_ms: u64,
    pub role: StepRole,
}

#[derive(Debug, Clone)]
pub struct DagRunReport {
    pub graph: DagGraph,
    pub results: HashMap<String, DagNodeExecution>,
}

impl DagRunReport {
    pub fn has_failures(&self) -> bool {
        self.graph.has_failures()
    }
}

#[async_trait]
pub trait DagNodeExecutor: Send + Sync {
    async fn execute_node(
        &self,
        step: &PlanStep,
        dependency_context: &str,
        route: Route,
    ) -> Result<DagNodeExecution>;
}

#[derive(Clone)]
struct DagPersistence {
    task_id: Uuid,
    task_repo: Arc<TaskRepository>,
    domain: String,
}

#[derive(Default)]
pub struct DagRunner {
    persistence: Option<DagPersistence>,
}

impl DagRunner {
    pub fn new() -> Self {
        Self { persistence: None }
    }

    pub fn with_persistence(
        task_repo: Arc<TaskRepository>,
        task_id: Uuid,
        domain: impl Into<String>,
    ) -> Self {
        Self {
            persistence: Some(DagPersistence {
                task_id,
                task_repo,
                domain: domain.into(),
            }),
        }
    }

    pub async fn run<E>(
        &self,
        mut graph: DagGraph,
        plan: &ConductorPlan,
        executor: &E,
    ) -> Result<DagRunReport>
    where
        E: DagNodeExecutor,
    {
        let mut results = HashMap::new();
        let mut next_step_order = 1_i64;
        let mut next_event_step = 1_i64;

        self.validate_plan(plan)?;

        while !graph.is_complete() {
            let ready = graph.ready_node_ids();
            if ready.is_empty() {
                return Err(anyhow!(
                    "DAG execution stalled: no ready nodes remain for plan {}",
                    plan.id
                ));
            }

            let wave_ids = self.select_wave(&ready, plan)?;
            let wave = graph.open_wave(&wave_ids, wave_ids.len() > 1);
            self.persist_wave_started(&wave, &mut next_event_step).await?;

            let mut specs = Vec::with_capacity(wave_ids.len());
            for step_id in &wave_ids {
                let step = plan_step(plan, step_id)?;
                let context = dependency_context(&graph, step);
                let route = graph
                    .node(step_id)
                    .map(|node| node.route)
                    .unwrap_or(graph.preferred_route);
                let started_at = unix_timestamp();

                graph.mark_running(step_id, started_at, route);
                self.persist_step_started(
                    step,
                    wave.index,
                    route,
                    &mut next_step_order,
                    &mut next_event_step,
                )
                .await?;

                specs.push(PendingNodeExecution {
                    step: step.clone(),
                    context,
                    route,
                    wave_index: wave.index,
                });
            }

            let outcomes = if wave.parallel {
                join_all(specs.into_iter().map(|spec| async {
                    let result = executor
                        .execute_node(&spec.step, &spec.context, spec.route)
                        .await;
                    (spec, result)
                }))
                .await
            } else {
                let mut outcomes = Vec::new();
                for spec in specs {
                    let result = executor
                        .execute_node(&spec.step, &spec.context, spec.route)
                        .await;
                    outcomes.push((spec, result));
                }
                outcomes
            };

            for (spec, outcome) in outcomes {
                let finished_at = unix_timestamp();
                match outcome {
                    Ok(execution) => {
                        graph.mark_succeeded(&spec.step.id, finished_at, execution.output.clone());
                        if let Some(node) = graph.node_mut(&spec.step.id) {
                            node.route = execution.route;
                        }
                        self.persist_step_succeeded(
                            &spec.step,
                            spec.wave_index,
                            &execution,
                            &mut next_step_order,
                            &mut next_event_step,
                        )
                        .await?;
                        results.insert(spec.step.id.clone(), execution);
                    }
                    Err(error) => {
                        let safe_error = scrub_text(&error.to_string());
                        graph.mark_failed(&spec.step.id, finished_at, safe_error.clone());
                        self.persist_step_failed(
                            &spec.step,
                            spec.wave_index,
                            spec.route,
                            &safe_error,
                            &mut next_step_order,
                            &mut next_event_step,
                        )
                        .await?;
                    }
                }
            }

            let blocked_before = blocked_node_ids(&graph);
            graph.advance_ready_states(plan);
            let blocked_after = blocked_node_ids(&graph);
            for step_id in blocked_after.difference(&blocked_before) {
                let step = plan_step(plan, step_id)?;
                if let Some(node) = graph.node_mut(step_id) {
                    node.finished_at = Some(unix_timestamp());
                    node.error = Some("blocked by failed dependency".to_string());
                }
                self.persist_step_blocked(
                    step,
                    wave.index,
                    &mut next_step_order,
                    &mut next_event_step,
                )
                .await?;
            }
        }

        graph.completed_at = Some(unix_timestamp());

        Ok(DagRunReport { graph, results })
    }

    fn validate_plan(&self, plan: &ConductorPlan) -> Result<()> {
        let known_steps: HashSet<&str> = plan.steps.iter().map(|step| step.id.as_str()).collect();
        for step in &plan.steps {
            for dependency in &step.dependencies {
                if !known_steps.contains(dependency.as_str()) {
                    return Err(anyhow!(
                        "step {} depends on unknown step {}",
                        step.id,
                        dependency
                    ));
                }
            }
        }

        Ok(())
    }

    fn select_wave(&self, ready: &[String], plan: &ConductorPlan) -> Result<Vec<String>> {
        let mut ready_steps = ready
            .iter()
            .map(|step_id| plan_step(plan, step_id))
            .collect::<Result<Vec<_>>>()?;
        ready_steps.sort_by_key(|step| step.order);

        let mut serial = ready_steps
            .iter()
            .filter(|step| !step.parallel_safe)
            .map(|step| step.id.clone())
            .collect::<Vec<_>>();
        if !serial.is_empty() {
            serial.truncate(1);
            return Ok(serial);
        }

        Ok(ready_steps.into_iter().map(|step| step.id.clone()).collect())
    }

    async fn persist_wave_started(
        &self,
        wave: &DagWave,
        next_event_step: &mut i64,
    ) -> Result<()> {
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };

        let payload = serde_json::json!({
            "wave": wave.index,
            "parallel": wave.parallel,
            "steps": wave.node_ids,
        })
        .to_string();

        persistence
            .task_repo
            .insert_agent_event(
                &persistence.task_id,
                "dag_wave_started",
                &payload,
                take_counter(next_event_step),
                Some(&persistence.domain),
            )
            .await
            .context("Failed to persist DAG wave start event")?;

        Ok(())
    }

    async fn persist_step_started(
        &self,
        step: &PlanStep,
        wave_index: u32,
        route: Route,
        next_step_order: &mut i64,
        next_event_step: &mut i64,
    ) -> Result<()> {
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };

        let content = serde_json::json!({
            "step_id": step.id,
            "description": scrub_text(&step.description),
            "status": "running",
            "role": role_label(&step.role),
            "wave": wave_index,
            "route": route_label(route),
        })
        .to_string();

        persistence
            .task_repo
            .add_task_step(
                &persistence.task_id,
                take_counter(next_step_order),
                storage_step_type(&step.step_type),
                &content,
            )
            .await
            .context("Failed to persist DAG task step start")?;

        persistence
            .task_repo
            .insert_agent_event(
                &persistence.task_id,
                "dag_step_started",
                &content,
                take_counter(next_event_step),
                Some(&persistence.domain),
            )
            .await
            .context("Failed to persist DAG step start event")?;

        Ok(())
    }

    async fn persist_step_succeeded(
        &self,
        step: &PlanStep,
        wave_index: u32,
        execution: &DagNodeExecution,
        next_step_order: &mut i64,
        next_event_step: &mut i64,
    ) -> Result<()> {
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };

        let content = serde_json::json!({
            "step_id": step.id,
            "status": "succeeded",
            "role": role_label(&execution.role),
            "wave": wave_index,
            "route": route_label(execution.route),
            "duration_ms": execution.duration_ms,
            "output": scrub_text(&execution.output),
        })
        .to_string();

        persistence
            .task_repo
            .add_task_step(
                &persistence.task_id,
                take_counter(next_step_order),
                storage_step_type(&step.step_type),
                &content,
            )
            .await
            .context("Failed to persist DAG task step success")?;

        persistence
            .task_repo
            .insert_agent_event(
                &persistence.task_id,
                "dag_step_succeeded",
                &content,
                take_counter(next_event_step),
                Some(&persistence.domain),
            )
            .await
            .context("Failed to persist DAG step success event")?;

        Ok(())
    }

    async fn persist_step_failed(
        &self,
        step: &PlanStep,
        wave_index: u32,
        route: Route,
        error: &str,
        next_step_order: &mut i64,
        next_event_step: &mut i64,
    ) -> Result<()> {
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };

        let content = serde_json::json!({
            "step_id": step.id,
            "status": "failed",
            "role": role_label(&step.role),
            "wave": wave_index,
            "route": route_label(route),
            "error": error,
        })
        .to_string();

        persistence
            .task_repo
            .add_task_step(
                &persistence.task_id,
                take_counter(next_step_order),
                storage_step_type(&step.step_type),
                &content,
            )
            .await
            .context("Failed to persist DAG task step failure")?;

        persistence
            .task_repo
            .insert_agent_event(
                &persistence.task_id,
                "dag_step_failed",
                &content,
                take_counter(next_event_step),
                Some(&persistence.domain),
            )
            .await
            .context("Failed to persist DAG step failure event")?;

        Ok(())
    }

    async fn persist_step_blocked(
        &self,
        step: &PlanStep,
        wave_index: u32,
        next_step_order: &mut i64,
        next_event_step: &mut i64,
    ) -> Result<()> {
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };

        let content = serde_json::json!({
            "step_id": step.id,
            "status": "blocked",
            "role": role_label(&step.role),
            "wave": wave_index,
            "error": "blocked by failed dependency",
        })
        .to_string();

        persistence
            .task_repo
            .add_task_step(
                &persistence.task_id,
                take_counter(next_step_order),
                storage_step_type(&step.step_type),
                &content,
            )
            .await
            .context("Failed to persist blocked DAG task step")?;

        persistence
            .task_repo
            .insert_agent_event(
                &persistence.task_id,
                "dag_step_blocked",
                &content,
                take_counter(next_event_step),
                Some(&persistence.domain),
            )
            .await
            .context("Failed to persist blocked DAG step event")?;

        Ok(())
    }
}

struct PendingNodeExecution {
    step: PlanStep,
    context: String,
    route: Route,
    wave_index: u32,
}

fn dependency_context(graph: &DagGraph, step: &PlanStep) -> String {
    step.dependencies
        .iter()
        .filter_map(|dependency| graph.node(dependency))
        .filter_map(|node| node.output.as_ref().map(|output| (node.step_id.as_str(), output)))
        .map(|(step_id, output)| format!("[{}]: {}", step_id, output))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn plan_step<'a>(plan: &'a ConductorPlan, step_id: &str) -> Result<&'a PlanStep> {
    plan.steps
        .iter()
        .find(|step| step.id == step_id)
        .ok_or_else(|| anyhow!("Step not found in plan: {}", step_id))
}

fn blocked_node_ids(graph: &DagGraph) -> HashSet<String> {
    graph
        .nodes
        .iter()
        .filter(|node| node.state == DagNodeState::Blocked)
        .map(|node| node.step_id.clone())
        .collect()
}

fn storage_step_type(step_type: &StepType) -> StorageStepType {
    match step_type {
        // task_steps still enforces the legacy message/tool enum in SQLite.
        // DAG-specific state lives in the JSON payload and agent_events until the
        // task_steps schema is widened in a dedicated storage migration.
        StepType::Research | StepType::Execute | StepType::Verify => {
            StorageStepType::AssistantMessage
        }
    }
}

fn take_counter(counter: &mut i64) -> i64 {
    let current = *counter;
    *counter += 1;
    current
}

fn role_label(role: &StepRole) -> &'static str {
    match role {
        StepRole::Researcher => "researcher",
        StepRole::Executor => "executor",
        StepRole::Verifier => "verifier",
    }
}

fn route_label(route: Route) -> &'static str {
    match route {
        Route::Local => "local",
        Route::Ollama => "ollama",
        Route::Cloud => "cloud",
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use crate::conductor::types::RoutePolicy;
    use sdk::{Complexity, Route, TaskDomain};
    use std::collections::HashSet;
    use tempfile::TempDir;

    struct MockDagExecutor {
        failing: HashSet<String>,
    }

    #[async_trait]
    impl DagNodeExecutor for MockDagExecutor {
        async fn execute_node(
            &self,
            step: &PlanStep,
            dependency_context: &str,
            route: Route,
        ) -> Result<DagNodeExecution> {
            if self.failing.contains(&step.id) {
                return Err(anyhow!("{} failed", step.id));
            }

            Ok(DagNodeExecution {
                step_id: step.id.clone(),
                output: format!("{} :: {}", step.description, dependency_context),
                route,
                duration_ms: 5,
                role: step.role.clone(),
            })
        }
    }

    fn sample_plan() -> ConductorPlan {
        ConductorPlan {
            id: "plan-1".to_string(),
            original_goal: "summarize repo and verify".to_string(),
            mode: Default::default(),
            stages: Vec::new(),
            steps: vec![
                PlanStep {
                    id: "step_1".to_string(),
                    order: 0,
                    step_type: StepType::Research,
                    role: StepRole::Researcher,
                    parallel_safe: true,
                    route_policy: RoutePolicy::LocalPreferred,
                    dependencies: Vec::new(),
                    description: "read docs".to_string(),
                    expected_outcome: "docs loaded".to_string(),
                },
                PlanStep {
                    id: "step_2".to_string(),
                    order: 1,
                    step_type: StepType::Research,
                    role: StepRole::Researcher,
                    parallel_safe: true,
                    route_policy: RoutePolicy::LocalPreferred,
                    dependencies: Vec::new(),
                    description: "read code".to_string(),
                    expected_outcome: "code loaded".to_string(),
                },
                PlanStep {
                    id: "step_3".to_string(),
                    order: 2,
                    step_type: StepType::Verify,
                    role: StepRole::Verifier,
                    parallel_safe: true,
                    route_policy: RoutePolicy::LocalPreferred,
                    dependencies: vec!["step_1".to_string(), "step_2".to_string()],
                    description: "cross-check findings".to_string(),
                    expected_outcome: "validated summary".to_string(),
                },
            ],
            created_at: 100,
        }
    }

    #[tokio::test]
    async fn runner_executes_parallel_wave_then_dependent_step() {
        let plan = sample_plan();
        let graph = DagGraph::from_plan(
            "task-1",
            &plan,
            TaskDomain::Code,
            Complexity::Complex,
            false,
            Route::Local,
        );
        let runner = DagRunner::new();
        let report = runner
            .run(
                graph,
                &plan,
                &MockDagExecutor {
                    failing: HashSet::new(),
                },
            )
            .await
            .unwrap();

        assert_eq!(report.graph.waves.len(), 2);
        assert_eq!(report.graph.waves[0].node_ids.len(), 2);
        assert!(report.graph.waves[0].parallel);
        assert_eq!(report.graph.waves[1].node_ids, vec!["step_3".to_string()]);
        assert_eq!(
            report.graph.node("step_3").and_then(|node| node.output.clone()),
            Some("cross-check findings :: [step_1]: read docs :: \n\n[step_2]: read code :: ".to_string())
        );
    }

    #[tokio::test]
    async fn runner_persists_wave_and_step_events() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("runner.db");
        let database = Database::new(&db_path).await.unwrap();
        let repo = Arc::new(database.tasks());
        let task_id = Uuid::new_v4();
        repo.create_task(&task_id, "run dag").await.unwrap();

        let plan = sample_plan();
        let graph = DagGraph::from_plan(
            task_id.to_string(),
            &plan,
            TaskDomain::Code,
            Complexity::Complex,
            false,
            Route::Local,
        );
        let runner = DagRunner::with_persistence(repo.clone(), task_id, "code");

        let report = runner
            .run(
                graph,
                &plan,
                &MockDagExecutor {
                    failing: HashSet::new(),
                },
            )
            .await
            .unwrap();

        assert!(!report.has_failures());

        let steps = repo.get_task_steps(&task_id).await.unwrap();
        assert_eq!(steps.len(), 6);

        let events = repo.get_agent_events(&task_id.to_string()).await.unwrap();
        assert!(events.iter().any(|event| event.event_type == "dag_wave_started"));
        assert!(
            events
                .iter()
                .any(|event| event.event_type == "dag_step_succeeded")
        );
    }

    #[tokio::test]
    async fn runner_blocks_dependents_after_failure() {
        let plan = sample_plan();
        let graph = DagGraph::from_plan(
            "task-1",
            &plan,
            TaskDomain::Code,
            Complexity::Complex,
            false,
            Route::Cloud,
        );
        let runner = DagRunner::new();

        let report = runner
            .run(
                graph,
                &plan,
                &MockDagExecutor {
                    failing: HashSet::from(["step_1".to_string()]),
                },
            )
            .await
            .unwrap();

        assert!(report.has_failures());
        assert_eq!(
            report.graph.node("step_1").map(|node| node.state.clone()),
            Some(DagNodeState::Failed)
        );
        assert_eq!(
            report.graph.node("step_3").map(|node| node.state.clone()),
            Some(DagNodeState::Blocked)
        );
    }
}
