//! End-to-end test for LocalBrain with actual llama-server
//!
//! This test requires llama-server to be running on localhost:8080
//! Run with: cargo test -p engine --test local_brain_e2e_test -- --ignored

use brain::reasoning::LocalBrain;
use rove_engine::config::LLMConfig;
use rove_engine::llm::router::LLMRouter;
use rove_engine::llm::Message;
use std::sync::Arc;

/// Test LocalBrain with actual llama-server
///
/// Prerequisites:
/// 1. llama-server must be running on localhost:8080
/// 2. A model must be loaded
///
/// Run: cargo test -p engine --test local_brain_e2e_test -- --ignored
#[tokio::test]
#[ignore] // Requires llama-server to be running
async fn test_local_brain_with_actual_server() {
    // Create LocalBrain instance
    let local_brain = Arc::new(LocalBrain::new("http://localhost:8080", "phi-3-mini-q4"));

    // Check if server is available
    let available = local_brain.check_available().await;
    assert!(available, "llama-server must be running on localhost:8080");

    // Create router with LocalBrain
    let config = Arc::new(LLMConfig {
        default_provider: "local-brain".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let router = LLMRouter::with_local_brain(
        vec![], // No other providers
        config,
        Some(local_brain),
    );

    // Test simple completion
    let messages = vec![
        Message::system("You are a helpful assistant."),
        Message::user("Say hello in one word"),
    ];

    let result = router.call(&messages).await;
    assert!(result.is_ok(), "LocalBrain should complete successfully");

    let (response, provider_name) = result.unwrap();
    assert_eq!(provider_name, "local-brain");

    // Check response content
    match response {
        rove_engine::llm::LLMResponse::FinalAnswer(answer) => {
            println!("LocalBrain response: {}", answer.content);
            assert!(!answer.content.is_empty());
            // Should contain some form of greeting
            let lower = answer.content.to_lowercase();
            assert!(
                lower.contains("hello") || lower.contains("hi") || lower.contains("greet"),
                "Response should be a greeting, got: {}",
                answer.content
            );
        }
        _ => panic!("Expected FinalAnswer"),
    }
}

/// Test LocalBrain with code generation task
#[tokio::test]
#[ignore] // Requires llama-server to be running
async fn test_local_brain_code_generation() {
    let local_brain = Arc::new(LocalBrain::new("http://localhost:8080", "phi-3-mini-q4"));

    if !local_brain.check_available().await {
        println!("Skipping test: llama-server not running");
        return;
    }

    let config = Arc::new(LLMConfig {
        default_provider: "local-brain".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let router = LLMRouter::with_local_brain(vec![], config, Some(local_brain));

    let messages = vec![
        Message::system("You are a Rust programming expert."),
        Message::user("Write a hello world function in Rust. Just the code, no explanation."),
    ];

    let result = router.call(&messages).await;
    assert!(result.is_ok());

    let (response, provider_name) = result.unwrap();
    assert_eq!(provider_name, "local-brain");

    match response {
        rove_engine::llm::LLMResponse::FinalAnswer(answer) => {
            println!("Generated code:\n{}", answer.content);
            let content = answer.content.to_lowercase();
            // Should contain Rust code elements
            assert!(
                content.contains("fn") || content.contains("println"),
                "Response should contain Rust code"
            );
        }
        _ => panic!("Expected FinalAnswer"),
    }
}

/// Test LocalBrain performance
#[tokio::test]
#[ignore] // Requires llama-server to be running
async fn test_local_brain_performance() {
    let local_brain = Arc::new(LocalBrain::new("http://localhost:8080", "phi-3-mini-q4"));

    if !local_brain.check_available().await {
        println!("Skipping test: llama-server not running");
        return;
    }

    let config = Arc::new(LLMConfig {
        default_provider: "local-brain".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let router = LLMRouter::with_local_brain(vec![], config, Some(local_brain));

    let messages = vec![
        Message::system("You are a helpful assistant."),
        Message::user("Count from 1 to 5"),
    ];

    let start = std::time::Instant::now();
    let result = router.call(&messages).await;
    let duration = start.elapsed();

    assert!(result.is_ok());
    println!("LocalBrain response time: {:?}", duration);

    // Should complete within reasonable time (120s timeout)
    assert!(duration.as_secs() < 120);
}
