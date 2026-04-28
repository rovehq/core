use anyhow::{bail, Context, Result};
use sdk::{ChannelBinding, FileWatchBinding, WebhookBinding, WorkflowSpec, WorkflowStepSpec};

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::Database;
use crate::system::specs::{slugify, SpecRepository};
use crate::system::{factory, worker_presets, workflow_runtime};

use super::commands::{
    WorkflowAction, WorkflowFactoryAction, WorkflowFileWatchTriggerAction, WorkflowTriggerAction,
    WorkflowWebhookTriggerAction,
};

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
        WorkflowAction::CancelRun { run_id } => cancel_workflow_run(config, &run_id).await,
        WorkflowAction::Export { id, path } => export_workflow(&repo, &id, &path),
        WorkflowAction::Import { path } => import_workflow(&repo, &path),
        WorkflowAction::Runs { limit } => list_runs(config, limit).await,
        WorkflowAction::Factory { action } => handle_factory(action, &repo),
        WorkflowAction::FromTask { task_id, id, name } => {
            create_workflow_from_task(&repo, config, &task_id, id.as_deref(), name.as_deref()).await
        }
        WorkflowAction::Trigger { action } => handle_trigger_action(&repo, action),
    }
}

fn handle_trigger_action(repo: &SpecRepository, action: WorkflowTriggerAction) -> Result<()> {
    match action {
        WorkflowTriggerAction::List { id } => list_channel_triggers(repo, &id),
        WorkflowTriggerAction::Add {
            id,
            channel,
            target,
        } => add_channel_trigger(repo, &id, &channel, target.as_deref()),
        WorkflowTriggerAction::Remove {
            id,
            channel,
            target,
        } => remove_channel_trigger(repo, &id, &channel, target.as_deref()),
        WorkflowTriggerAction::Webhook { action } => handle_webhook_trigger_action(repo, action),
        WorkflowTriggerAction::FileWatch { action } => {
            handle_file_watch_trigger_action(repo, action)
        }
    }
}

fn handle_webhook_trigger_action(
    repo: &SpecRepository,
    action: WorkflowWebhookTriggerAction,
) -> Result<()> {
    match action {
        WorkflowWebhookTriggerAction::List { id } => list_webhook_triggers(repo, &id),
        WorkflowWebhookTriggerAction::Add {
            id,
            webhook,
            secret,
        } => add_webhook_trigger(repo, &id, &webhook, secret.as_deref()),
        WorkflowWebhookTriggerAction::Remove { id, webhook } => {
            remove_webhook_trigger(repo, &id, &webhook)
        }
    }
}

