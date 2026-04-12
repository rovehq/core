//! Example demonstrating the Ollama LLM provider
//!
//! This example shows how to use the OllamaProvider to interact with a local Ollama instance.
//!
//! Prerequisites:
//! - Ollama must be installed and running (https://ollama.ai)
//! - A model must be pulled (e.g., `ollama pull llama3.1:8b`)
//!
//! Run with: cargo run --example ollama_provider_example

use rove_engine::llm::{ollama::OllamaProvider, LLMProvider, LLMResponse, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Ollama Provider Example ===\n");

    let provider = OllamaProvider::new("http://localhost:11434", "qwen2.5-coder:7b").unwrap();

    println!("Provider: {}", provider.name());
    println!("Is Local: {}", provider.is_local());
    println!(
        "Cost for 1000 tokens: ${:.4}",
        provider.estimated_cost(1000)
    );
    println!(
        "Cost for 10000 tokens: ${:.4}\n",
        provider.estimated_cost(10000)
    );

    // Check if Ollama is running
    println!("Checking if Ollama is available...");
    let test_messages = vec![Message::user("Hello")];

    match provider.generate(&test_messages).await {
        Ok(response) => {
            println!("✓ Ollama is running and responsive\n");

            match response {
                LLMResponse::ToolCall(tool_call) => {
                    println!("Response type: Tool Call");
                    println!("  Tool: {}", tool_call.name);
                    println!("  Arguments: {}", tool_call.arguments);
                }
                LLMResponse::FinalAnswer(answer) => {
                    println!("Response type: Final Answer");
                    println!("  Content: {}", answer.content);
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Failed to connect to Ollama: {}", e);
            eprintln!("\nMake sure Ollama is running:");
            eprintln!("  1. Install Ollama from https://ollama.ai");
            eprintln!("  2. Pull a model: ollama pull qwen2.5-coder:7b");
            eprintln!("  3. Ollama should start automatically");
            eprintln!("\nOr start it manually if needed.");
            return Err(e.into());
        }
    }

    println!("\n=== Conversation Example ===\n");

    // Demonstrate a multi-turn conversation
    let mut messages = vec![
        Message::system("You are a helpful assistant that can use tools to help users."),
        Message::user("What is 2 + 2?"),
    ];

    println!("User: {}", messages[1].content);

    match provider.generate(&messages).await? {
        LLMResponse::ToolCall(tool_call) => {
            println!("Assistant: [Calling tool: {}]", tool_call.name);
            println!("  Arguments: {}", tool_call.arguments);

            // Simulate tool execution
            messages.push(Message::tool_result("4", &tool_call.id));

            // Get final answer
            match provider.generate(&messages).await? {
                LLMResponse::FinalAnswer(answer) => {
                    println!("Assistant: {}", answer.content);
                }
                _ => println!("Assistant: [Unexpected response]"),
            }
        }
        LLMResponse::FinalAnswer(answer) => {
            println!("Assistant: {}", answer.content);
        }
    }

    println!("\n=== Example Complete ===");

    Ok(())
}
