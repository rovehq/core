use super::{emit_stream_chunk, streaming_active, LLMError, LLMProvider, LLMResponse, Message, MessageRole};
use crate::secrets::SecretCache;
use async_trait::async_trait;
use futures::StreamExt;
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
    /// When true, overrides localhost detection and treats this provider as cloud-capable.
    pub force_cloud: bool,
    /// When true, system messages are merged into the first user message instead of
    /// being sent as a system role. Needed for models whose chat template forbids system role.
    pub no_system_prompt: bool,
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
        force_cloud: bool,
        no_system_prompt: bool,
        secret_cache: Arc<SecretCache>,
    ) -> Self {
        Self {
            provider_name,
            protocol,
            base_url,
            model,
            api_key_name,
            force_cloud,
            no_system_prompt,
            secret_cache,
            client: reqwest::Client::new(),
        }
    }

    fn flatten_messages<'a>(&self, messages: &'a [Message]) -> Vec<std::borrow::Cow<'a, Message>> {
        if !self.no_system_prompt {
            return messages.iter().map(std::borrow::Cow::Borrowed).collect();
        }
        let mut system_prefix = String::new();
        let mut out: Vec<std::borrow::Cow<'_, Message>> = Vec::new();
        for msg in messages {
            if msg.role == MessageRole::System {
                if !system_prefix.is_empty() {
                    system_prefix.push('\n');
                }
                system_prefix.push_str(&msg.content);
            } else if !system_prefix.is_empty() && msg.role == MessageRole::User && out.is_empty() {
                let merged = Message {
                    role: MessageRole::User,
                    content: format!("{}\n\n{}", system_prefix, msg.content),
                    tool_call_id: msg.tool_call_id.clone(),
                };
                system_prefix.clear();
                out.push(std::borrow::Cow::Owned(merged));
            } else {
                out.push(std::borrow::Cow::Borrowed(msg));
            }
        }
        out
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
        let stream = streaming_active();

        let flattened = self.flatten_messages(messages);
        let api_messages: Vec<_> = flattened
            .iter()
            .map(|msg| {
                json!({
                    "role": msg.role.to_string(),
                    "content": msg.content
                })
            })
            .collect();

        let mut payload = json!({
            "model": self.model,
            "messages": api_messages,
        });
        if stream {
            payload["stream"] = json!(true);
        }

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

        if stream {
            // SSE streaming: accumulate text, emit each chunk to the global sink.
            let mut body = response.bytes_stream();
            let mut accumulated = String::new();
            let mut buf = String::new();

            while let Some(chunk) = body.next().await {
                let bytes = chunk.map_err(|e| LLMError::NetworkError(e.to_string()))?;
                buf.push_str(&String::from_utf8_lossy(&bytes));

                // SSE lines end with "\n"; process complete lines.
                while let Some(newline_pos) = buf.find('\n') {
                    let line = buf[..newline_pos].trim().to_string();
                    buf = buf[newline_pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(text) = val
                                .get("choices")
                                .and_then(|c| c.as_array())
                                .and_then(|c| c.first())
                                .and_then(|c| c.get("delta"))
                                .and_then(|d| d.get("content"))
                                .and_then(|t| t.as_str())
                            {
                                emit_stream_chunk(text);
                                accumulated.push_str(text);
                            }
                        }
                    }
                }
            }

            if let Some(tool_call) = super::parse_tool_calls(&accumulated) {
                return Ok(LLMResponse::ToolCall(tool_call));
            }
            return Ok(LLMResponse::FinalAnswer(super::FinalAnswer::new(
                accumulated,
            )));
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
        if self.force_cloud {
            return false;
        }
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
