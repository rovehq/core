//! Example demonstrating simple task execution with the Agent Core
//!
//! This example shows how to:
//! - Create an agent with LLM provider
//! - Execute a simple task
//! - View task execution logs
//!
//! Prerequisites:
//! - Ollama must be installed and running
//! - A model must be available (e.g., qwen2.5-coder:7b)

use rove_engine::gateway::Task;
use rove_engine::{
    agent::AgentCore,
    builtin_tools::ToolRegistry,
    config::LLMConfig,
    db::{tasks::TaskRepository, Database},
    llm::{ollama::OllamaProvider, router::LLMRouter, LLMProvider},
    rate_limiter::RateLimiter,
    risk_assessor::RiskAssessor,
};
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Simple Task Execution Example ===\n");

    // Setup temporary database
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await?;
    let pool = db.pool().clone();

    println!("✓ Database initialized");

    // Setup LLM configuration
    let llm_config = Arc::new(LLMConfig {
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

    // Create Ollama provider
    let ollama = OllamaProvider::new("http://localhost:11434", "qwen2.5-coder:7b").unwrap();

    println!(
        "✓ LLM Provider: {} (local: {})",
        ollama.name(),
        ollama.is_local()
    );

    // Create LLM router with the provider
    let router = Arc::new(LLMRouter::with_local_brain(
        vec![Box::new(ollama)],
        llm_config,
        None,
    ));

    // Create agent components
    let risk_assessor = RiskAssessor::new();
    let rate_limiter = Arc::new(RateLimiter::new(pool.clone()));
    let task_repo = Arc::new(TaskRepository::new(pool));

    println!("✓ Agent components initialized\n");

    // Create agent core
    let tools = Arc::new(ToolRegistry::empty());
    let workspace_locks = Arc::new(rove_engine::gateway::WorkspaceLocks::new());
    let mut agent = AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo.clone(),
        tools,
        None,
        Arc::new(rove_engine::config::Config::default()),
        workspace_locks,
    )
    .expect("Failed to create AgentCore");

    println!("🤖 Agent ready!\n");

    // Execute a simple task
    println!("─────────────────────────────────────");
    println!("📝 Task: Calculate 15 + 27");
    println!("─────────────────────────────────────\n");

    let task = Task::build_from_cli("What is 15 + 27?");

    match agent.process_task(task).await {
        Ok(result) => {
            println!("✅ Task completed successfully!\n");
            println!("Task ID: {}", result.task_id);
            println!("Answer: {}", result.answer);
            println!("Provider: {}", result.provider_used);
            println!("Duration: {}ms", result.duration_ms);
            println!("Iterations: {}", result.iterations);

            // Fetch task history from database
            println!("\n─────────────────────────────────────");
            println!("📊 Task Execution Log:");
            println!("─────────────────────────────────────\n");

            let task_uuid = uuid::Uuid::parse_str(&result.task_id).unwrap_or_default();
            if let Ok(Some(task_record)) = task_repo.get_task(&task_uuid).await {
                println!("Status: {:?}", task_record.status);
                println!("Created: {}", task_record.created_at);
                if let Some(completed) = task_record.completed_at {
                    println!("Completed: {}", completed);
                }

                // Get task steps
                if let Ok(steps) = task_repo.get_task_steps(&task_uuid).await {
                    println!("\nExecution Steps ({} total):", steps.len());
                    for (i, step) in steps.iter().enumerate() {
                        println!("\n  Step {}: {:?}", i + 1, step.step_type);
                        let content = if step.content.len() > 100 {
                            format!("{}...", &step.content[..100])
                        } else {
                            step.content.clone()
                        };
                        println!("    {}", content);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Task failed: {}", e);
            return Err(e.into());
        }
    }

    println!("\n─────────────────────────────────────");
    println!("✅ Example Complete!");
    println!("─────────────────────────────────────\n");

    Ok(())
}
