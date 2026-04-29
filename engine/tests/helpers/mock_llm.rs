use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use rove_engine::llm::{LLMProvider, LLMResponse, Message, Result as LlmResult};

pub struct MockSequenceProvider {
    name: String,
    responses: Mutex<VecDeque<LLMResponse>>,
}

impl MockSequenceProvider {
    pub fn new(name: &str, responses: Vec<LLMResponse>) -> Self {
        Self {
            name: name.to_string(),
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

#[async_trait]
impl LLMProvider for MockSequenceProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_local(&self) -> bool {
        false
    }

    fn estimated_cost(&self, _tokens: usize) -> f64 {
        1.0
    }

    async fn generate(&self, _messages: &[Message]) -> LlmResult<LLMResponse> {
        self.responses.lock().unwrap().pop_front().ok_or_else(|| {
            rove_engine::llm::LLMError::ProviderUnavailable("No mock response".to_string())
        })
    }
}
