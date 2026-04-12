//! Example demonstrating the LLMProvider trait usage
//!
//! This example shows how to implement a simple mock LLM provider
//! and use the LLMProvider trait interface.

use async_trait::async_trait;
use rove_engine::llm::{FinalAnswer, LLMProvider, LLMResponse, Message, Result, ToolCall};

/// Mock LLM provider for demonstration
struct MockProvider {
    name: String,
    is_local: bool,
    cost_per_1k_tokens: f64,
}

impl MockProvider {
    fn new(name: impl Into<String>, is_local: bool, cost_per_1k_tokens: f64) -> Self {
        Self {
            name: name.into(),
            is_local,
            cost_per_1k_tokens,
        }
    }
}

#[async_trait]
impl LLMProvider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_local(&self) -> bool {
        self.is_local
    }

    fn estimated_cost(&self, tokens: usize) -> f64 {
        (tokens as f64 / 1000.0) * self.cost_per_1k_tokens
    }

    async fn generate(&self, messages: &[Message]) -> Result<LLMResponse> {
        // Mock implementation: return a tool call for the first message,
        // then a final answer
        if messages.len() == 1 {
            Ok(LLMResponse::ToolCall(ToolCall::new(
                "call_123",
                "read_file",
                r#"{"path": "example.txt"}"#,
            )))
        } else {
            Ok(LLMResponse::FinalAnswer(FinalAnswer::new(
                "Task completed successfully!",
            )))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Create different provider types
    let ollama = MockProvider::new("ollama", true, 0.0);
    let openai = MockProvider::new("openai", false, 0.002);
    let anthropic = MockProvider::new("anthropic", false, 0.003);

    println!("=== LLM Provider Examples ===\n");

    // Demonstrate provider properties
    for provider in [&ollama as &dyn LLMProvider, &openai, &anthropic] {
        println!("Provider: {}", provider.name());
        println!("  Is Local: {}", provider.is_local());
        println!(
            "  Cost for 1000 tokens: ${:.4}",
            provider.estimated_cost(1000)
        );
        println!(
            "  Cost for 10000 tokens: ${:.4}",
            provider.estimated_cost(10000)
        );
        println!();
    }

    // Demonstrate conversation flow
    println!("=== Conversation Flow ===\n");

    let provider = &ollama;
    let mut messages = vec![Message::user("Read the file example.txt")];

    println!("User: {}", messages[0].content);

    // First call - expect tool call
    match provider.generate(&messages).await? {
        LLMResponse::ToolCall(tool_call) => {
            println!("Assistant: [Tool Call]");
            println!("  Tool: {}", tool_call.name);
            println!("  Arguments: {}", tool_call.arguments);

            // Add tool result to conversation
            messages.push(Message::tool_result(
                "File contents: Hello, World!",
                tool_call.id,
            ));
        }
        LLMResponse::FinalAnswer(answer) => {
            println!("Assistant: {}", answer.content);
        }
    }

    // Second call - expect final answer
    match provider.generate(&messages).await? {
        LLMResponse::ToolCall(tool_call) => {
            println!("Assistant: [Tool Call]");
            println!("  Tool: {}", tool_call.name);
        }
        LLMResponse::FinalAnswer(answer) => {
            println!("Assistant: {}", answer.content);
        }
    }

    Ok(())
}
