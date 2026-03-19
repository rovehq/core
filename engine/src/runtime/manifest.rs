use serde::{Deserialize, Serialize};

pub const SDK_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub sdk_version: String,
    pub plugin_type: PluginType,
    pub permissions: Permissions,
    pub trust_tier: TrustTier,
    pub min_model: Option<ModelRequirement>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PluginType {
    Skill,
    Channel,
    Brain,
    Workspace,
    Mcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permissions {
    pub filesystem: Vec<PathPattern>,
    pub network: Vec<DomainPattern>,
    pub memory_read: bool,
    pub memory_write: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum TrustTier {
    Official = 0,
    Reviewed = 1,
    Community = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequirement {
    pub min_context_tokens: Option<u32>,
    pub requires_tool_calls: bool,
    pub requires_vision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathPattern(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainPattern(pub String);