fn handle_file_watch_trigger_action(
    repo: &SpecRepository,
    action: WorkflowFileWatchTriggerAction,
) -> Result<()> {
    match action {
        WorkflowFileWatchTriggerAction::List { id } => list_file_watch_triggers(repo, &id),
        WorkflowFileWatchTriggerAction::Add {
            id,
            path,
            recursive,
            event,
        } => add_file_watch_trigger(repo, &id, &path, recursive, &event),
        WorkflowFileWatchTriggerAction::Remove { id, path } => {
            remove_file_watch_trigger(repo, &id, &path)
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

fn list_channel_triggers(repo: &SpecRepository, id: &str) -> Result<()> {
    let spec = repo.load_workflow(id)?;
    if spec.channels.is_empty() {
        println!("Workflow '{}' has no channel triggers.", spec.id);
        return Ok(());
    }

    println!("Channel triggers for '{}':", spec.id);
    for binding in spec.channels {
        println!(
            "{}\t{}\t{}",
            binding.kind,
            binding.target.unwrap_or_else(|| "*".to_string()),
            if binding.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
    Ok(())
}

fn add_channel_trigger(
    repo: &SpecRepository,
    id: &str,
    channel: &str,
    target: Option<&str>,
) -> Result<()> {
    let kind = channel.trim().to_ascii_lowercase();
    if kind.is_empty() {
        bail!("--channel must not be empty");
    }

    let mut spec = repo.load_workflow(id)?;
    let normalized_target = target
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if spec.channels.iter().any(|binding| {
        binding.kind.eq_ignore_ascii_case(&kind)
            && binding.target.as_deref().map(str::trim) == normalized_target.as_deref()
    }) {
        bail!(
            "Workflow '{}' already has channel trigger {}:{}",
            spec.id,
            kind,
            normalized_target.as_deref().unwrap_or("*")
        );
    }

    spec.channels.push(ChannelBinding {
        kind: kind.clone(),
        target: normalized_target.clone(),
        enabled: true,
        provenance: None,
    });
    spec.channels.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.target.cmp(&right.target))
    });
    repo.save_workflow(&spec)?;

    println!(
        "Added channel trigger {}:{} to workflow '{}'",
        kind,
        normalized_target.as_deref().unwrap_or("*"),
        spec.id
    );
    Ok(())
}

fn remove_channel_trigger(
    repo: &SpecRepository,
    id: &str,
    channel: &str,
    target: Option<&str>,
) -> Result<()> {
    let kind = channel.trim().to_ascii_lowercase();
    if kind.is_empty() {
        bail!("--channel must not be empty");
    }

    let mut spec = repo.load_workflow(id)?;
    let normalized_target = target
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let before = spec.channels.len();
    spec.channels.retain(|binding| {
        !(binding.kind.eq_ignore_ascii_case(&kind)
            && binding.target.as_deref().map(str::trim) == normalized_target.as_deref())
    });

    if spec.channels.len() == before {
        bail!(
            "Workflow '{}' has no channel trigger {}:{}",
            spec.id,
            kind,
            normalized_target.as_deref().unwrap_or("*")
        );
    }

    repo.save_workflow(&spec)?;
    println!(
        "Removed channel trigger {}:{} from workflow '{}'",
        kind,
        normalized_target.as_deref().unwrap_or("*"),
        spec.id
    );
    Ok(())
}

fn list_webhook_triggers(repo: &SpecRepository, id: &str) -> Result<()> {
    let spec = repo.load_workflow(id)?;
    if spec.webhooks.is_empty() {
        println!("Workflow '{}' has no webhook triggers.", spec.id);
        return Ok(());
    }

    println!("Webhook triggers for '{}':", spec.id);
    for binding in spec.webhooks {
        println!(
            "{}\t{}\t{}",
            binding.id,
            if binding
                .secret
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            {
                "secret"
            } else {
                "open"
            },
            if binding.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
    Ok(())
}

fn add_webhook_trigger(
    repo: &SpecRepository,
    id: &str,
    webhook: &str,
    secret: Option<&str>,
) -> Result<()> {
    let webhook_id = webhook.trim().to_ascii_lowercase();
    if webhook_id.is_empty() {
        bail!("--webhook must not be empty");
    }

    let mut spec = repo.load_workflow(id)?;
    if spec
        .webhooks
        .iter()
        .any(|binding| binding.id.eq_ignore_ascii_case(&webhook_id))
    {
        bail!(
            "Workflow '{}' already has webhook trigger '{}'",
            spec.id,
            webhook_id
        );
    }

    spec.webhooks.push(WebhookBinding {
        id: webhook_id.clone(),
        secret: secret
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        enabled: true,
        provenance: None,
    });
    spec.webhooks.sort_by(|left, right| left.id.cmp(&right.id));
    repo.save_workflow(&spec)?;

    println!(
        "Added webhook trigger '{}' to workflow '{}'",
        webhook_id, spec.id
    );
    Ok(())
}

fn remove_webhook_trigger(repo: &SpecRepository, id: &str, webhook: &str) -> Result<()> {
    let webhook_id = webhook.trim().to_ascii_lowercase();
    if webhook_id.is_empty() {
        bail!("--webhook must not be empty");
    }

    let mut spec = repo.load_workflow(id)?;
    let before = spec.webhooks.len();
    spec.webhooks
        .retain(|binding| !binding.id.eq_ignore_ascii_case(&webhook_id));

    if spec.webhooks.len() == before {
        bail!(
            "Workflow '{}' has no webhook trigger '{}'",
            spec.id,
            webhook_id
        );
    }

    repo.save_workflow(&spec)?;
    println!(
        "Removed webhook trigger '{}' from workflow '{}'",
        webhook_id, spec.id
    );
    Ok(())
}

fn list_file_watch_triggers(repo: &SpecRepository, id: &str) -> Result<()> {
    let spec = repo.load_workflow(id)?;
    if spec.file_watches.is_empty() {
        println!("Workflow '{}' has no file-watch triggers.", spec.id);
        return Ok(());
    }

    println!("File-watch triggers for '{}':", spec.id);
    for binding in spec.file_watches {
        let events = if binding.events.is_empty() {
            "any".to_string()
        } else {
            binding.events.join(",")
        };
        println!(
            "{}\t{}\t{}\t{}",
            binding.path,
            if binding.recursive {
                "recursive"
            } else {
                "direct"
            },
            events,
            if binding.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
    Ok(())
}

fn add_file_watch_trigger(
    repo: &SpecRepository,
    id: &str,
    path: &str,
    recursive: bool,
    events: &[String],
) -> Result<()> {
    let watch_path = path.trim();
    if watch_path.is_empty() {
        bail!("--path must not be empty");
    }

    let mut spec = repo.load_workflow(id)?;
    if spec
        .file_watches
        .iter()
        .any(|binding| binding.path.eq_ignore_ascii_case(watch_path))
    {
        bail!(
            "Workflow '{}' already has file-watch trigger '{}'",
            spec.id,
            watch_path
        );
    }

    let normalized_events = normalize_watch_events(events)?;
    spec.file_watches.push(FileWatchBinding {
        path: watch_path.to_string(),
        recursive,
        events: normalized_events,
        enabled: true,
        provenance: None,
    });
    spec.file_watches
        .sort_by(|left, right| left.path.cmp(&right.path));
    repo.save_workflow(&spec)?;

    println!(
        "Added file-watch trigger '{}' to workflow '{}'",
        watch_path, spec.id
    );
    Ok(())
}

fn remove_file_watch_trigger(repo: &SpecRepository, id: &str, path: &str) -> Result<()> {
    let watch_path = path.trim();
    if watch_path.is_empty() {
        bail!("--path must not be empty");
    }

    let mut spec = repo.load_workflow(id)?;
    let before = spec.file_watches.len();
    spec.file_watches
        .retain(|binding| !binding.path.eq_ignore_ascii_case(watch_path));

    if spec.file_watches.len() == before {
        bail!(
            "Workflow '{}' has no file-watch trigger '{}'",
            spec.id,
            watch_path
        );
    }

    repo.save_workflow(&spec)?;
    println!(
        "Removed file-watch trigger '{}' from workflow '{}'",
        watch_path, spec.id
    );
    Ok(())
}

fn normalize_watch_events(events: &[String]) -> Result<Vec<String>> {
    let mut normalized = Vec::new();
    for event in events {
        let value = event.trim().to_ascii_lowercase();
        if value.is_empty() {
            continue;
        }
        match value.as_str() {
            "any" | "create" | "modify" | "remove" => {}
            _ => bail!("Unsupported file-watch event '{}'", value),
        }
        if !normalized.iter().any(|existing| existing == &value) {
            normalized.push(value);
        }
    }
    Ok(normalized)
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
            thread_key: None,
            outcome_contract: None,
            continue_on_error: false,
            branches: Vec::new(),
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

async fn cancel_workflow_run(config: &Config, run_id: &str) -> Result<()> {
    let db = Database::new(&database_path(config))
        .await
        .context("Failed to open database for workflow cancellation")?;
    if db.agent_runs().request_workflow_run_cancel(run_id).await? {
        println!("Requested cancel for workflow run {}", run_id);
        return Ok(());
    }

    anyhow::bail!(
        "Workflow run '{}' was not found or is already settled",
        run_id
    )
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
            "{}\t{}\t{:?}\t{}/{}\tretries={}\tcancel_requested={}\t{}",
            run.run_id,
            run.workflow_id,
            run.status,
            run.steps_completed,
            run.steps_total,
            run.retry_count,
            run.cancel_requested,
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
