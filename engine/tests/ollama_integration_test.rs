//! Integration tests for Ollama provider
//!
//! These tests verify the Ollama provider implementation.
//! Note: These tests do NOT require a running Ollama instance.
//! They test the provider's internal logic and error handling.

use rove_engine::llm::{ollama::OllamaProvider, LLMError, LLMProvider, Message};

#[tokio::test]
async fn test_ollama_provider_properties() {
    let provider = OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap();

    assert_eq!(provider.name(), "ollama");
    assert!(provider.is_local());
    assert_eq!(provider.estimated_cost(1000), 0.0);
    assert_eq!(provider.estimated_cost(10000), 0.0);
    assert_eq!(provider.estimated_cost(100000), 0.0);
}

#[tokio::test]
async fn test_ollama_connection_error() {
    // Use an invalid port to ensure connection fails
    let provider = OllamaProvider::new("http://localhost:99999", "llama3.1:8b").unwrap();
    let messages = vec![Message::user("Hello")];

    let result = provider.generate(&messages).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        LLMError::ProviderUnavailable(msg) => {
            assert!(msg.contains("Cannot connect to Ollama"));
        }
        LLMError::NetworkError(_) => {
            // Also acceptable - network errors can manifest differently
        }
        other => panic!(
            "Expected ProviderUnavailable or NetworkError, got: {:?}",
            other
        ),
    }
}

// Note: Message conversion and tool call parsing are tested in unit tests
// within the ollama.rs module. Integration tests focus on public API behavior.

#[test]
fn test_ollama_multiple_providers() {
    // Test that we can create multiple provider instances
    let provider1 = OllamaProvider::new("http://localhost:11434", "llama3.1:8b").unwrap();
    let provider2 = OllamaProvider::new("http://localhost:11434", "llama3.1:70b").unwrap();
    let provider3 = OllamaProvider::new("http://192.168.1.100:11434", "llama3.1:8b").unwrap();

    assert_eq!(provider1.name(), "ollama");
    assert_eq!(provider2.name(), "ollama");
    assert_eq!(provider3.name(), "ollama");

    // All should be local
    assert!(provider1.is_local());
    assert!(provider2.is_local());
    assert!(provider3.is_local());

    // All should have zero cost
    assert_eq!(provider1.estimated_cost(1000), 0.0);
    assert_eq!(provider2.estimated_cost(1000), 0.0);
    assert_eq!(provider3.estimated_cost(1000), 0.0);
}
