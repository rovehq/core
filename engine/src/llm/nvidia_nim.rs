use super::{LLMError, LLMProvider, LLMResponse, Message};
use crate::config::NvidiaNimConfig;
use crate::secrets::SecretCache;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct NvidiaNimProvider {
    config: NvidiaNimConfig,
    secret_cache: Arc<SecretCache>,
    client: reqwest::Client,
}

impl NvidiaNimProvider {
    pub fn new(config: NvidiaNimConfig, secret_cache: Arc<SecretCache>) -> Self {
        Self {
            config,
            secret_cache,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for NvidiaNimProvider {
    fn name(&self) -> &str {
        "nvidia_nim"
    }

    fn is_local(&self) -> bool {
        false
    }

    fn estimated_cost(&self, tokens: usize) -> f64 {
        // approx $0.001 per 1k tokens
        (tokens as f64 / 1000.0) * 0.001
    }

    async fn check_health(&self) -> bool {
        self.secret_cache
            .get_secret("nvidia_nim_api_key")
            .await
            .is_ok()
    }

    async fn generate(&self, messages: &[Message]) -> super::Result<LLMResponse> {
        let api_key = self
            .secret_cache
            .get_secret("nvidia_nim_api_key")
            .await
            .map_err(|e| LLMError::AuthenticationFailed(e.to_string()))?;

        let url = format!("{}/chat/completions", self.config.base_url);

        let mut api_messages = Vec::new();
        for msg in messages {
            api_messages.push(json!({
                "role": msg.role.to_string(),
                "content": msg.content
            }));
        }

        let payload = json!({
            "model": self.config.model,
            "messages": api_messages,
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key.unsecure()))
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

        let choice = data
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .ok_or_else(|| LLMError::ParseError("No choices in response".to_string()))?;

        let message = choice
            .get("message")
            .ok_or_else(|| LLMError::ParseError("No message in choice".to_string()))?;

        if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
            if let Some(tool_call) = super::parse_tool_calls(content) {
                return Ok(LLMResponse::ToolCall(tool_call));
            }
            Ok(LLMResponse::FinalAnswer(super::FinalAnswer::new(content)))
        } else {
            Err(LLMError::ParseError("Empty content".to_string()))
        }
    }
}
