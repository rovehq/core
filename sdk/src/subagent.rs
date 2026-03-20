use serde::{Deserialize, Serialize};

/// Configuration for an independently constrained subagent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentSpec {
    pub role: SubagentRole,
    pub task: String,
    pub tools_allowed: Vec<String>,
    pub memory_budget: usize,
    pub model_override: Option<String>,
    pub max_steps: u32,
    pub timeout_secs: u64,
}

impl Default for SubagentSpec {
    fn default() -> Self {
        Self {
            role: SubagentRole::Executor,
            task: String::new(),
            tools_allowed: Vec::new(),
            memory_budget: 800,
            model_override: None,
            max_steps: 8,
            timeout_secs: 120,
        }
    }
}

/// Role-specific subagent execution profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubagentRole {
    Researcher,
    Executor,
    Verifier,
    Summariser,
    Custom(String),
}

impl SubagentRole {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Researcher => "researcher",
            Self::Executor => "executor",
            Self::Verifier => "verifier",
            Self::Summariser => "summariser",
            Self::Custom(role) => role.as_str(),
        }
    }
}
