use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const AGENT_SPEC_SCHEMA_VERSION: u32 = 1;
pub const WORKFLOW_SPEC_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpecRunStatus {
    Pending,
    Running,
    #[default]
    Completed,
    Failed,
    Canceled,
}

impl SpecRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpecRunStatus::Pending => "pending",
            SpecRunStatus::Running => "running",
            SpecRunStatus::Completed => "completed",
            SpecRunStatus::Failed => "failed",
            SpecRunStatus::Canceled => "canceled",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "pending" => SpecRunStatus::Pending,
            "running" => SpecRunStatus::Running,
            "completed" => SpecRunStatus::Completed,
            "failed" => SpecRunStatus::Failed,
            "canceled" => SpecRunStatus::Canceled,
            _ => SpecRunStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CapabilityRef {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SpecProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_for: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChannelBinding {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SpecProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WebhookBinding {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SpecProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWatchBinding {
    pub path: String,
    #[serde(default = "default_true")]
    pub recursive: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SpecProvenance>,
}

impl Default for FileWatchBinding {
    fn default() -> Self {
        Self {
            path: String::new(),
            recursive: true,
            events: Vec::new(),
            enabled: true,
            provenance: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePlacementPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preferred_nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_tags: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_local: bool,
    #[serde(default)]
    pub require_executor: bool,
}

impl Default for NodePlacementPolicy {
    fn default() -> Self {
        Self {
            preferred_nodes: Vec::new(),
            required_tags: Vec::new(),
            allow_local: true,
            require_executor: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentUiSchema {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeContract {
    pub success_criteria: String,
    #[serde(default = "default_outcome_max_self_evals")]
    pub max_self_evals: u32,
    #[serde(default = "default_evaluator_policy")]
    pub evaluator_policy: String,
}

impl Default for OutcomeContract {
    fn default() -> Self {
        Self {
            success_criteria: String::new(),
            max_self_evals: default_outcome_max_self_evals(),
            evaluator_policy: default_evaluator_policy(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    #[serde(default = "default_agent_spec_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub purpose: String,
    pub instructions: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<CapabilityRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<String>,
    #[serde(default = "default_memory_policy")]
    pub memory_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_profile: Option<String>,
    #[serde(default)]
    pub node_placement: NodePlacementPolicy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_contract: Option<OutcomeContract>,
    #[serde(default)]
    pub ui: AgentUiSchema,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SpecProvenance>,
}

impl Default for AgentSpec {
    fn default() -> Self {
        Self {
            schema_version: AGENT_SPEC_SCHEMA_VERSION,
            id: String::new(),
            name: String::new(),
            purpose: String::new(),
            instructions: String::new(),
            enabled: true,
            capabilities: Vec::new(),
            channels: Vec::new(),
            model_policy: None,
            memory_policy: default_memory_policy(),
            approval_mode: None,
            runtime_profile: None,
            node_placement: NodePlacementPolicy::default(),
            schedules: Vec::new(),
            output_contract: None,
            outcome_contract: None,
            ui: AgentUiSchema::default(),
            tags: Vec::new(),
            provenance: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub agent: AgentSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FactoryFieldChange {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FactoryReview {
    pub kind: String,
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
    pub target_exists: bool,
    pub review_status: String,
    pub suggested_action: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<FactoryFieldChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentFactoryResult {
    pub spec: AgentSpec,
    pub review: FactoryReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkflowFactoryResult {
    pub spec: WorkflowSpec,
    pub review: FactoryReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkerPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
    pub max_steps: u32,
    pub timeout_secs: u64,
    pub memory_budget: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBranchSpec {
    pub contains: String,
    pub next_step_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepSpec {
    pub id: String,
    pub name: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_contract: Option<OutcomeContract>,
    #[serde(default)]
    pub continue_on_error: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<WorkflowBranchSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSpec {
    #[serde(default = "default_workflow_spec_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<WorkflowStepSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub webhooks: Vec<WebhookBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_watches: Vec<FileWatchBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SpecProvenance>,
}

impl Default for WorkflowSpec {
    fn default() -> Self {
        Self {
            schema_version: WORKFLOW_SPEC_SCHEMA_VERSION,
            id: String::new(),
            name: String::new(),
            description: String::new(),
            enabled: true,
            steps: Vec::new(),
            channels: Vec::new(),
            webhooks: Vec::new(),
            file_watches: Vec::new(),
            schedules: Vec::new(),
            runtime_profile: None,
            output_contract: None,
            tags: Vec::new(),
            provenance: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskExecutionProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_preset_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_contract: Option<OutcomeContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub run_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<String>,
    pub status: SpecRunStatus,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub run_id: String,
    pub workflow_id: String,
    pub status: SpecRunStatus,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub steps_total: i64,
    pub steps_completed: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step_name: Option<String>,
    #[serde(default)]
    pub retry_count: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_task_id: Option<String>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub resumable: bool,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunStepRecord {
    pub run_id: String,
    pub step_index: i64,
    pub step_id: String,
    pub step_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_preset: Option<String>,
    pub status: SpecRunStatus,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub attempt_count: i64,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunDetail {
    pub run: WorkflowRunRecord,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<WorkflowRunStepRecord>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_memory_policy() -> String {
    "default".to_string()
}

fn default_outcome_max_self_evals() -> u32 {
    1
}

fn default_evaluator_policy() -> String {
    "self_check".to_string()
}

fn default_agent_spec_schema_version() -> u32 {
    AGENT_SPEC_SCHEMA_VERSION
}

fn default_workflow_spec_schema_version() -> u32 {
    WORKFLOW_SPEC_SCHEMA_VERSION
}
