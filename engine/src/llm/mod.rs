//! LLM Provider Abstraction Layer
//!
//! This module provides a common interface for interacting with multiple LLM providers
//! (Ollama, OpenAI, Anthropic, Gemini, NVIDIA NIM). The LLMProvider trait defines
//! the contract that all providers must implement, enabling the LLM router to work
//! with multiple providers transparently.

use std::sync::Mutex;

use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

pub mod anthropic;
pub mod custom;
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

// ── Global streaming sink ─────────────────────────────────────────────────────
// When `--stream` is active for a CLI run, the CLI sets this sink before
// execution. LLM providers that support streaming emit token chunks here.

static STREAM_SINK: Mutex<Option<UnboundedSender<String>>> = Mutex::new(None);

/// Set the global token-streaming sink. Called by the CLI before a streamed run.
pub fn set_stream_sink(tx: UnboundedSender<String>) {
    if let Ok(mut guard) = STREAM_SINK.lock() {
        *guard = Some(tx);
    }
}

/// Clear the global streaming sink. Called by the CLI after a streamed run.
pub fn clear_stream_sink() {
    if let Ok(mut guard) = STREAM_SINK.lock() {
        *guard = None;
    }
}

/// Emit one token chunk to the streaming sink, if one is registered.
pub(crate) fn emit_stream_chunk(chunk: &str) {
    if let Ok(guard) = STREAM_SINK.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(chunk.to_string());
        }
    }
}

/// Returns true when a streaming sink is registered (i.e. caller wants streaming).
pub(crate) fn streaming_active() -> bool {
    STREAM_SINK
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
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
