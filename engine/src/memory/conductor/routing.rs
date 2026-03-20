use sdk::{Complexity, Route, TaskDomain};

use super::graph::DagGraph;
use super::types::{ConductorPlan, PlanStep, StepRole};

pub struct DagRoutingPolicy {
    local_brain_available: bool,
}

impl DagRoutingPolicy {
    pub fn new(local_brain_available: bool) -> Self {
        Self {
            local_brain_available,
        }
    }

    pub fn assign_routes(&self, graph: &mut DagGraph, plan: &ConductorPlan) {
        for step in &plan.steps {
            let route = self.route_for_step(step, graph.domain, graph.complexity, graph.preferred_route);
            if let Some(node) = graph.node_mut(&step.id) {
                node.route = route;
            }
        }
    }

    pub fn route_for_step(
        &self,
        step: &PlanStep,
        domain: TaskDomain,
        complexity: Complexity,
        preferred_route: Route,
    ) -> Route {
        match step.role {
            StepRole::Researcher | StepRole::Verifier => {
                if self.local_brain_available {
                    Route::Local
                } else if matches!(preferred_route, Route::Cloud)
                    && matches!(complexity, Complexity::Complex)
                    && matches!(domain, TaskDomain::Browser | TaskDomain::Data)
                {
                    Route::Cloud
                } else {
                    Route::Ollama
                }
            }
            StepRole::Executor => match preferred_route {
                Route::Local if self.local_brain_available => Route::Local,
                Route::Local => Route::Ollama,
                Route::Ollama => Route::Ollama,
                Route::Cloud => {
                    if matches!(complexity, Complexity::Complex)
                        && matches!(
                            domain,
                            TaskDomain::Code | TaskDomain::Browser | TaskDomain::Data
                        )
                    {
                        Route::Cloud
                    } else {
                        Route::Ollama
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use sdk::{Complexity, Route, TaskDomain};

    use crate::conductor::types::{PlanStep, StepRole, StepType};

    use super::DagRoutingPolicy;

    fn make_step(role: StepRole) -> PlanStep {
        PlanStep {
            id: "step".to_string(),
            order: 0,
            step_type: match role {
                StepRole::Researcher => StepType::Research,
                StepRole::Executor => StepType::Execute,
                StepRole::Verifier => StepType::Verify,
            },
            role,
            parallel_safe: false,
            dependencies: Vec::new(),
            description: "do work".to_string(),
            expected_outcome: "done".to_string(),
        }
    }

    #[test]
    fn researcher_prefers_local_when_available() {
        let policy = DagRoutingPolicy::new(true);
        let route = policy.route_for_step(
            &make_step(StepRole::Researcher),
            TaskDomain::Code,
            Complexity::Complex,
            Route::Cloud,
        );
        assert_eq!(route, Route::Local);
    }

    #[test]
    fn verifier_falls_back_to_ollama_without_local_brain() {
        let policy = DagRoutingPolicy::new(false);
        let route = policy.route_for_step(
            &make_step(StepRole::Verifier),
            TaskDomain::General,
            Complexity::Medium,
            Route::Cloud,
        );
        assert_eq!(route, Route::Ollama);
    }

    #[test]
    fn complex_code_execution_keeps_cloud_route() {
        let policy = DagRoutingPolicy::new(false);
        let route = policy.route_for_step(
            &make_step(StepRole::Executor),
            TaskDomain::Code,
            Complexity::Complex,
            Route::Cloud,
        );
        assert_eq!(route, Route::Cloud);
    }

    #[test]
    fn medium_general_execution_stays_local_first() {
        let policy = DagRoutingPolicy::new(false);
        let route = policy.route_for_step(
            &make_step(StepRole::Executor),
            TaskDomain::General,
            Complexity::Medium,
            Route::Cloud,
        );
        assert_eq!(route, Route::Ollama);
    }
}
