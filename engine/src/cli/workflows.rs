use anyhow::{Context, Result};
use sdk::{WorkflowSpec, WorkflowStepSpec};

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::Database;
use crate::system::specs::{slugify, SpecRepository};
use crate::system::{factory, worker_presets, workflow_runtime};

use super::commands::{WorkflowAction, WorkflowFactoryAction};

pub async fn handle_workflows(action: WorkflowAction, config: &Config) -> Result<()> {
    let repo = SpecRepository::new()?;
    match action {
        WorkflowAction::List => list_workflows(&repo),
        WorkflowAction::WorkerPresets => list_worker_presets(),
        WorkflowAction::Show { id } => show_workflow(&repo, &id),
        WorkflowAction::Create {
            id,
            name,
            description,
            step,
            agent,
            worker_preset,
            disabled,
        } => create_workflow(
            &repo,
            id,
            name,
            description,
            step,
            agent,
            worker_preset,
            disabled,
        ),
        WorkflowAction::Enable { id } => set_enabled(&repo, &id, true),
        WorkflowAction::Disable { id } => set_enabled(&repo, &id, false),
        WorkflowAction::Review { id } => review_workflow(&repo, &id),
        WorkflowAction::Approve { id } => approve_workflow(&repo, &id),
        WorkflowAction::Run { id, input } => {
            run_workflow(&repo, config, &id, input.join(" ")).await
        }
        WorkflowAction::ResumeRun { run_id } => resume_workflow_run(&repo, config, &run_id).await,
        WorkflowAction::Export { id, path } => export_workflow(&repo, &id, &path),
        WorkflowAction::Import { path } => import_workflow(&repo, &path),
        WorkflowAction::Runs { limit } => list_runs(config, limit).await,
        WorkflowAction::Factory { action } => handle_factory(action, &repo),
        WorkflowAction::FromTask { task_id, id, name } => {
            create_workflow_from_task(&repo, config, &task_id, id.as_deref(), name.as_deref()).await
        }
    }
}

fn handle_factory(action: WorkflowFactoryAction, repo: &SpecRepository) -> Result<()> {
    match action {
        WorkflowFactoryAction::Templates => {
            for template in factory::list_workflow_templates() {
                println!(
                    "{}\t{}\t{}",
                    template.id, template.name, template.description
                );
            }
            Ok(())
        }
        WorkflowFactoryAction::Preview {
            template,
            id,
            name,
            requirement,
        } => {
            let result = factory::preview_workflow_result(
                Some(repo),
                &requirement.join(" "),
                template.as_deref(),
                id.as_deref(),
                name.as_deref(),
            )?;
            println!("{}", toml::to_string_pretty(&result)?);
            Ok(())
        }
        WorkflowFactoryAction::Create {
            template,
            id,
            name,
            requirement,
        } => {
            let result = factory::create_workflow(
                repo,
                &requirement.join(" "),
                template.as_deref(),
                id.as_deref(),
                name.as_deref(),
            )?;
            println!("{}", toml::to_string_pretty(&result)?);
            Ok(())
        }
    }
}

