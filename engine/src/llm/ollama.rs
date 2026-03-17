//! Ollama LLM Provider
//!
//! This module implements the LLMProvider trait for Ollama, a local LLM provider.
//! Ollama runs models locally on the user's machine, typically at http://localhost:11434.
//!
//! Key features:
//! - Local execution (no API keys required)
//! - Zero cost (is_local() returns true, estimated_cost() returns 0.0)
//! - SSE streaming support
//! - Tool call format handling
//! - Error mapping to EngineError

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::{FinalAnswer, LLMError, LLMProvider, LLMResponse, Message, MessageRole, Result};
use sdk::{Brain, BrainResponse, EngineError, Message as BrainMessage, ToolSchema};

/// Ollama provider configuration
#[derive(Debug, Clone)]
pub struct OllamaProvider {
    /// Base URL for Ollama API (typically http://localhost:11434)
    base_url: String,

    /// Model name to use (e.g., "llama3.1:8b")
    model: String,

    /// HTTP client for API requests
    client: Client,
}

impl OllamaProvider {
    /// Create a new Ollama provider
    ///
    /// # Arguments
    /// * `base_url` - Base URL for Ollama API (e.g., "http://localhost:11434")
    /// * `model` - Model name to use (e.g., "llama3.1:8b")
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> std::result::Result<Self, sdk::errors::EngineError> {
        Ok(Self {
            base_url: base_url.into(),
            model: model.into(),
            client: Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .map_err(|e| sdk::errors::EngineError::LLMProvider(format!("Ollama client error: {}", e)))?,
        })
    }

    /// Convert our Message format to Ollama's format
    fn convert_messages(&self, messages: &[Message]) -> Vec<OllamaMessage> {
        messages
            .iter()
            .map(|msg| OllamaMessage {
                role: match msg.role {
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::System => "system".to_string(),
                    MessageRole::Tool => "tool".to_string(),
                },
                content: msg.content.clone(),
            })
            .collect()
    }
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn is_local(&self) -> bool {
        true
    }

    fn estimated_cost(&self, _tokens: usize) -> f64 {
        0.0 // Local provider, no cost
    }

    async fn check_health(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));

        let response = match tokio::time::timeout(
            Duration::from_secs(2),
            self.client.get(&url).send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            _ => return false,
        };

        if !response.status().is_success() {
            return false;
        }

        let tags: OllamaTagsResponse = match response.json().await {
            Ok(tags) => tags,
            Err(_) => return false,
        };

        let configured = self.model.to_lowercase();
        tags.models.iter().any(|m| {
            let model_name = m.name.to_lowercase();
            model_name == configured
                || model_name.trim_end_matches(":latest") == configured.trim_end_matches(":latest")
        })
    }

    async fn generate(&self, messages: &[Message]) -> Result<LLMResponse> {
        // Convert messages to Ollama format
        let ollama_messages = self.convert_messages(messages);

        tracing::debug!(
            "Ollama request: model={}, messages={}, total_chars={}",
            self.model,
            ollama_messages.len(),
            ollama_messages
                .iter()
                .map(|m| m.content.len())
                .sum::<usize>()
        );

        // Build request
        let request = OllamaRequest {
            model: self.model.clone(),
            messages: ollama_messages,
            stream: false, // For now, use non-streaming mode
        };

        // Make API call
        let url = format!("{}/api/chat", self.base_url);
        let start = std::time::Instant::now();
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LLMError::Timeout
                } else if e.is_connect() {
                    LLMError::ProviderUnavailable(format!(
                        "Cannot connect to Ollama at {}. Is Ollama running?",
                        self.base_url
                    ))
                } else {
                    LLMError::NetworkError(e.to_string())
                }
            })?;

        tracing::info!(
            "Ollama response received in {:.1}s",
            start.elapsed().as_secs_f64()
        );

        // Check response status
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(LLMError::ProviderUnavailable(format!(
                "Ollama API error ({}): {}",
                status, error_text
            )));
        }

        // Parse response
        let ollama_response: OllamaResponse = response
            .json()
            .await
            .map_err(|e| LLMError::ParseError(format!("Failed to parse Ollama response: {}", e)))?;

        // Extract content from response
        let content = ollama_response.message.content;

        // Check if this is a tool call or final answer
        if let Some(tool_call) = super::parse_tool_calls(&content) {
            Ok(LLMResponse::ToolCall(tool_call))
        } else {
            Ok(LLMResponse::FinalAnswer(FinalAnswer::new(content)))
        }
    }
}

