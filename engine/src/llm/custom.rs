use super::{LLMError, LLMProvider, LLMResponse, Message};
use crate::secrets::SecretCache;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// A custom LLM provider that wraps user-configured endpoints.
///
/// Unlike the built-in providers, this provider:
/// - Returns the user-defined name from `name()` so the router can route to it by name
/// - Optionally omits the Authorization header when `api_key_name` is None or empty
///   (useful for local proxies that don't require authentication)
/// - Supports `openai` and `anthropic` wire protocols
pub struct CustomLLMProvider {
    /// User-defined name (e.g. "kiro-proxy", "my-llm")
    pub provider_name: String,
    /// Wire protocol: "openai" or "anthropic"
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    /// Keychain entry name, or empty string / None for keyless providers
    pub api_key_name: Option<String>,
    secret_cache: Arc<SecretCache>,
    client: reqwest::Client,
}

impl CustomLLMProvider {
    pub fn new(
        provider_name: String,
        protocol: String,
        base_url: String,
        model: String,
        api_key_name: Option<String>,
        secret_cache: Arc<SecretCache>,
    ) -> Self {
        Self {
            provider_name,
            protocol,
            base_url,
            model,
            api_key_name,
            secret_cache,
            client: reqwest::Client::new(),
        }
    }

    async fn get_api_key(&self) -> Option<String> {
        let key_name = self.api_key_name.as_deref()?;
        if key_name.is_empty() {
            return None;
        }
        self.secret_cache
            .get_secret(key_name)
            .await
            .ok()
            .map(|s| s.unsecure().to_string())
    }

    async fn generate_openai(&self, messages: &[Message]) -> super::Result<LLMResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let api_messages: Vec<_> = messages
            .iter()
            .map(|msg| {
                json!({
                    "role": msg.role.to_string(),
                    "content": msg.content
                })
            })
            .collect();

        let payload = json!({
            "model": self.model,
            "messages": api_messages,
        });

        let api_key = self.get_api_key().await;

        let mut req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload);

        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req
            .send()
            .await
            .map_err(|e| LLMError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => LLMError::AuthenticationFailed(text),
                429 => LLMError::RateLimitExceeded,
                _ => LLMError::InvalidRequest(text),
            });
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

        let content = choice
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| LLMError::ParseError("No content in choice".to_string()))?;

        if let Some(tool_call) = super::parse_tool_calls(content) {
            return Ok(LLMResponse::ToolCall(tool_call));
        }
        Ok(LLMResponse::FinalAnswer(super::FinalAnswer::new(content)))
    }

    async fn generate_anthropic(&self, messages: &[Message]) -> super::Result<LLMResponse> {
        let url = format!("{}/messages", self.base_url);
        let api_key = self.get_api_key().await;

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
            "model": self.model,
            "max_tokens": 4096,
            "system": system_prompt,
            "messages": api_messages,
        });

        let mut req = self
            .client
            .post(&url)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&payload);

        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }

        let response = req
            .send()
            .await
            .map_err(|e| LLMError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => LLMError::AuthenticationFailed(text),
                429 => LLMError::RateLimitExceeded,
                _ => LLMError::InvalidRequest(text),
            });
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LLMError::ParseError(e.to_string()))?;

        let content_arr = data
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| LLMError::ParseError("No content array in response".to_string()))?;

        let full_content: String = content_arr
            .iter()
            .filter_map(|item| item.get("text")?.as_str())
            .collect();

        if let Some(tool_call) = super::parse_tool_calls(&full_content) {
            return Ok(LLMResponse::ToolCall(tool_call));
        }
        Ok(LLMResponse::FinalAnswer(super::FinalAnswer::new(
            full_content,
        )))
    }
}

#[async_trait]
impl LLMProvider for CustomLLMProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    fn is_local(&self) -> bool {
        // Treat localhost/127.0.0.1 endpoints as local
        self.base_url.contains("localhost") || self.base_url.contains("127.0.0.1")
    }

    fn estimated_cost(&self, _tokens: usize) -> f64 {
        0.0
    }

    async fn check_health(&self) -> bool {
        // For keyless providers (local proxies) assume they're available; key-gated
        // providers still need the key present.
        if let Some(ref key_name) = self.api_key_name {
            if !key_name.is_empty() {
                return self.secret_cache.get_secret(key_name).await.is_ok();
            }
        }
        true
    }

    async fn generate(&self, messages: &[Message]) -> super::Result<LLMResponse> {
        match self.protocol.as_str() {
            "openai" => self.generate_openai(messages).await,
            "anthropic" => self.generate_anthropic(messages).await,
            other => Err(LLMError::ProviderUnavailable(format!(
                "Unknown protocol '{}' for custom provider '{}'",
                other, self.provider_name
            ))),
        }
    }
}
