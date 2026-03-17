use super::{LLMError, LLMProvider, LLMResponse, Message};
use crate::config::GeminiConfig;
use crate::secrets::SecretCache;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct GeminiProvider {
    config: GeminiConfig,
    secret_cache: Arc<SecretCache>,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(config: GeminiConfig, secret_cache: Arc<SecretCache>) -> Self {
        Self {
            config,
            secret_cache,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn is_local(&self) -> bool {
        false
    }

    fn estimated_cost(&self, tokens: usize) -> f64 {
        // approx $0.001 per 1k tokens
        (tokens as f64 / 1000.0) * 0.001
    }

    async fn check_health(&self) -> bool {
        self.secret_cache.get_secret("gemini_api_key").await.is_ok()
    }

    async fn generate(&self, messages: &[Message]) -> super::Result<LLMResponse> {
        let api_key = self
            .secret_cache
            .get_secret("gemini_api_key")
            .await
            .map_err(|e| LLMError::AuthenticationFailed(e.to_string()))?;

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.config.base_url,
            self.config.model,
            api_key.unsecure()
        );

        let mut contents = Vec::new();
        let mut system_instruction = None;

        for msg in messages {
            if msg.role == super::MessageRole::System {
                system_instruction = Some(json!({
                    "parts": [{"text": msg.content}]
                }));
                continue;
            }

            contents.push(json!({
                "role": if msg.role == super::MessageRole::Assistant { "model" } else { "user" },
                "parts": [{"text": msg.content}]
            }));
        }

        let mut payload = serde_json::Map::new();
        payload.insert("contents".to_string(), json!(contents));

        if let Some(sys) = system_instruction {
            payload.insert("systemInstruction".to_string(), sys);
        }

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LLMError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();

            if status.as_u16() == 400 || status.as_u16() == 404 {
                return Err(LLMError::InvalidRequest(text));
            } else if status.as_u16() == 429 {
                return Err(LLMError::RateLimitExceeded);
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(LLMError::AuthenticationFailed(text));
            } else {
                return Err(LLMError::ProviderUnavailable(format!(
                    "Gemini API error ({}): {}",
                    status, text
                )));
            }
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LLMError::ParseError(e.to_string()))?;

        let candidate = data
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .ok_or_else(|| LLMError::ParseError("No candidates in response".to_string()))?;

        let content_item = candidate
            .get("content")
            .ok_or_else(|| LLMError::ParseError("No content in candidate".to_string()))?;

        let parts = content_item
            .get("parts")
            .and_then(|p| p.as_array())
            .ok_or_else(|| LLMError::ParseError("No parts in candidate content".to_string()))?;

        let mut full_text = String::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                full_text.push_str(text);
            }
        }

        if let Some(tool_call) = super::parse_tool_calls(&full_text) {
            return Ok(LLMResponse::ToolCall(tool_call));
        }

        Ok(LLMResponse::FinalAnswer(super::FinalAnswer::new(full_text)))
    }
}
