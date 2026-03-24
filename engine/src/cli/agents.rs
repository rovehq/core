use anyhow::{Context, Result};
use sdk::{AgentSpec, CapabilityRef, SpecRunStatus, TaskExecutionProfile};
use uuid::Uuid;

use crate::cli::database_path::database_path;
use crate::cli::run::execute_local_task_request;
use crate::config::Config;
use crate::storage::Database;
use crate::system::factory;
use crate::system::specs::{allowed_tools, slugify, SpecRepository};

use super::commands::{AgentAction, AgentFactoryAction};

pub async fn handle_agents(action: AgentAction, config: &Config) -> Result<()> {
    let repo = SpecRepository::new()?;
    match action {
        AgentAction::List => list_agents(&repo),
        AgentAction::Show { id } => show_agent(&repo, &id),
        AgentAction::Create {
            id,
            name,
            purpose,
            instructions,
            tool,
            disabled,
        } => create_agent(
            &repo,
            AgentSpec {
                id: slugify(&id),
                name: name.unwrap_or_else(|| id.clone()),
                purpose: purpose.unwrap_or_else(|| "Reusable Rove agent".to_string()),
                instructions: instructions.unwrap_or_else(|| {
                    "Help the user complete the assigned task while staying within the configured capabilities.".to_string()
                }),
                enabled: !disabled,
                capabilities: tool
                    .into_iter()
                    .map(|name| CapabilityRef {
                        kind: "tool".to_string(),
                        name,
                        required: false,
                    })
                    .collect(),
                ..AgentSpec::default()
            },
        ),
        AgentAction::Enable { id } => set_enabled(&repo, &id, true),
        AgentAction::Disable { id } => set_enabled(&repo, &id, false),
        AgentAction::Run { id, prompt } => run_agent(&repo, config, &id, prompt.join(" ")).await,
        AgentAction::Export { id, path } => export_agent(&repo, &id, &path),
        AgentAction::Import { path } => import_agent(&repo, &path),
        AgentAction::Runs { limit } => list_runs(config, limit).await,
        AgentAction::Factory { action } => handle_factory(action, &repo),
        AgentAction::FromTask { task_id, id, name } => {
            create_agent_from_task(&repo, config, &task_id, id.as_deref(), name.as_deref()).await
        }
    }
}

fn handle_factory(action: AgentFactoryAction, repo: &SpecRepository) -> Result<()> {
    match action {
        AgentFactoryAction::Templates => {
            for template in factory::list_agent_templates() {
                println!(
                    "{}\t{}\t{}",
                    template.id, template.name, template.description
                );
            }
            Ok(())
        }
        AgentFactoryAction::Preview {
            template,
            id,
            name,
            requirement,
        } => {
            let spec = factory::preview_agent(
                &requirement.join(" "),
                template.as_deref(),
                id.as_deref(),
                name.as_deref(),
            )?;
            println!("{}", toml::to_string_pretty(&spec)?);
            Ok(())
        }
        AgentFactoryAction::Create {
            template,
            id,
            name,
            requirement,
        } => {
            let spec = factory::create_agent(
                repo,
                &requirement.join(" "),
                template.as_deref(),
                id.as_deref(),
                name.as_deref(),
            )?;
            println!("{}", toml::to_string_pretty(&spec)?);
            Ok(())
        }
    }
}

fn list_agents(repo: &SpecRepository) -> Result<()> {
    let specs = repo.list_agents()?;
    if specs.is_empty() {
        println!("No agents configured.");
        return Ok(());
    }
    for spec in specs {
        println!(
            "{}\t{}\t{}",
            spec.id,
            if spec.enabled { "enabled" } else { "disabled" },
            spec.name
        );
    }
    Ok(())
}

fn show_agent(repo: &SpecRepository, id: &str) -> Result<()> {
    let spec = repo.load_agent(id)?;
    println!("{}", toml::to_string_pretty(&spec)?);
    Ok(())
}

fn create_agent(repo: &SpecRepository, spec: AgentSpec) -> Result<()> {
    let saved = repo.save_agent(&spec)?;
    println!("Saved agent {} ({})", saved.id, saved.name);
    Ok(())
}

fn set_enabled(repo: &SpecRepository, id: &str, enabled: bool) -> Result<()> {
    let mut spec = repo.load_agent(id)?;
    spec.enabled = enabled;
    repo.save_agent(&spec)?;
    println!(
        "Agent '{}' {}",
        spec.id,
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

async fn run_agent(repo: &SpecRepository, config: &Config, id: &str, prompt: String) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("Agent run requires a non-empty prompt");
    }

    let spec = repo.load_agent(id)?;
    if !spec.enabled {
        anyhow::bail!("Agent '{}' is disabled", spec.id);
    }

    let run_id = Uuid::new_v4().to_string();
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for agent run")?;
    db.agent_runs()
        .start_agent_run(&run_id, &spec.id, None, None, &prompt)
        .await?;

    let execution_profile = TaskExecutionProfile {
        agent_id: Some(spec.id.clone()),
        agent_name: Some(spec.name.clone()),
        purpose: Some(spec.purpose.clone()),
        instructions: spec.instructions.clone(),
        allowed_tools: allowed_tools(&spec),
        output_contract: spec.output_contract.clone(),
    };

    let result = execute_local_task_request(
        prompt.clone(),
        config,
        sdk::RunMode::Serial,
        sdk::RunIsolation::None,
        Some(execution_profile),
    )
    .await;

    match result {
        Ok(task_result) => {
            db.agent_runs()
                .finish_agent_run(
                    &run_id,
                    SpecRunStatus::Completed,
                    Some(&task_result.task_id),
                    Some(&task_result.answer),
                    None,
                )
                .await?;
            println!("{}", task_result.answer);
            println!("Task ID: {}", task_result.task_id);
            Ok(())
        }
        Err(error) => {
            db.agent_runs()
                .finish_agent_run(
                    &run_id,
                    SpecRunStatus::Failed,
                    None,
                    None,
                    Some(&error.to_string()),
                )
                .await?;
            Err(error)
        }
    }
}

fn export_agent(repo: &SpecRepository, id: &str, path: &std::path::Path) -> Result<()> {
    let target = repo.export_agent(id, path)?;
    println!("Exported agent to {}", target.display());
    Ok(())
}

fn import_agent(repo: &SpecRepository, path: &std::path::Path) -> Result<()> {
    let spec = repo.import_agent(path)?;
    println!("Imported agent {} ({})", spec.id, spec.name);
    Ok(())
}

async fn list_runs(config: &Config, limit: i64) -> Result<()> {
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for run listing")?;
    for run in db.agent_runs().list_agent_runs(limit).await? {
        println!(
            "{}\t{}\t{:?}\t{}",
            run.run_id, run.agent_id, run.status, run.input
        );
    }
    Ok(())
}

async fn create_agent_from_task(
    repo: &SpecRepository,
    config: &Config,
    task_id: &str,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<()> {
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for task conversion")?;
    let spec = factory::agent_from_task(repo, &db, task_id, id, name).await?;
    println!("{}", toml::to_string_pretty(&spec)?);
    Ok(())
}
