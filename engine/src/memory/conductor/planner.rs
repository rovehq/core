//! Conductor Planner
//!
//! Interacts with the LLM to generate, refine, and orchestrate `ConductorPlan`s
//! based on user requests and available tools.

use crate::conductor::types::{ConductorPlan, PlanStep, StepRole, StepType};

use crate::llm::{LLMProvider, Message};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Planner {
    llm: Arc<Box<dyn LLMProvider>>,
}

/// Intermediate deserialization type for LLM JSON output
#[derive(Debug, Deserialize)]
struct RawPlanStep {
    id: Option<String>,
    description: String,
    step_type: Option<String>,
    role: Option<String>,
    parallel_safe: Option<bool>,
    #[serde(default)]
    dependencies: Vec<String>,
    expected_outcome: Option<String>,
}

impl Planner {
    pub fn new(llm: Arc<Box<dyn LLMProvider>>) -> Self {
        Self { llm }
    }

    /// Generate an initial plan based on a user goal
    pub async fn generate_plan(&self, goal: &str) -> Result<ConductorPlan> {
        let system_prompt = Message::system(
            "You are the Conductor Planner. Break down the user's goal into discrete steps.\n\
            Output ONLY a JSON array of steps. Each step object must have:\n\
            - \"description\": string describing what to do\n\
            - \"step_type\": one of \"Research\", \"Execute\", or \"Verify\"\n\
            - \"role\": one of \"Researcher\", \"Executor\", or \"Verifier\"\n\
            - \"parallel_safe\": boolean, true only for read-only gather or verification steps\n\
            - \"dependencies\": array of step ids this depends on (empty for first step)\n\
            - \"expected_outcome\": string describing success criteria\n\n\
            Example output:\n\
            [{\"description\":\"Analyze the codebase\",\"step_type\":\"Research\",\"role\":\"Researcher\",\"parallel_safe\":true,\"dependencies\":[],\"expected_outcome\":\"Understanding of code structure\"},\
            {\"description\":\"Implement changes\",\"step_type\":\"Execute\",\"role\":\"Executor\",\"parallel_safe\":false,\"dependencies\":[\"step_1\"],\"expected_outcome\":\"Code modified\"},\
            {\"description\":\"Run tests\",\"step_type\":\"Verify\",\"role\":\"Verifier\",\"parallel_safe\":true,\"dependencies\":[\"step_2\"],\"expected_outcome\":\"All tests pass\"}]\n\n\
            Output ONLY the JSON array, no markdown, no explanation."
        );
        let user_prompt = Message::user(goal);

        let response = self.llm.generate(&[system_prompt, user_prompt]).await?;

        let content = match &response {
            crate::llm::LLMResponse::FinalAnswer(a) => &a.content,
            crate::llm::LLMResponse::ToolCall(_) => {
                // LLM tried to call a tool instead of planning — fall back to default plan
                return Ok(self.default_plan(goal));
            }
        };

        // Try to parse the LLM response as JSON steps
        match self.parse_steps(content) {
            Ok(steps) if !steps.is_empty() => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                Ok(ConductorPlan {
                    id: uuid::Uuid::new_v4().to_string(),
                    original_goal: goal.to_string(),
                    mode: Default::default(),
                    stages: vec![],
                    steps,
                    created_at: now,
                })
            }
            _ => {
                tracing::warn!("Failed to parse LLM plan output, using default plan");
                Ok(self.default_plan(goal))
            }
        }
    }

    /// Parse LLM output into PlanSteps, handling various JSON formats
    fn parse_steps(&self, content: &str) -> Result<Vec<PlanStep>> {
        let trimmed = content.trim();

        // Try to extract JSON array from the response
        let json_str = if let Some(start) = trimmed.find('[') {
            if let Some(end) = trimmed.rfind(']') {
                &trimmed[start..=end]
            } else {
                trimmed
            }
        } else {
            trimmed
        };

        let raw_steps: Vec<RawPlanStep> =
            serde_json::from_str(json_str).context("Failed to parse plan steps JSON")?;

        let steps = raw_steps
            .into_iter()
            .enumerate()
            .map(|(i, raw)| {
                let step_id = raw.id.unwrap_or_else(|| format!("step_{}", i + 1));

                let step_type = match raw.step_type.as_deref().unwrap_or("Execute") {
                    "Research" | "research" => StepType::Research,
                    "Verify" | "verify" => StepType::Verify,
                    _ => StepType::Execute,
                };
                let role = match raw.role.as_deref().unwrap_or("") {
                    "Researcher" | "researcher" => StepRole::Researcher,
                    "Verifier" | "verifier" => StepRole::Verifier,
                    "Executor" | "executor" => StepRole::Executor,
                    _ => StepRole::for_step_type(&step_type),
                };
                let parallel_safe = raw.parallel_safe.unwrap_or(matches!(
                    &step_type,
                    StepType::Research | StepType::Verify
                ));

                PlanStep {
                    id: step_id,
                    order: i as u32,
                    description: raw.description,
                    step_type,
                    role,
                    parallel_safe,
                    dependencies: raw.dependencies,
                    expected_outcome: raw
                        .expected_outcome
                        .unwrap_or_else(|| "Step completed".to_string()),
                }
            })
            .collect();

        Ok(steps)
    }

    /// Generate a default 3-step plan when LLM parsing fails
    fn default_plan(&self, goal: &str) -> ConductorPlan {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        ConductorPlan {
            id: uuid::Uuid::new_v4().to_string(),
            original_goal: goal.to_string(),
            mode: Default::default(),
            stages: vec![],
            steps: vec![
                PlanStep {
                    id: "step_1".to_string(),
                    order: 0,
                    description: format!("Analyze how to achieve: {}", goal),
                    step_type: StepType::Research,
                    role: StepRole::Researcher,
                    parallel_safe: true,
                    dependencies: vec![],
                    expected_outcome: "Understanding of required changes".to_string(),
                },
                PlanStep {
                    id: "step_2".to_string(),
                    order: 1,
                    description: "Implement the required changes".to_string(),
                    step_type: StepType::Execute,
                    role: StepRole::Executor,
                    parallel_safe: false,
                    dependencies: vec!["step_1".to_string()],
                    expected_outcome: "Changes implemented successfully".to_string(),
                },
                PlanStep {
                    id: "step_3".to_string(),
                    order: 2,
                    description: "Verify the implementation".to_string(),
                    step_type: StepType::Verify,
                    role: StepRole::Verifier,
                    parallel_safe: true,
                    dependencies: vec!["step_2".to_string()],
                    expected_outcome: "Tests pass and functionality is confirmed".to_string(),
                },
            ],
            created_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_steps_valid_json() {
        use crate::llm::ollama::OllamaProvider;

        let provider: Box<dyn LLMProvider> =
            Box::new(OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap());
        let planner = Planner::new(Arc::new(provider));

        let json = r#"[
            {"description": "Read the config file", "step_type": "Research", "role": "Researcher", "parallel_safe": true, "dependencies": [], "expected_outcome": "Config understood"},
            {"description": "Modify the settings", "step_type": "Execute", "role": "Executor", "parallel_safe": false, "dependencies": ["step_1"], "expected_outcome": "Settings changed"},
            {"description": "Run validation", "step_type": "Verify", "role": "Verifier", "parallel_safe": true, "dependencies": ["step_2"], "expected_outcome": "Config valid"}
        ]"#;

        let steps = planner.parse_steps(json).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].id, "step_1");
        assert_eq!(steps[0].step_type, StepType::Research);
        assert_eq!(steps[0].role, StepRole::Researcher);
        assert!(steps[0].parallel_safe);
        assert_eq!(steps[1].step_type, StepType::Execute);
        assert_eq!(steps[1].role, StepRole::Executor);
        assert_eq!(steps[2].step_type, StepType::Verify);
        assert_eq!(steps[2].role, StepRole::Verifier);
        assert_eq!(steps[1].dependencies, vec!["step_1"]);
    }

    #[test]
    fn test_parse_steps_with_markdown_wrapper() {
        use crate::llm::ollama::OllamaProvider;

        let provider: Box<dyn LLMProvider> =
            Box::new(OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap());
        let planner = Planner::new(Arc::new(provider));

        let json = r#"Here is the plan:
        [{"description": "Do the thing", "step_type": "Execute", "role": "Executor", "parallel_safe": false, "dependencies": [], "expected_outcome": "Done"}]
        Hope this helps!"#;

        let steps = planner.parse_steps(json).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].description, "Do the thing");
    }

    #[test]
    fn test_parse_steps_missing_optional_fields() {
        use crate::llm::ollama::OllamaProvider;

        let provider: Box<dyn LLMProvider> =
            Box::new(OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap());
        let planner = Planner::new(Arc::new(provider));

        let json = r#"[{"description": "Minimal step"}]"#;

        let steps = planner.parse_steps(json).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].step_type, StepType::Execute); // default
        assert_eq!(steps[0].role, StepRole::Executor); // derived default
        assert!(!steps[0].parallel_safe); // writes default to serial
        assert_eq!(steps[0].expected_outcome, "Step completed"); // default
    }

    #[test]
    fn test_default_plan() {
        use crate::llm::ollama::OllamaProvider;

        let provider: Box<dyn LLMProvider> =
            Box::new(OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap());
        let planner = Planner::new(Arc::new(provider));

        let plan = planner.default_plan("Fix the bug");
        assert_eq!(plan.steps.len(), 3);
        assert!(plan.original_goal.contains("Fix the bug"));
        assert!(plan.created_at > 0);
        assert_eq!(plan.steps[0].step_type, StepType::Research);
        assert_eq!(plan.steps[0].role, StepRole::Researcher);
        assert!(plan.steps[0].parallel_safe);
        assert_eq!(plan.steps[1].step_type, StepType::Execute);
        assert_eq!(plan.steps[1].role, StepRole::Executor);
        assert!(!plan.steps[1].parallel_safe);
        assert_eq!(plan.steps[2].step_type, StepType::Verify);
        assert_eq!(plan.steps[2].role, StepRole::Verifier);
        assert!(plan.steps[2].parallel_safe);
    }
}
