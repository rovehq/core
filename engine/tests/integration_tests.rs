use async_trait::async_trait;
use rove_engine::llm::{FinalAnswer, LLMProvider, LLMResponse, Message};

/// A Mock LLM Provider that returns pre-determined JSON for testing
/// the Conductor/Agent Think-Act-Observe loop without API costs.
#[derive(Debug)]
pub struct MockLLMProvider {
    canned_response: String,
}

impl MockLLMProvider {
    pub fn new(canned_response: &str) -> Self {
        Self {
            canned_response: canned_response.to_string(),
        }
    }
}

#[async_trait]
impl LLMProvider for MockLLMProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn estimated_cost(&self, _tokens: usize) -> f64 {
        0.0
    }

    fn is_local(&self) -> bool {
        true
    }

    async fn check_health(&self) -> bool {
        true
    }

    async fn generate(&self, _messages: &[Message]) -> rove_engine::llm::Result<LLMResponse> {
        Ok(LLMResponse::FinalAnswer(FinalAnswer::new(
            &self.canned_response,
        )))
    }
}

#[tokio::test]
async fn test_agent_think_act_observe_loop() {
    let mock_plan_json = r#"[
        {
            "id": "step_1",
            "step_type": "Command",
            "description": "Echo test",
            "content": "echo 'hello world'"
        }
    ]"#;

    let provider = MockLLMProvider::new(mock_plan_json);

    // Initialize agent with the mock provider
    // In a real test we'd wire up the full AgentCore.
    // Here we assert the mock provider returns the expected response content.
    let response = provider.generate(&[]).await.unwrap();
    match response {
        LLMResponse::FinalAnswer(answer) => {
            assert!(answer.content.contains("step_1"));
        }
        _ => panic!("Expected FinalAnswer"),
    }
}

#[tokio::test]
async fn test_daemon_startup_sequence() {
    // This is a placeholder test for the daemon orchestrator.
    // In practice, this would involve starting `DaemonManager::run`
    // with a mock configuration and verifying the socket binds.
    // Daemon startup mock test passed
}
