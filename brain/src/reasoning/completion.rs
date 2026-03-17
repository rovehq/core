use async_trait::async_trait;
use sdk::{Brain, BrainResponse, EngineError, Message, ToolSchema};

use super::client::LocalBrain;
use super::transport::{LlamaCppMessage, LlamaCppRequest, LlamaCppResponse};

#[async_trait]
impl Brain for LocalBrain {
    fn name(&self) -> &str {
        "local-brain"
    }

    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[ToolSchema],
    ) -> Result<BrainResponse, EngineError> {
        let request = LlamaCppRequest {
            messages: build_messages(system, messages),
            tools: build_tools(tools),
            temperature: 0.7,
            max_tokens: 2048,
        };

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let response = self.client.post(&url).json(&request).send().await.map_err(|error| {
            if error.is_timeout() {
                EngineError::LLMProvider("llama-server timeout".to_string())
            } else if error.is_connect() {
                EngineError::LLMProvider(format!(
                    "Cannot connect to llama-server at {}. Is it running?",
                    self.base_url
                ))
            } else {
                EngineError::LLMProvider(format!("llama-server error: {}", error))
            }
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| String::new());
            return Err(EngineError::LLMProvider(format!(
                "llama-server API error ({}): {}",
                status, error_text
            )));
        }

        let llama_response = response.json::<LlamaCppResponse>().await.map_err(|error| {
            EngineError::LLMProvider(format!("Failed to parse llama-server response: {}", error))
        })?;

        let content = llama_response
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .unwrap_or_default();

        if let Ok(tool_calls) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
            if !tool_calls.is_empty() {
                return Ok(BrainResponse {
                    content: String::new(),
                    tool_calls: Some(tool_calls),
                });
            }
        }

        Ok(BrainResponse {
            content,
            tool_calls: None,
        })
    }
}

fn build_messages(system: &str, messages: &[Message]) -> Vec<LlamaCppMessage> {
    let mut llama_messages = vec![LlamaCppMessage {
        role: "system".to_string(),
        content: system.to_string(),
    }];

    for message in messages {
        llama_messages.push(LlamaCppMessage {
            role: message.role.clone(),
            content: message.content.clone(),
        });
    }

    llama_messages
}

fn build_tools(tools: &[ToolSchema]) -> Option<Vec<serde_json::Value>> {
    if tools.is_empty() {
        return None;
    }

    Some(
        tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect(),
    )
}
