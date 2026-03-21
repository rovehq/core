use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEnvelope {
    pub origin_node: String,
    pub target_node: String,
    pub coordinator_node: String,
    pub task_id: String,
    pub task_input: String,
    pub stream_policy: String,
}
