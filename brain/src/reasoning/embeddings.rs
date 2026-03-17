use sdk::EngineError;

use super::client::LocalBrain;
use super::transport::EmbeddingResponse;

impl LocalBrain {
    /// Generate an embedding for text using llama-server.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, EngineError> {
        let url = format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'));

        let request = serde_json::json!({
            "input": text,
        });

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    EngineError::LLMProvider("llama-server embedding timeout".to_string())
                } else if error.is_connect() {
                    EngineError::LLMProvider(format!(
                        "Cannot connect to llama-server at {}. Is it running?",
                        self.base_url
                    ))
                } else {
                    EngineError::LLMProvider(format!("llama-server embedding error: {}", error))
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| String::new());
            return Err(EngineError::LLMProvider(format!(
                "llama-server embedding API error ({}): {}",
                status, error_text
            )));
        }

        let embedding_response = response
            .json::<EmbeddingResponse>()
            .await
            .map_err(|error| {
                EngineError::LLMProvider(format!(
                    "Failed to parse llama-server embedding response: {}",
                    error
                ))
            })?;

        embedding_response
            .data
            .first()
            .map(|entry| entry.embedding.clone())
            .ok_or_else(|| EngineError::LLMProvider("No embedding in response".to_string()))
    }
}
