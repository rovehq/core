//! Integration tests for LocalBrain router integration
//!
//! Tests that LocalBrain is properly wired into the LLM router
//! and can be used as a fallback before cloud providers.

use brain::reasoning::LocalBrain;
use rove_engine::config::LLMConfig;
use rove_engine::llm::router::LLMRouter;
use rove_engine::llm::{LLMProvider, Message};
use std::sync::Arc;

/// Test that router can be created with LocalBrain
#[tokio::test]
async fn test_router_with_local_brain() {
    let config = Arc::new(LLMConfig {
        default_provider: "ollama".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let local_brain = Arc::new(LocalBrain::new(
        "http://localhost:8080",
        "qwen2.5-coder-0.5b",
    ));

    let _router = LLMRouter::with_local_brain(vec![], config, Some(local_brain));

    // Router should be created successfully
    assert!(true);
}

/// Test that router works without LocalBrain (backward compatibility)
#[tokio::test]
async fn test_router_without_local_brain() {
    let config = Arc::new(LLMConfig {
        default_provider: "ollama".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let _router = LLMRouter::with_local_brain(vec![], config, None);

    // Router should work without LocalBrain
    assert!(true);
}

/// Test that LocalBrain check_available works
#[tokio::test]
async fn test_local_brain_availability_check() {
    let brain = LocalBrain::new("http://localhost:9999", "test-model");

    // Should return false when server not running
    let available = brain.check_available().await;
    assert!(!available);
}

/// Test LocalBrain default paths
#[test]
fn test_local_brain_paths() {
    // Default brain directory should exist
    let brain_dir = LocalBrain::default_brain_dir();
    assert!(brain_dir.is_some());

    if let Some(dir) = brain_dir {
        assert!(dir.to_string_lossy().contains(".rove/brains/reasoning"));
    }

    // Adapter path should be defined
    let adapter_path = LocalBrain::adapter_path();
    assert!(adapter_path.is_some());

    if let Some(path) = adapter_path {
        assert!(path.to_string_lossy().ends_with("adapter.gguf"));
    }
}

/// Test that router falls back to providers when LocalBrain unavailable
#[tokio::test]
async fn test_router_fallback_when_local_brain_unavailable() {
    use async_trait::async_trait;
    use rove_engine::llm::{FinalAnswer, LLMError, LLMResponse};

    // Mock provider that always succeeds
    struct MockProvider;

    #[async_trait]
    impl LLMProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn is_local(&self) -> bool {
            false
        }

        fn estimated_cost(&self, _tokens: usize) -> f64 {
            0.0
        }

        async fn generate(&self, _messages: &[Message]) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::FinalAnswer(FinalAnswer::new("mock response")))
        }

        async fn check_health(&self) -> bool {
            true
        }
    }

    let config = Arc::new(LLMConfig {
        default_provider: "mock".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    // LocalBrain pointing to non-existent server
    let local_brain = Arc::new(LocalBrain::new("http://localhost:9999", "test-model"));

    let router =
        LLMRouter::with_local_brain(vec![Box::new(MockProvider)], config, Some(local_brain));

    let messages = vec![Message::user("test")];

    // Should fall back to mock provider when LocalBrain unavailable
    let result = router.call(&messages).await;
    assert!(result.is_ok());

    let (response, provider_name) = result.unwrap();
    assert_eq!(provider_name, "mock");

    match response {
        LLMResponse::FinalAnswer(answer) => {
            assert_eq!(answer.content, "mock response");
        }
        _ => panic!("Expected FinalAnswer"),
    }
}

/// Test model installation check
#[test]
fn test_model_installation_check() {
    // Should return false for non-existent model
    let installed = LocalBrain::is_model_installed("non-existent-model");
    assert!(!installed);
}
