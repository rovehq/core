//! Integration tests for the LLM Router
//!
//! Validates provider failover logic using Mock servers

use serde_json::json;
use std::sync::Arc;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use rove_engine::config::LLMConfig;
use rove_engine::llm::{ollama::OllamaProvider, router::LLMRouter, LLMProvider, Message};

async fn mount_ollama_health(server: &MockServer) {
    let tags_response = json!({
        "models": [
            { "name": "llama3.1:8b" }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(tags_response))
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_llm_router_failover_with_wiremock() {
    // Start two mock servers to represent two different Ollama instances
    let failing_server = MockServer::start().await;
    let succeeding_server = MockServer::start().await;
    mount_ollama_health(&failing_server).await;
    mount_ollama_health(&succeeding_server).await;

    // First provider throws 500 error (simulating failure)
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&failing_server)
        .await;

    // Second provider succeeds
    let success_response = json!({
        "model": "llama3.1:8b",
        "created_at": "2023-08-04T19:22:45.499127Z",
        "message": {
            "role": "assistant",
            "content": "Hello! I am the backup provider."
        },
        "done": true
    });

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_response))
        .mount(&succeeding_server)
        .await;

    // Create providers pointing to our mock servers
    let provider1 = Box::new(OllamaProvider::new(failing_server.uri(), "llama3.1:8b").unwrap())
        as Box<dyn LLMProvider>;
    let provider2 = Box::new(OllamaProvider::new(succeeding_server.uri(), "llama3.1:8b").unwrap())
        as Box<dyn LLMProvider>;

    // We pass them to router in this order: failing first, succeeding second.
    // However, LLMRouter ranks them. If both are exact same profile/costs, ranking might retain order or swap.
    // To ensure provider1 is attempted first, let's use a config where both are local and zero cost.
    // The router's unstable sort usually preserves order if costs/is_local are tied, or we can just verify the outcome.
    let config = Arc::new(LLMConfig {
        default_provider: "ollama".to_string(),
        sensitivity_threshold: 0.5,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let router = LLMRouter::new(vec![provider1, provider2], config);

    let messages = vec![Message::user("Hello")];

    // Call the router
    let response = router.call(&messages).await;

    assert!(
        response.is_ok(),
        "Router should fallback to the succeeding provider and return Ok"
    );

    let unwrapped = response.unwrap();
    match unwrapped {
        (rove_engine::llm::LLMResponse::FinalAnswer(ans), provider) => {
            assert_eq!(ans.content, "Hello! I am the backup provider.");
            assert_eq!(provider, "ollama");
        }
        _ => panic!("Expected FinalAnswer"),
    }
}

// Property 8: LLM Router Provider Fallback
// Validates: Requirements 4.4
#[tokio::test]
async fn test_property_llm_router_provider_fallback_all_fail() {
    let failing_server1 = MockServer::start().await;
    let failing_server2 = MockServer::start().await;
    mount_ollama_health(&failing_server1).await;
    mount_ollama_health(&failing_server2).await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&failing_server1)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&failing_server2)
        .await;

    let p1 = Box::new(OllamaProvider::new(failing_server1.uri(), "llama3.1:8b").unwrap())
        as Box<dyn LLMProvider>;
    let p2 = Box::new(OllamaProvider::new(failing_server2.uri(), "llama3.1:8b").unwrap())
        as Box<dyn LLMProvider>;

    let config = Arc::new(LLMConfig {
        default_provider: "ollama".to_string(),
        sensitivity_threshold: 0.5,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let router = LLMRouter::new(vec![p1, p2], config);
    let messages = vec![Message::user("Hello")];

    let response = router.call(&messages).await;

    assert!(
        response.is_err(),
        "Router should fail if ALL providers fail"
    );

    // Check error type
    let err = response.unwrap_err();
    match err {
        rove_engine::llm::LLMError::ProviderUnavailable(msg) => {
            assert!(
                msg.contains("All LLM providers failed"),
                "Expected all providers exhausted message"
            );
        }
        _ => panic!("Expected ProviderUnavailable error indicating all providers exhausted"),
    }
}
