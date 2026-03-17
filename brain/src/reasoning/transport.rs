use serde::{Deserialize, Serialize};

/// llama.cpp chat completion request format.
#[derive(Debug, Serialize)]
pub(super) struct LlamaCppRequest {
    pub messages: Vec<LlamaCppMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    pub temperature: f32,
    pub max_tokens: i32,
}

/// llama.cpp message format.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct LlamaCppMessage {
    pub role: String,
    pub content: String,
}

/// llama.cpp chat completion response format.
#[derive(Debug, Deserialize)]
pub(super) struct LlamaCppResponse {
    pub choices: Vec<LlamaCppChoice>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LlamaCppChoice {
    pub message: LlamaCppMessage,
}

/// llama.cpp embedding response format.
#[derive(Debug, Deserialize)]
pub(super) struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EmbeddingData {
    pub embedding: Vec<f32>,
}
