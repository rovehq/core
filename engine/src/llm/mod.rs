//! LLM Provider Abstraction Layer
//!
//! This module provides a common interface for interacting with multiple LLM providers
//! (Ollama, OpenAI, Anthropic, Gemini, NVIDIA NIM). The LLMProvider trait defines
//! the contract that all providers must implement, enabling the LLM router to work
//! with multiple providers transparently.

use async_trait::async_trait;

pub mod anthropic;
pub mod gemini;
pub mod nvidia_nim;
pub mod ollama;
pub mod openai;
pub mod parser;
pub mod router;
pub mod types;

pub use parser::parse_tool_calls;
pub use types::{FinalAnswer, LLMResponse, Message, MessageRole, ToolCall, ToolSchema};

/// Result type for LLM operations
pub type Result<T> = std::result::Result<T, LLMError>;

/// Errors that can occur during LLM operations
#[derive(Debug, thiserror::Error)]
pub enum LLMError {
    #[error("Provider unavailable: {0}")]
    ProviderUnavailable(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Timeout")]
    Timeout,

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// LLM Provider trait that all providers must implement
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Returns the name of the provider (e.g., "ollama", "openai", "anthropic")
    fn name(&self) -> &str;

    /// Returns true if this is a local provider (e.g., Ollama), false for cloud providers
    fn is_local(&self) -> bool;

    /// Returns the estimated cost per 1K tokens in USD
    /// Local providers should return 0.0
    fn estimated_cost(&self, tokens: usize) -> f64;

    /// Generate a response from the LLM
    ///
    /// # Arguments
    /// * `messages` - Conversation history including system prompt, user messages, and tool results
    ///
    /// # Returns
    /// * `Ok(LLMResponse)` - Either a tool call or final answer
    /// * `Err(LLMError)` - If the request fails
    async fn generate(&self, messages: &[Message]) -> Result<LLMResponse>;

    /// Check if the provider is currently healthy and available
    /// Default implementation returns true.
    async fn check_health(&self) -> bool {
        true
    }
}
