//! Hybrid Execution Mode
//!
//! Cloud brain plans multi-step tasks, local brain executes each step.
//! This provides the best of both worlds:
//! - Cloud: Large context, complex planning, multi-step reasoning
//! - Local: Fast execution, privacy for sensitive data, zero API cost per step
//!
//! Architecture:
//! 1. Cloud brain generates structured plan (list of steps with dependencies)
//! 2. APEX executor runs steps concurrently where possible
//! 3. Each step executed by local brain (if available) or cloud fallback
//! 4. Sensitive data never sent to cloud during execution

use crate::conductor::graph::ApexGraph;
use crate::conductor::policy::StepExecutionPolicy;
use crate::conductor::routing::ApexRoutingPolicy;
use crate::conductor::runner::{ApexNodeExecution, ApexNodeExecutor, ApexRunner};
use crate::conductor::types::{ConductorPlan, PlanStep, RoutePolicy, StepRole, StepType};
use crate::llm::router::LLMRouter;
use crate::llm::{LLMResponse, Message};
use anyhow::{Context, Result};
use async_trait::async_trait;
use brain::reasoning::LocalBrain;
use sdk::{Brain, Complexity, Route, TaskDomain};

/// Controls which LLM is used for APEX planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanMode {
    /// Use the ONNX classifier result: Complex → cloud, Simple/Medium → local.
    #[default]
    Auto,
    /// Always use a cloud provider for planning.
    Cloud,
    /// Always plan locally (skip cloud call and compression).
    Local,
}

/// Returns true when the planning context should be compressed before sending
/// to the cloud planner.
pub fn should_compress(complexity: Complexity, mode: PlanMode) -> bool {
    match mode {
        PlanMode::Cloud => true,
        PlanMode::Local => false,
        PlanMode::Auto => complexity == Complexity::Complex,
    }
}
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Hybrid execution coordinator
///
/// Manages the split between cloud planning and local execution
pub struct HybridExecutor {
    /// LLM router for cloud planning
    router: Arc<LLMRouter>,

    /// Optional local brain for step execution
    local_brain: Option<Arc<LocalBrain>>,

    /// Step execution results (shared across concurrent tasks)
    results: Arc<RwLock<HashMap<String, StepExecutionResult>>>,
}

/// Result of executing a single step
#[derive(Debug, Clone)]
pub struct StepExecutionResult {
    pub step_id: String,
    pub success: bool,
    pub output: String,
    pub execution_time_ms: u64,
    pub role: StepRole,
    pub executed_by: ExecutionLocation,
}

/// Where the step was executed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionLocation {
    LocalBrain,
    Cloud,
}

struct HybridNodeExecutor {
    router: Arc<LLMRouter>,
    local_brain: Option<Arc<LocalBrain>>,
}

#[async_trait]
impl ApexNodeExecutor for HybridNodeExecutor {
    async fn execute_node(
        &self,
        step: &PlanStep,
        dependency_context: &str,
        route: Route,
    ) -> Result<ApexNodeExecution> {
        let start = std::time::Instant::now();
        let policy = StepExecutionPolicy::for_step(step, route);
        let mut executed_route = route;

        if matches!(policy.preferred_route, Route::Local) {
            if let Some(local_brain) = &self.local_brain {
                if local_brain.check_available().await {
                    debug!("Executing step {} with local brain", step.id);
                    match self
                        .execute_with_local(local_brain, step, dependency_context, &policy)
                        .await
                    {
                        Ok(output) => {
                            let elapsed = start.elapsed().as_millis() as u64;
                            return Ok(ApexNodeExecution {
                                step_id: step.id.clone(),
                                output,
                                route: Route::Local,
                                duration_ms: elapsed,
                                role: step.role.clone(),
                            });
                        }
                        Err(error) => {
                            warn!("Local brain failed for step {}: {}", step.id, error);
                            executed_route = Route::Cloud;
                        }
                    }
                } else {
                    executed_route = Route::Cloud;
                }
            } else {
                executed_route = Route::Cloud;
            }
        }

        debug!("Executing step {} with {:?}", step.id, executed_route);
        let (output, actual_route) = self
            .execute_with_router(step, dependency_context, &policy)
            .await
            .context("Router execution failed")?;
        let elapsed = start.elapsed().as_millis() as u64;

        Ok(ApexNodeExecution {
            step_id: step.id.clone(),
            output,
            route: actual_route,
            duration_ms: elapsed,
            role: step.role.clone(),
        })
    }
}

