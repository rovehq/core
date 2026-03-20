use sdk::Route;

use super::types::{PlanStep, StepRole};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepExecutionPolicy {
    pub preferred_route: Route,
    pub allow_writes: bool,
    pub require_evidence: bool,
    pub parallel_safe: bool,
}

impl StepExecutionPolicy {
    pub fn for_step(step: &PlanStep, preferred_route: Route) -> Self {
        match step.role {
            StepRole::Researcher => Self {
                preferred_route,
                allow_writes: false,
                require_evidence: true,
                parallel_safe: true,
            },
            StepRole::Executor => Self {
                preferred_route,
                allow_writes: true,
                require_evidence: false,
                parallel_safe: step.parallel_safe,
            },
            StepRole::Verifier => Self {
                preferred_route,
                allow_writes: false,
                require_evidence: true,
                parallel_safe: true,
            },
        }
    }

    pub fn role_instructions(&self, role: &StepRole) -> &'static str {
        match role {
            StepRole::Researcher => {
                "You are the researcher specialist. Stay read-only, gather concrete evidence, and do not mutate the workspace or external systems."
            }
            StepRole::Executor => {
                "You are the executor specialist. Make only the minimum required changes, prefer deterministic actions, and keep mutations tightly scoped to the requested outcome."
            }
            StepRole::Verifier => {
                "You are the verifier specialist. Stay read-only, validate prior work with concrete checks, and call out gaps or failures instead of guessing success."
            }
        }
    }

    pub fn system_prompt(&self, step: &PlanStep, context: &str) -> String {
        let write_rule = if self.allow_writes {
            "Writes are allowed only when they are necessary to complete this step."
        } else {
            "Writes are not allowed for this step."
        };
        let evidence_rule = if self.require_evidence {
            "Base the result on concrete evidence from the available context and tools."
        } else {
            "Finish the requested action directly and summarize the outcome precisely."
        };

        format!(
            "{}\n{}\n{}\nPreferred route: {:?}\nStep description: {}\nExpected outcome: {}\n\nDependency context:\n{}",
            self.role_instructions(&step.role),
            write_rule,
            evidence_rule,
            self.preferred_route,
            step.description,
            step.expected_outcome,
            context
        )
    }
}

#[cfg(test)]
mod tests {
    use sdk::Route;

    use crate::conductor::types::{PlanStep, StepRole, StepType};

    use super::StepExecutionPolicy;

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
            description: "do the thing".to_string(),
            expected_outcome: "done".to_string(),
        }
    }

    #[test]
    fn researcher_policy_is_read_only() {
        let policy = StepExecutionPolicy::for_step(&make_step(StepRole::Researcher), Route::Local);
        assert!(!policy.allow_writes);
        assert!(policy.require_evidence);
        assert!(policy.parallel_safe);
    }

    #[test]
    fn executor_policy_allows_writes() {
        let policy = StepExecutionPolicy::for_step(&make_step(StepRole::Executor), Route::Cloud);
        assert!(policy.allow_writes);
        assert!(!policy.require_evidence);
    }

    #[test]
    fn verifier_prompt_mentions_read_only() {
        let prompt =
            StepExecutionPolicy::for_step(&make_step(StepRole::Verifier), Route::Ollama)
                .system_prompt(&make_step(StepRole::Verifier), "prior output");
        assert!(prompt.contains("verifier specialist"));
        assert!(prompt.contains("Writes are not allowed"));
        assert!(prompt.contains("concrete evidence"));
    }
}
