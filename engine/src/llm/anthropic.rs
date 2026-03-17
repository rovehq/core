use super::{LLMError, LLMProvider, LLMResponse, Message};
use crate::config::AnthropicConfig;
use crate::secrets::SecretCache;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct AnthropicProvider {
    config: AnthropicConfig,
    secret_cache: Arc<SecretCache>,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig, secret_cache: Arc<SecretCache>) -> Self {
        Self {
            config,
            secret_cache,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn is_local(&self) -> bool {
        false
    }

    fn estimated_cost(&self, tokens: usize) -> f64 {
        // Approx $0.003 per 1k tokens for claude-3.5-sonnet
        (tokens as f64 / 1000.0) * 0.003
    }

    async fn check_health(&self) -> bool {
        self.secret_cache
            .get_secret("anthropic_api_key")
            .await
            .is_ok()
    }

    async fn generate(&self, messages: &[Message]) -> super::Result<LLMResponse> {
        let api_key = self
            .secret_cache
            .get_secret("anthropic_api_key")
            .await
            .map_err(|e| LLMError::AuthenticationFailed(e.to_string()))?;

        let url = format!("{}/messages", self.config.base_url);

        let mut system_prompt = String::new();
        let mut api_messages = Vec::new();
        for msg in messages {
            if msg.role == super::MessageRole::System {
                system_prompt.push_str(&msg.content);
                system_prompt.push('\n');
                continue;
            }
            api_messages.push(json!({
                "role": if msg.role == super::MessageRole::Assistant { "assistant" } else { "user" },
                "content": msg.content
            }));
        }

        let payload = json!({
            "model": self.config.model,
            "max_tokens": 4096,
            "system": system_prompt,
            "messages": api_messages,
        });

        let response = self
            .client
            .post(&url)
            .header("x-api-key", api_key.unsecure())
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(LLMError::AuthenticationFailed(text));
            } else if status.as_u16() == 429 {
                return Err(LLMError::RateLimitExceeded);
            } else {
                return Err(LLMError::InvalidRequest(text));
            }
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LLMError::ParseError(e.to_string()))?;

        let content_arr = data
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| LLMError::ParseError("No content array in response".to_string()))?;

        let mut full_content = String::new();
        for item in content_arr {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                full_content.push_str(text);
            }
        }

        if let Some(tool_call) = super::parse_tool_calls(&full_content) {
            return Ok(LLMResponse::ToolCall(tool_call));
        }

        Ok(LLMResponse::FinalAnswer(super::FinalAnswer::new(
            full_content,
        )))
    }
}