fn list_workflows(repo: &SpecRepository) -> Result<()> {
    let specs = repo.list_workflows()?;
    if specs.is_empty() {
        println!("No workflows configured.");
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

fn list_worker_presets() -> Result<()> {
    for preset in worker_presets::list_worker_presets() {
        println!(
            "{}\t{}\titerations={}\t{}",
            preset.id,
            preset.name,
            preset
                .max_iterations
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unbounded".to_string()),
            preset.description
        );
    }
    Ok(())
}

fn show_workflow(repo: &SpecRepository, id: &str) -> Result<()> {
    let spec = repo.load_workflow(id)?;
    println!("{}", toml::to_string_pretty(&spec)?);
    Ok(())
}

fn create_workflow(
    repo: &SpecRepository,
    id: String,
    name: Option<String>,
    description: Option<String>,
    step: Vec<String>,
    agent: Vec<String>,
    worker_preset: Vec<String>,
    disabled: bool,
) -> Result<()> {
    if step.is_empty() {
        anyhow::bail!("Workflow creation requires at least one --step");
    }

    let steps = step
        .into_iter()
        .enumerate()
        .map(|(index, prompt)| WorkflowStepSpec {
            id: format!("step-{}", index + 1),
            name: format!("Step {}", index + 1),
            prompt,
            agent_id: agent.get(index).cloned(),
            worker_preset: worker_preset.get(index).cloned(),
            continue_on_error: false,
        })
        .collect();

    let spec = WorkflowSpec {
        id: slugify(&id),
        name: name.unwrap_or_else(|| id.clone()),
        description: description.unwrap_or_else(|| "Reusable Rove workflow".to_string()),
        enabled: !disabled,
        steps,
        ..WorkflowSpec::default()
    };
    let saved = repo.save_workflow(&spec)?;
    println!("Saved workflow {} ({})", saved.id, saved.name);
    Ok(())
}

fn set_enabled(repo: &SpecRepository, id: &str, enabled: bool) -> Result<()> {
    let mut spec = repo.load_workflow(id)?;
    spec.enabled = enabled;
    repo.save_workflow(&spec)?;
    println!(
        "Workflow '{}' {}",
        spec.id,
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

fn review_workflow(repo: &SpecRepository, id: &str) -> Result<()> {
    let review = factory::get_workflow_review(repo, id)?;
    println!("{}", toml::to_string_pretty(&review)?);
    Ok(())
}

fn approve_workflow(repo: &SpecRepository, id: &str) -> Result<()> {
    let saved = factory::approve_workflow(repo, id)?;
    println!("Approved workflow {} ({})", saved.id, saved.name);
    Ok(())
}

async fn run_workflow(
    repo: &SpecRepository,
    config: &Config,
    id: &str,
    input: String,
) -> Result<()> {
    if input.trim().is_empty() {
        anyhow::bail!("Workflow run requires a non-empty input");
    }

    let workflow = repo.load_workflow(id)?;
    if !workflow.enabled {
        anyhow::bail!("Workflow '{}' is disabled", workflow.id);
    }

    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for workflow run")?;
    let result = workflow_runtime::start_new_run(repo, &db, config, &workflow, &input).await?;
    println!("{}", result.final_output);
    println!("Workflow Run ID: {}", result.run.run_id);
    Ok(())
}

async fn resume_workflow_run(repo: &SpecRepository, config: &Config, run_id: &str) -> Result<()> {
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for workflow resume")?;
    let result = workflow_runtime::resume_run(repo, &db, config, run_id).await?;
    println!("{}", result.final_output);
    println!("Workflow Run ID: {}", result.run.run_id);
    println!(
        "Progress: {}/{} steps, retries={}",
        result.run.steps_completed, result.run.steps_total, result.run.retry_count
    );
    Ok(())
}

fn export_workflow(repo: &SpecRepository, id: &str, path: &std::path::Path) -> Result<()> {
    let target = repo.export_workflow(id, path)?;
    println!("Exported workflow to {}", target.display());
    Ok(())
}

fn import_workflow(repo: &SpecRepository, path: &std::path::Path) -> Result<()> {
    let spec = repo.import_workflow(path)?;
    println!("Imported workflow {} ({})", spec.id, spec.name);
    Ok(())
}

async fn list_runs(config: &Config, limit: i64) -> Result<()> {
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for run listing")?;
    for run in db.agent_runs().list_workflow_runs(limit).await? {
        println!(
            "{}\t{}\t{:?}\t{}/{}\tretries={}\t{}",
            run.run_id,
            run.workflow_id,
            run.status,
            run.steps_completed,
            run.steps_total,
            run.retry_count,
            run.input
        );
    }
    Ok(())
}

async fn create_workflow_from_task(
    repo: &SpecRepository,
    config: &Config,
    task_id: &str,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<()> {
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for task conversion")?;
    let result = factory::workflow_from_task(repo, &db, task_id, id, name).await?;
    println!("{}", toml::to_string_pretty(&result)?);
    Ok(())
}
