use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    Uninitialized,
    Locked,
    Unlocked,
    ReauthRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub access_token: String,
    pub expires_in_secs: u64,
    pub absolute_expires_in_secs: u64,
    pub reauth_required_for: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthStatus {
    pub state: AuthState,
    pub idle_expires_in_secs: Option<u64>,
    pub absolute_expires_in_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonHello {
    pub version: String,
    pub daemon_running: bool,
    pub auth_state: AuthState,
    pub node: NodeSummary,
    pub capabilities: DaemonCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSummary {
    pub node_id: String,
    pub node_name: String,
    pub role: NodeExecutionRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DaemonCapabilities {
    pub brains: Vec<String>,
    pub services: Vec<String>,
    pub extensions: Vec<String>,
}

/// Public kind for installable Rove capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionKind {
    Skill,
    System,
    Connector,
    Channel,
}

impl ExtensionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::System => "system",
            Self::Connector => "connector",
            Self::Channel => "channel",
        }
    }
}

/// Public kind for optional daemon services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceKind {
    Logging,
    WebUi,
    Remote,
    ConnectorEngine,
}

impl ServiceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Logging => "logging",
            Self::WebUi => "webui",
            Self::Remote => "remote",
            Self::ConnectorEngine => "connector-engine",
        }
    }
}

/// Public family for installed brains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrainFamily {
    Dispatch,
    Reasoning,
    Embedding,
    Rerank,
    Vision,
}

impl BrainFamily {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Reasoning => "reasoning",
            Self::Embedding => "embedding",
            Self::Rerank => "rerank",
            Self::Vision => "vision",
        }
    }
}

/// Scope for policy files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyScope {
    User,
    Workspace,
    Project,
}

impl PolicyScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Workspace => "workspace",
            Self::Project => "project",
        }
    }
}

/// Top-level task execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunMode {
    Serial,
    Parallel,
}

/// Explicit workspace isolation mode for top-level tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunIsolation {
    None,
    Worktree,
    Snapshot,
}

/// Stable identifier for a top-level run context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunContextId(pub String);

/// Stable identity for a remote node in the mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub node_id: String,
    pub node_name: String,
    pub public_key: String,
}

/// Execution profile for a remote node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeExecutionRole {
    Full,
    ExecutorOnly,
}

/// Capability profile advertised by a remote node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeProfile {
    pub capabilities: Vec<String>,
    pub tags: Vec<String>,
    pub execution_role: NodeExecutionRole,
}

/// Envelope sent between remote daemons for coordinated execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteEnvelope {
    pub origin_node: String,
    pub target_node: String,
    pub coordinator_node: String,
    pub task_id: String,
    pub task_input: String,
    pub stream_policy: String,
    pub execution_plan: Option<RemoteExecutionPlan>,
}

/// Coordinator-computed direct execution plan for executor-oriented remote nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteExecutionPlan {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<RemoteExecutionStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<Value>,
    pub domain_hint: Option<String>,
}

/// One ordered execution step inside a remote direct-execution bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteExecutionStep {
    pub summary: String,
    pub tool_name: String,
    pub tool_args: Value,
}

impl RemoteExecutionPlan {
    pub fn direct(
        summary: impl Into<String>,
        tool_name: impl Into<String>,
        tool_args: Value,
        domain_hint: Option<String>,
    ) -> Self {
        let summary = summary.into();
        let tool_name = tool_name.into();
        let step = RemoteExecutionStep {
            summary: summary.clone(),
            tool_name: tool_name.clone(),
            tool_args: tool_args.clone(),
        };

        Self {
            summary,
            steps: vec![step],
            tool_name: Some(tool_name),
            tool_args: Some(tool_args),
            domain_hint,
        }
    }

    pub fn append_step(
        &mut self,
        summary: impl Into<String>,
        tool_name: impl Into<String>,
        tool_args: Value,
    ) {
        self.steps.push(RemoteExecutionStep {
            summary: summary.into(),
            tool_name: tool_name.into(),
            tool_args,
        });
    }

    pub fn steps(&self) -> Vec<RemoteExecutionStep> {
        if !self.steps.is_empty() {
            return self.steps.clone();
        }

        match self.tool_name.as_ref() {
            Some(tool_name) => vec![RemoteExecutionStep {
                summary: self.summary.clone(),
                tool_name: tool_name.clone(),
                tool_args: self.tool_args.clone().unwrap_or(Value::Null),
            }],
            None => Vec::new(),
        }
    }

    pub fn primary_tool_name(&self) -> Option<&str> {
        self.steps
            .first()
            .map(|step| step.tool_name.as_str())
            .or(self.tool_name.as_deref())
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.tool_name.is_none()
    }
}