impl HybridNodeExecutor {
    async fn execute_with_local(
        &self,
        local_brain: &Arc<LocalBrain>,
        step: &PlanStep,
        context: &str,
        policy: &StepExecutionPolicy,
    ) -> Result<String> {
        let system = policy.system_prompt(step, context);

        let messages = vec![sdk::Message {
            role: "user".to_string(),
            content: step.description.clone(),
        }];

        let response = local_brain
            .complete(&system, &messages, &[])
            .await
            .context("Local brain execution failed")?;

        Ok(response.content)
    }

    async fn execute_with_router(
        &self,
        step: &PlanStep,
        context: &str,
        policy: &StepExecutionPolicy,
    ) -> Result<(String, Route)> {
        let system = Message::system(policy.system_prompt(step, context));

        let user = Message::user(&step.description);

        let (response, provider) = self
            .router
            .call(&[system, user])
            .await
            .context("Cloud execution failed")?;
        let route = match provider.as_str() {
            "local-brain" => Route::Local,
            "ollama" => Route::Ollama,
            _ => Route::Cloud,
        };

        match response {
            LLMResponse::FinalAnswer(answer) => Ok((answer.content, route)),
            LLMResponse::ToolCall(call) => Ok((
                format!("Tool call: {}({})", call.name, call.arguments),
                route,
            )),
        }
    }
}

impl HybridExecutor {
    /// Create a new hybrid executor
    pub fn new(router: Arc<LLMRouter>, local_brain: Option<Arc<LocalBrain>>) -> Self {
        Self {
            router,
            local_brain,
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Compress the memory block locally before cloud planning.
    ///
    /// Returns `None` when:
    /// - No local brain is available
    /// - The local brain call fails
    /// - The compressed output is empty or not shorter than the raw input
    pub async fn compress_context_locally(&self, goal: &str, raw: &str) -> Option<String> {
        use crate::memory::conductor::session::truncate;

        let local_brain = self.local_brain.as_ref()?;
        if !local_brain.check_available().await {
            return None;
        }

        let capped = truncate(raw, 8000);

        let system = "You compress conversation context for a task planner. Preserve:\n\
            - The user's intent (what they want accomplished)\n\
            - Hard constraints (file paths, numbers, deadlines, names)\n\
            - Decisions already made\n\
            - Open questions\n\
            Drop: pleasantries, repetition, meta-commentary, tool-call noise.\n\
            Output tight bullets under 400 tokens. No preamble. No meta-summary.";

        let messages = vec![sdk::Message {
            role: "user".to_string(),
            content: format!("Goal: {}\n\nRaw context:\n{}", goal, capped),
        }];

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            local_brain.complete(system, &messages, &[]),
        )
        .await;

        let compressed = match result {
            Ok(Ok(response)) => response.content,
            Ok(Err(e)) => {
                warn!("Local compression failed: {}", e);
                return None;
            }
            Err(_) => {
                warn!("Local compression timed out");
                return None;
            }
        };

        if compressed.is_empty() || compressed.len() >= raw.len() {
            debug!(
                raw_len = raw.len(),
                compressed_len = compressed.len(),
                "Compression output rejected (empty or not shorter), using raw context"
            );
            return None;
        }

        info!(
            raw_chars = raw.len(),
            compressed_chars = compressed.len(),
            "Compressed memory: {} chars → {} chars",
            raw.len(),
            compressed.len()
        );
        Some(compressed)
    }

    /// Generate a plan using cloud brain
    ///
    /// Cloud brain has larger context and better planning capabilities.
    /// Always routes to a non-local (cloud) provider so that planning benefits
    /// from a large-context reasoning model.
    pub async fn plan_with_cloud(&self, goal: &str, context: &str) -> Result<ConductorPlan> {
        info!("Planning with cloud brain: {}", goal);

        let system = Message::system(
            "You are a task planner. Break down the user's goal into discrete, executable steps.\n\
            Output ONLY a JSON array of steps. Each step must have:\n\
            - \"id\": unique identifier (e.g., \"step_1\")\n\
            - \"description\": what to do in this step\n\
            - \"role\": one of \"Researcher\", \"Executor\", or \"Verifier\"\n\
            - \"parallel_safe\": true only for read-only gather or verification steps\n\
            - \"dependencies\": array of step IDs this depends on (empty for independent steps)\n\
            - \"expected_outcome\": success criteria\n\n\
            Make steps as independent as possible to enable parallel execution.\n\
            Keep steps small and focused (1-2 actions per step).\n\n\
            Example:\n\
            [{\"id\":\"step_1\",\"description\":\"Read config.toml\",\"role\":\"Researcher\",\"parallel_safe\":true,\"dependencies\":[],\"expected_outcome\":\"Config loaded\"},\
            {\"id\":\"step_2\",\"description\":\"Read main.rs\",\"role\":\"Researcher\",\"parallel_safe\":true,\"dependencies\":[],\"expected_outcome\":\"Code loaded\"},\
            {\"id\":\"step_3\",\"description\":\"Analyze both files\",\"role\":\"Verifier\",\"parallel_safe\":true,\"dependencies\":[\"step_1\",\"step_2\"],\"expected_outcome\":\"Analysis complete\"}]\n\n\
            Output ONLY the JSON array."
        );

        let user = Message::user(format!("Goal: {}\n\nContext:\n{}", goal, context));

        let (response, provider) = self
            .router
            .call_cloud_only(&[system, user])
            .await
            .context("Cloud planning failed")?;

        info!("Plan generated by: {}", provider);

        let content = match response {
            LLMResponse::FinalAnswer(answer) => answer.content,
            LLMResponse::ToolCall(_) => {
                return Err(anyhow::anyhow!("Expected plan, got tool call"));
            }
        };

        // Parse plan from JSON
        self.parse_plan(&content, goal)
    }

    /// Execute a plan using hybrid mode
    ///
    /// Steps are executed by local brain if available, cloud as fallback
    pub async fn execute_plan(
        &self,
        plan: &ConductorPlan,
    ) -> Result<HashMap<String, StepExecutionResult>> {
        info!("Executing plan: {} ({} steps)", plan.id, plan.steps.len());

        self.build_dag(&plan.steps)?;

        let preferred_route = if self.local_brain.is_some() {
            Route::Local
        } else {
            Route::Cloud
        };
        let mut graph = ApexGraph::from_plan(
            format!("plan:{}", plan.id),
            plan,
            TaskDomain::General,
            Complexity::Complex,
            false,
            preferred_route,
        );
        ApexRoutingPolicy::new(self.local_brain.is_some()).assign_routes(&mut graph, plan);
        let runner = ApexRunner::new();
        let node_executor = HybridNodeExecutor {
            router: Arc::clone(&self.router),
            local_brain: self.local_brain.clone(),
        };
        let report = runner.run(graph, plan, &node_executor).await?;

        if report.has_failures() {
            warn!(plan_id = %plan.id, "APEX execution completed with failed or blocked steps");
        }

        let mut results = self.results.write().await;
        results.clear();
        for (step_id, execution) in report.results {
            results.insert(
                step_id.clone(),
                StepExecutionResult {
                    step_id,
                    success: true,
                    output: execution.output,
                    execution_time_ms: execution.duration_ms,
                    role: execution.role,
                    executed_by: match execution.route {
                        Route::Local => ExecutionLocation::LocalBrain,
                        Route::Ollama | Route::Cloud => ExecutionLocation::Cloud,
                    },
                },
            );
        }

        Ok(results.clone())
    }

    /// Build dependency graph (DAG) from steps
    pub fn build_dag(&self, steps: &[PlanStep]) -> Result<HashMap<String, Vec<String>>> {
        let mut dag: HashMap<String, Vec<String>> = HashMap::new();

        for step in steps {
            dag.insert(step.id.clone(), step.dependencies.clone());
        }

        // Verify no cycles
        self.verify_acyclic(&dag)?;

        Ok(dag)
    }

    /// Verify DAG has no cycles
    fn verify_acyclic(&self, dag: &HashMap<String, Vec<String>>) -> Result<()> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for node in dag.keys() {
            if self.has_cycle(node, dag, &mut visited, &mut rec_stack) {
                return Err(anyhow::anyhow!("Cycle detected in dependency graph"));
            }
        }

        Ok(())
    }

