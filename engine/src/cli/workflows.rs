use anyhow::{Context, Result};
use sdk::{SpecRunStatus, TaskExecutionProfile, WorkflowSpec, WorkflowStepSpec};
use uuid::Uuid;

use crate::cli::database_path::database_path;
use crate::cli::run::execute_local_task_request;
use crate::config::Config;
use crate::storage::Database;
use crate::system::specs::{allowed_tools, slugify, SpecRepository};

use super::commands::WorkflowAction;

pub async fn handle_workflows(action: WorkflowAction, config: &Config) -> Result<()> {
    let repo = SpecRepository::new()?;
    match action {
        WorkflowAction::List => list_workflows(&repo),
        WorkflowAction::Show { id } => show_workflow(&repo, &id),
        WorkflowAction::Create {
            id,
            name,
            description,
            step,
            agent,
            disabled,
        } => create_workflow(&repo, id, name, description, step, agent, disabled),
        WorkflowAction::Enable { id } => set_enabled(&repo, &id, true),
        WorkflowAction::Disable { id } => set_enabled(&repo, &id, false),
        WorkflowAction::Run { id, input } => run_workflow(&repo, config, &id, input.join(" ")).await,
        WorkflowAction::Export { id, path } => export_workflow(&repo, &id, &path),
        WorkflowAction::Import { path } => import_workflow(&repo, &path),
        WorkflowAction::Runs { limit } => list_runs(config, limit).await,
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

    let run_id = Uuid::new_v4().to_string();
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for workflow run")?;
    db.agent_runs()
        .start_workflow_run(&run_id, &workflow.id, &input)
        .await?;

    let mut last_output = input.clone();
    for step in &workflow.steps {
        let prompt = render_step_prompt(&step.prompt, &input, &last_output);
        let execution_profile = if let Some(agent_id) = step.agent_id.as_deref() {
            let spec = repo.load_agent(agent_id)?;
            Some(TaskExecutionProfile {
                agent_id: Some(spec.id.clone()),
                agent_name: Some(spec.name.clone()),
                purpose: Some(spec.purpose.clone()),
                instructions: spec.instructions.clone(),
                allowed_tools: allowed_tools(&spec),
                output_contract: spec.output_contract.clone(),
            })
        } else {
            None
        };

        match execute_local_task_request(
            prompt,
            config,
            sdk::RunMode::Serial,
            sdk::RunIsolation::None,
            execution_profile,
        )
        .await
        {
            Ok(task_result) => {
                last_output = task_result.answer;
            }
            Err(error) => {
                db.agent_runs()
                    .finish_workflow_run(
                        &run_id,
                        SpecRunStatus::Failed,
                        None,
                        Some(&error.to_string()),
                    )
                    .await?;
                return Err(error);
            }
        }
    }

    db.agent_runs()
        .finish_workflow_run(&run_id, SpecRunStatus::Completed, Some(&last_output), None)
        .await?;
    println!("{}", last_output);
    println!("Workflow Run ID: {}", run_id);
    Ok(())
}

fn render_step_prompt(template: &str, input: &str, last_output: &str) -> String {
    template
        .replace("{{input}}", input)
        .replace("{{last_output}}", last_output)
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
            "{}\t{}\t{:?}\t{}",
            run.run_id, run.workflow_id, run.status, run.input
        );
    }
    Ok(())
}
