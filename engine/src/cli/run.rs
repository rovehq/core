use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde_json::json;

use crate::agent::AgentCore;
use crate::api::gateway::{Task, WorkspaceLocks};
use crate::cli::database_path::database_path;
use crate::config::{metadata::SERVICE_NAME, Config};
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::gemini::GeminiProvider;
use crate::llm::nvidia_nim::NvidiaNimProvider;
use crate::llm::ollama::OllamaProvider;
use crate::llm::openai::OpenAIProvider;
use crate::llm::router::LLMRouter;
use crate::security::rate_limiter::RateLimiter;
use crate::security::risk_assessor::RiskAssessor;
use crate::security::secrets::{SecretCache, SecretManager};
use crate::steering::loader::SteeringEngine;
use crate::storage::{Database, TaskRepository};
use crate::tools::{FilesystemTool, TerminalTool, ToolRegistry, VisionTool};

use super::output::OutputFormat;

pub async fn handle_run(task: String, config: &Config, format: OutputFormat) -> Result<()> {
    let database = Database::new(&database_path(config))
        .await
        .context("Failed to open database")?;

    let secret_manager = Arc::new(SecretManager::new(SERVICE_NAME));
    let secret_cache = Arc::new(SecretCache::new(secret_manager.clone()));

    let providers = build_providers(config, &secret_manager, &secret_cache).await?;
    let router = Arc::new(LLMRouter::new(providers, Arc::new(config.llm.clone())));
    let rate_limiter = Arc::new(RateLimiter::new(database.pool().clone()));
    let risk_assessor = RiskAssessor::new();
    let task_repo = Arc::new(TaskRepository::new(database.pool().clone()));
    let tools = Arc::new(build_tools(config)?);
    let steering = load_steering(config).await;
    let workspace_locks = Arc::new(WorkspaceLocks::new());

    let mut agent = AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        tools,
        steering,
        Arc::new(config.clone()),
        workspace_locks,
    )?;

    let agent_task = Task::build_from_cli(task.clone());
    print_start(&task, format)?;

    match agent.process_task(agent_task).await {
        Ok(task_result) => {
            print_success(
                &task_result.task_id,
                &task_result.answer,
                &task_result.provider_used,
                task_result.duration_ms,
                task_result.iterations,
                format,
            )?;
            Ok(())
        }
        Err(error) => {
            print_failure(&error, format)?;
            Err(error)
        }
    }
}

async fn build_providers(
    config: &Config,
    secret_manager: &Arc<SecretManager>,
    secret_cache: &Arc<SecretCache>,
) -> Result<Vec<Box<dyn crate::llm::LLMProvider>>> {
    let mut providers: Vec<Box<dyn crate::llm::LLMProvider>> = Vec::new();

    match OllamaProvider::new(
        config.llm.ollama.base_url.clone(),
        config.llm.ollama.model.clone(),
    ) {
        Ok(provider) => providers.push(Box::new(provider)),
        Err(error) => tracing::warn!("Skipping Ollama provider: {}", error),
    }

    if secret_manager.has_secret("openai_api_key").await {
        providers.push(Box::new(OpenAIProvider::new(
            config.llm.openai.clone(),
            secret_cache.clone(),
        )));
    }

    if secret_manager.has_secret("anthropic_api_key").await {
        providers.push(Box::new(AnthropicProvider::new(
            config.llm.anthropic.clone(),
            secret_cache.clone(),
        )));
    }

    if secret_manager.has_secret("gemini_api_key").await {
        providers.push(Box::new(GeminiProvider::new(
            config.llm.gemini.clone(),
            secret_cache.clone(),
        )));
    }

    if secret_manager.has_secret("nvidia_nim_api_key").await {
        providers.push(Box::new(NvidiaNimProvider::new(
            config.llm.nvidia_nim.clone(),
            secret_cache.clone(),
        )));
    }

    if providers.is_empty() {
        return Err(anyhow!(
            "No LLM providers are available. Start Ollama or configure a cloud provider API key."
        ));
    }

    Ok(providers)
}

fn build_tools(config: &Config) -> Result<ToolRegistry> {
    let mut tools = ToolRegistry::empty();
    let workspace = config.core.workspace.clone();

    if config.plugins.fs_editor {
        tools.fs = Some(FilesystemTool::new(workspace.clone())?);
    }

    if config.plugins.terminal {
        tools.terminal = Some(TerminalTool::new(workspace.to_string_lossy().to_string()));
    }

    if config.plugins.screenshot {
        tools.vision = Some(VisionTool::new(workspace));
    }

    Ok(tools)
}

async fn load_steering(config: &Config) -> Option<SteeringEngine> {
    if !config.steering.auto_detect {
        return None;
    }

    let skill_dir = expand_skill_dir(&config.steering.skill_dir);
    match SteeringEngine::new(&skill_dir).await {
        Ok(engine) => Some(engine),
        Err(error) => {
            tracing::warn!("Failed to load steering engine: {}", error);
            None
        }
    }
}

fn expand_skill_dir(skill_dir: &Path) -> std::path::PathBuf {
    let raw = skill_dir.to_string_lossy();
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    skill_dir.to_path_buf()
}

fn print_start(task: &str, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("Executing task: {}", task);
            println!();
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "running",
                    "task": task,
                }))?
            );
        }
    }

    Ok(())
}

fn print_success(
    task_id: &str,
    answer: &str,
    provider_used: &str,
    duration_ms: i64,
    iterations: usize,
    format: OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("Result:");
            println!("{}", answer);
            println!();
            println!("Task completed successfully");
            println!("  Task ID: {}", task_id);
            println!("  Provider: {}", provider_used);
            println!("  Duration: {}ms", duration_ms);
            println!("  Iterations: {}", iterations);
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "completed",
                    "task_id": task_id,
                    "answer": answer,
                    "provider": provider_used,
                    "duration_ms": duration_ms,
                    "iterations": iterations,
                }))?
            );
        }
    }

    Ok(())
}

fn print_failure(error: &anyhow::Error, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("Task failed: {}", error);
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "failed",
                    "error": error.to_string(),
                }))?
            );
        }
    }

    Ok(())
}