    /// DFS cycle detection
    fn has_cycle(
        &self,
        node: &str,
        dag: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        if rec_stack.contains(node) {
            return true;
        }

        if visited.contains(node) {
            return false;
        }

        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());

        if let Some(deps) = dag.get(node) {
            for dep in deps {
                if self.has_cycle(dep, dag, visited, rec_stack) {
                    return true;
                }
            }
        }

        rec_stack.remove(node);
        false
    }

    /// Parse plan JSON from LLM response
    pub fn parse_plan(&self, content: &str, goal: &str) -> Result<ConductorPlan> {
        // Extract JSON array
        let json_str = if let Some(start) = content.find('[') {
            if let Some(end) = content.rfind(']') {
                &content[start..=end]
            } else {
                content
            }
        } else {
            content
        };

        #[derive(serde::Deserialize)]
        struct RawStep {
            id: String,
            description: String,
            role: Option<String>,
            parallel_safe: Option<bool>,
            #[serde(default)]
            dependencies: Vec<String>,
            expected_outcome: String,
        }

        let raw_steps: Vec<RawStep> =
            serde_json::from_str(json_str).context("Failed to parse plan JSON")?;

        let steps = raw_steps
            .into_iter()
            .enumerate()
            .map(|(i, raw)| {
                let role = match raw.role.as_deref().unwrap_or("executor") {
                    "Researcher" | "researcher" => StepRole::Researcher,
                    "Verifier" | "verifier" => StepRole::Verifier,
                    _ => StepRole::Executor,
                };
                let step_type = match role {
                    StepRole::Researcher => StepType::Research,
                    StepRole::Verifier => StepType::Verify,
                    StepRole::Executor => StepType::Execute,
                };
                let parallel_safe = raw
                    .parallel_safe
                    .unwrap_or(matches!(&step_type, StepType::Research | StepType::Verify));

                PlanStep {
                    id: raw.id,
                    order: i as u32,
                    description: raw.description,
                    step_type,
                    role: role.clone(),
                    parallel_safe,
                    route_policy: match role {
                        StepRole::Researcher | StepRole::Verifier => RoutePolicy::LocalPreferred,
                        StepRole::Executor => RoutePolicy::Inherit,
                    },
                    dependencies: raw.dependencies,
                    expected_outcome: raw.expected_outcome,
                }
            })
            .collect();

        Ok(ConductorPlan {
            id: uuid::Uuid::new_v4().to_string(),
            original_goal: goal.to_string(),
            mode: Default::default(),
            stages: vec![],
            steps,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| anyhow::anyhow!("System time error: {}", e))?
                .as_secs() as i64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conductor::types::{StepRole, StepType};
    use crate::config::LLMConfig;

    #[test]
    fn should_compress_auto_simple() {
        assert!(!should_compress(Complexity::Simple, PlanMode::Auto));
    }

    #[test]
    fn should_compress_auto_medium() {
        assert!(!should_compress(Complexity::Medium, PlanMode::Auto));
    }

    #[test]
    fn should_compress_auto_complex() {
        assert!(should_compress(Complexity::Complex, PlanMode::Auto));
    }

    #[test]
    fn should_compress_cloud_override() {
        assert!(should_compress(Complexity::Simple, PlanMode::Cloud));
    }

    #[test]
    fn should_compress_local_override() {
        assert!(!should_compress(Complexity::Complex, PlanMode::Local));
    }

    fn make_test_step(id: &str, deps: Vec<String>) -> PlanStep {
        PlanStep {
            id: id.to_string(),
            order: 0,
            description: format!("Step {}", id),
            step_type: StepType::Execute,
            role: StepRole::Executor,
            parallel_safe: false,
            route_policy: RoutePolicy::Inherit,
            dependencies: deps,
            expected_outcome: "Done".to_string(),
        }
    }

    #[test]
    fn test_dag_no_cycles() {
        let config = Arc::new(LLMConfig {
            default_provider: "ollama".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });
        let router = Arc::new(LLMRouter::new(vec![], config));
        let executor = HybridExecutor::new(router, None);

        let steps = vec![
            make_test_step("step_1", vec![]),
            make_test_step("step_2", vec!["step_1".to_string()]),
            make_test_step("step_3", vec!["step_2".to_string()]),
        ];

        let dag = executor.build_dag(&steps).unwrap();
        assert_eq!(dag.len(), 3);
    }

    #[test]
    fn test_dag_detects_cycle() {
        let config = Arc::new(LLMConfig {
            default_provider: "ollama".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });
        let router = Arc::new(LLMRouter::new(vec![], config));
        let executor = HybridExecutor::new(router, None);

        let mut dag = HashMap::new();
        dag.insert("step_1".to_string(), vec!["step_2".to_string()]);
        dag.insert("step_2".to_string(), vec!["step_1".to_string()]);

        let result = executor.verify_acyclic(&dag);
        assert!(result.is_err());
    }

    #[test]
    fn test_dag_parallel_steps() {
        let config = Arc::new(LLMConfig {
            default_provider: "ollama".to_string(),
            sensitivity_threshold: 0.7,
            complexity_threshold: 0.8,
            ollama: Default::default(),
            openai: Default::default(),
            anthropic: Default::default(),
            gemini: Default::default(),
            nvidia_nim: Default::default(),
            custom_providers: vec![],
        });
        let router = Arc::new(LLMRouter::new(vec![], config));
        let executor = HybridExecutor::new(router, None);

        // step_1 and step_2 can run in parallel, step_3 depends on both
        let steps = vec![
            make_test_step("step_1", vec![]),
            make_test_step("step_2", vec![]),
            make_test_step("step_3", vec!["step_1".to_string(), "step_2".to_string()]),
        ];

        let dag = executor.build_dag(&steps).unwrap();
        assert_eq!(dag.get("step_3").unwrap().len(), 2);
    }
}