/// Ollama API request format
#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

/// Ollama message format
#[derive(Debug, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

/// Ollama API response format
#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    #[allow(dead_code)]
    done: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    name: String,
}

// Implement Brain trait for OllamaProvider
// This allows OllamaProvider to be used as a reasoning brain
#[async_trait]
impl Brain for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn complete(
        &self,
        system: &str,
        messages: &[BrainMessage],
        _tools: &[ToolSchema],
    ) -> std::result::Result<BrainResponse, EngineError> {
        // Convert Brain messages to LLM messages
        let mut llm_messages = vec![Message {
            role: MessageRole::System,
            content: system.to_string(),
            tool_call_id: None,
        }];

        for msg in messages {
            let role = match msg.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                "tool" => MessageRole::Tool,
                _ => MessageRole::User,
            };
            llm_messages.push(Message {
                role,
                content: msg.content.clone(),
                tool_call_id: None,
            });
        }

        // Call LLMProvider::generate
        match self.generate(&llm_messages).await {
            Ok(LLMResponse::FinalAnswer(answer)) => Ok(BrainResponse {
                content: answer.content,
                tool_calls: None,
            }),
            Ok(LLMResponse::ToolCall(tool_call)) => {
                // Convert tool call to JSON format expected by Brain trait
                let tool_call_json = serde_json::json!({
                    "id": tool_call.id,
                    "function": {
                        "name": tool_call.name,
                        "arguments": tool_call.arguments,
                    }
                });
                Ok(BrainResponse {
                    content: String::new(),
                    tool_calls: Some(vec![tool_call_json]),
                })
            }
            Err(e) => Err(EngineError::LLMProvider(format!("Ollama error: {}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::parse_tool_calls;

    #[test]
    fn test_ollama_provider_properties() {
        let provider = OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap();

        assert_eq!(LLMProvider::name(&provider), "ollama");
        assert!(provider.is_local());
        assert_eq!(provider.estimated_cost(1000), 0.0);
        assert_eq!(provider.estimated_cost(10000), 0.0);
    }

    #[test]
    fn test_message_conversion() {
        let provider = OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap();

        let messages = vec![
            Message::system("You are a helpful assistant"),
            Message::user("Hello"),
            Message::assistant("Hi there"),
        ];

        let ollama_messages = provider.convert_messages(&messages);

        assert_eq!(ollama_messages.len(), 3);
        assert_eq!(ollama_messages[0].role, "system");
        assert_eq!(ollama_messages[1].role, "user");
        assert_eq!(ollama_messages[2].role, "assistant");
    }

    #[test]
    fn test_parse_tool_calls_json_format() {
        let _provider = OllamaProvider::new("http://localhost:11434", "llama3.1:8b");

        let content = r#"{"function": "read_file", "arguments": {"path": "test.txt"}}"#;
        let tool_call = parse_tool_calls(content);

        assert!(tool_call.is_some());
        let tool_call = tool_call.unwrap();
        assert_eq!(tool_call.name, "read_file");
        assert!(tool_call.arguments.contains("path"));
    }

    #[test]
    fn test_parse_tool_calls_marker_format() {
        let _provider = OllamaProvider::new("http://localhost:11434", "llama3.1:8b");

        let content = r#"<tool_call>read_file({"path": "test.txt"})</tool_call>"#;
        let tool_call = parse_tool_calls(content);

        assert!(tool_call.is_some());
        let tool_call = tool_call.unwrap();
        assert_eq!(tool_call.name, "read_file");
        assert!(tool_call.arguments.contains("path"));
    }

    #[test]
    fn test_parse_tool_calls_no_match() {
        let _provider = OllamaProvider::new("http://localhost:11434", "llama3.1:8b");

        let content = "This is just a regular response";
        let tool_call = parse_tool_calls(content);

        assert!(tool_call.is_none());
    }
}
