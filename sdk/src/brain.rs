//! Brain system types and traits
//!
//! Defines the Brain trait for local inference and dispatch classification types.

use crate::errors::EngineError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Task classification for domain-gated memory queries and routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskDomain {
    Code,
    Git,
    Shell,
    #[default]
    General,
    Browser,
    Data,
}

impl std::fmt::Display for TaskDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Task complexity classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Complexity {
    #[default]
    Simple, // Single-step, direct execution
    Medium,  // Multi-step, sequential
    Complex, // Requires DAG, parallel execution
}

/// LLM routing decision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Route {
    Local, // Use local reasoning brain
    #[default]
    Ollama, // Use Ollama if available
    Cloud, // Use cloud provider
}

/// Tool category tags for filtering
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolTag {
    Filesystem,
    Terminal,
    Git,
    Network,
    Vision,
    Telegram,
    Browser,
    Data,
}

/// Result of dispatch brain classification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DispatchResult {
    pub domain: TaskDomain,
    pub complexity: Complexity,
    pub injection_score: f32,
    pub sensitive: bool,
    pub route: Route,
    pub tools_needed: Vec<ToolTag>,
}

impl Default for DispatchResult {
    fn default() -> Self {
        Self {
            domain: TaskDomain::General,
            complexity: Complexity::Simple,
            injection_score: 0.0,
            sensitive: false,
            route: Route::Ollama,
            tools_needed: vec![],
        }
    }
}

/// Message for LLM completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Tool schema for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Response from brain completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainResponse {
    pub content: String,
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

/// Brain trait for local inference
///
/// Both OllamaProvider and LocalBrain implement this trait,
/// allowing them to be used interchangeably.
#[async_trait]
pub trait Brain: Send + Sync {
    /// Returns the brain's identifier
    fn name(&self) -> &str;

    /// Complete a prompt with optional tool calling
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<BrainResponse, EngineError>;
}
