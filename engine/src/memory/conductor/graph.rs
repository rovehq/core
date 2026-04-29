use sdk::{Complexity, Route, TaskDomain};
use serde::{Deserialize, Serialize};

use super::types::{ConductorPlan, RoutePolicy};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ApexNodeState {
    #[default]
    Pending,
    Ready,
    Running,
    Succeeded,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApexNode {
    pub step_id: String,
    pub state: ApexNodeState,
    pub attempt: u32,
    pub route: Route,
    pub route_policy: RoutePolicy,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApexWave {
    pub index: u32,
    pub node_ids: Vec<String>,
    pub parallel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApexGraph {
    pub plan_id: String,
    pub task_id: String,
    pub goal: String,
    pub domain: TaskDomain,
    pub complexity: Complexity,
    pub sensitive: bool,
    pub preferred_route: Route,
    pub nodes: Vec<ApexNode>,
    pub waves: Vec<ApexWave>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

impl ApexGraph {
    pub fn from_plan(
        task_id: impl Into<String>,
        plan: &ConductorPlan,
        domain: TaskDomain,
        complexity: Complexity,
        sensitive: bool,
        preferred_route: Route,
    ) -> Self {
        let nodes = plan
            .steps
            .iter()
            .map(|step| ApexNode {
                step_id: step.id.clone(),
                state: if step.dependencies.is_empty() {
                    ApexNodeState::Ready
                } else {
                    ApexNodeState::Pending
                },
                attempt: 0,
                route: preferred_route,
                route_policy: step.route_policy.clone(),
                started_at: None,
                finished_at: None,
                output: None,
                error: None,
            })
            .collect();

        Self {
            plan_id: plan.id.clone(),
            task_id: task_id.into(),
            goal: plan.original_goal.clone(),
            domain,
            complexity,
            sensitive,
            preferred_route,
            nodes,
            waves: Vec::new(),
            created_at: plan.created_at,
            completed_at: None,
        }
    }

    pub fn ready_node_ids(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|node| node.state == ApexNodeState::Ready)
            .map(|node| node.step_id.clone())
            .collect()
    }

    pub fn node(&self, step_id: &str) -> Option<&ApexNode> {
        self.nodes.iter().find(|node| node.step_id == step_id)
    }

    pub fn node_mut(&mut self, step_id: &str) -> Option<&mut ApexNode> {
        self.nodes.iter_mut().find(|node| node.step_id == step_id)
    }

    pub fn open_wave(&mut self, node_ids: &[String], parallel: bool) -> ApexWave {
        let wave = ApexWave {
            index: self.waves.len() as u32,
            node_ids: node_ids.to_vec(),
            parallel,
        };
        self.waves.push(wave.clone());
        wave
    }

    pub fn mark_running(&mut self, step_id: &str, started_at: i64, route: Route) {
        if let Some(node) = self.node_mut(step_id) {
            node.state = ApexNodeState::Running;
            node.attempt += 1;
            node.route = route;
            node.started_at = Some(started_at);
            node.finished_at = None;
            node.error = None;
        }
    }

    pub fn mark_succeeded(&mut self, step_id: &str, finished_at: i64, output: String) {
        if let Some(node) = self.node_mut(step_id) {
            node.state = ApexNodeState::Succeeded;
            node.finished_at = Some(finished_at);
            node.output = Some(output);
            node.error = None;
        }
    }

    pub fn mark_failed(&mut self, step_id: &str, finished_at: i64, error: String) {
        if let Some(node) = self.node_mut(step_id) {
            node.state = ApexNodeState::Failed;
            node.finished_at = Some(finished_at);
            node.error = Some(error);
        }
    }

    pub fn advance_ready_states(&mut self, plan: &ConductorPlan) -> Vec<String> {
        let mut promoted = Vec::new();

        for step in &plan.steps {
            let Some(node) = self.node(&step.id) else {
                continue;
            };
            if node.state != ApexNodeState::Pending {
                continue;
            }

            let blocked = step.dependencies.iter().any(|dependency| {
                self.node(dependency)
                    .is_some_and(|dependency_node| dependency_node.state == ApexNodeState::Failed)
            });
            if blocked {
                if let Some(node) = self.node_mut(&step.id) {
                    node.state = ApexNodeState::Blocked;
                }
                continue;
            }

            let ready = step.dependencies.iter().all(|dependency| {
                self.node(dependency).is_some_and(|dependency_node| {
                    dependency_node.state == ApexNodeState::Succeeded
                })
            });
            if ready {
                if let Some(node) = self.node_mut(&step.id) {
                    node.state = ApexNodeState::Ready;
                }
                promoted.push(step.id.clone());
            }
        }

        promoted
    }

    pub fn is_complete(&self) -> bool {
        self.nodes.iter().all(|node| {
            matches!(
                node.state,
                ApexNodeState::Succeeded | ApexNodeState::Failed | ApexNodeState::Blocked
            )
        })
    }

    pub fn has_failures(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| matches!(node.state, ApexNodeState::Failed | ApexNodeState::Blocked))
    }
}

#[cfg(test)]
mod tests {
    use sdk::{Complexity, Route, TaskDomain};

    use crate::conductor::types::{
        ConductorPlan, ExecutionMode, PlanStep, RoutePolicy, StepRole, StepType,
    };

    use super::{ApexGraph, ApexNodeState};

    fn sample_plan() -> ConductorPlan {
        ConductorPlan {
            id: "plan-1".to_string(),
            original_goal: "do the thing".to_string(),
            mode: ExecutionMode::Direct,
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
                    description: "research".to_string(),
                    expected_outcome: "facts".to_string(),
                },
                PlanStep {
                    id: "step_2".to_string(),
                    order: 1,
                    step_type: StepType::Execute,
                    role: StepRole::Executor,
                    parallel_safe: false,
                    route_policy: RoutePolicy::Inherit,
                    dependencies: vec!["step_1".to_string()],
                    description: "execute".to_string(),
                    expected_outcome: "change".to_string(),
                },
            ],
            created_at: 100,
        }
    }

    #[test]
    fn graph_starts_with_dependency_free_steps_ready() {
        let plan = sample_plan();
        let graph = ApexGraph::from_plan(
            "task-1",
            &plan,
            TaskDomain::Code,
            Complexity::Complex,
            false,
            Route::Local,
        );

        assert_eq!(graph.ready_node_ids(), vec!["step_1".to_string()]);
        assert_eq!(graph.node("step_2").unwrap().state, ApexNodeState::Pending);
    }

    #[test]
    fn graph_promotes_dependents_after_success() {
        let plan = sample_plan();
        let mut graph = ApexGraph::from_plan(
            "task-1",
            &plan,
            TaskDomain::Code,
            Complexity::Complex,
            false,
            Route::Local,
        );

        graph.mark_running("step_1", 101, Route::Local);
        graph.mark_succeeded("step_1", 102, "done".to_string());

        let promoted = graph.advance_ready_states(&plan);

        assert_eq!(promoted, vec!["step_2".to_string()]);
        assert_eq!(graph.node("step_2").unwrap().state, ApexNodeState::Ready);
    }

    #[test]
    fn graph_blocks_dependents_after_failure() {
        let plan = sample_plan();
        let mut graph = ApexGraph::from_plan(
            "task-1",
            &plan,
            TaskDomain::Code,
            Complexity::Complex,
            false,
            Route::Local,
        );

        graph.mark_failed("step_1", 102, "boom".to_string());
        graph.advance_ready_states(&plan);

        assert_eq!(graph.node("step_2").unwrap().state, ApexNodeState::Blocked);
        assert!(graph.has_failures());
        assert!(graph.is_complete());
    }
}
