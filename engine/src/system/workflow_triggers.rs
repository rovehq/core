use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::storage::Database;
use crate::system::specs::SpecRepository;
use crate::system::workflow_runtime;
use sdk::{ChannelBinding, FileWatchBinding, WebhookBinding, WorkflowSpec};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkflowTriggerMatch {
    pub workflow_id: String,
    pub workflow_name: String,
    pub binding_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriggeredWorkflowRun {
    pub workflow_id: String,
    pub workflow_name: String,
    pub binding_target: Option<String>,
    pub run_id: String,
    pub final_output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWatchRegistration {
    pub workflow_id: String,
    pub workflow_name: String,
    pub path: PathBuf,
    pub recursive: bool,
}

pub fn default_channel_targets(extra: Option<&str>) -> Vec<String> {
    let mut targets = vec!["default".to_string()];
    if let Some(value) = extra.map(str::trim).filter(|value| !value.is_empty()) {
        if !targets
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(value))
        {
            targets.push(value.to_string());
        }
    }
    targets
}

pub fn list_matching_workflows(
    repo: &SpecRepository,
    channel_kind: &str,
    targets: &[String],
) -> Result<Vec<WorkflowTriggerMatch>> {
    let normalized_targets = normalize_targets(targets);
    let mut matches = Vec::new();

    for workflow in repo.list_workflows()? {
        if !workflow.enabled {
            continue;
        }

        let binding_target = workflow
            .channels
            .iter()
            .find(|binding| binding_matches(binding, channel_kind, &normalized_targets))
            .and_then(|binding| binding.target.clone());

        if binding_target.is_some() || has_wildcard_binding(&workflow, channel_kind) {
            matches.push(WorkflowTriggerMatch {
                workflow_id: workflow.id.clone(),
                workflow_name: workflow.name.clone(),
                binding_target,
            });
        }
    }

    matches.sort_by(|left, right| left.workflow_name.cmp(&right.workflow_name));
    Ok(matches)
}

pub fn webhook_binding_exists(repo: &SpecRepository, webhook_id: &str) -> Result<bool> {
    let expected = webhook_id.trim();
    if expected.is_empty() {
        return Ok(false);
    }

    Ok(repo.list_workflows()?.into_iter().any(|workflow| {
        workflow.enabled
            && workflow
                .webhooks
                .iter()
                .any(|binding| binding.enabled && binding.id.eq_ignore_ascii_case(expected))
    }))
}

pub fn list_matching_webhook_workflows(
    repo: &SpecRepository,
    webhook_id: &str,
    provided_secret: Option<&str>,
) -> Result<Vec<WorkflowTriggerMatch>> {
    let expected = webhook_id.trim();
    let mut matches = Vec::new();

    for workflow in repo.list_workflows()? {
        if !workflow.enabled {
            continue;
        }

        if workflow
            .webhooks
            .iter()
            .any(|binding| webhook_binding_matches(binding, expected, provided_secret))
        {
            matches.push(WorkflowTriggerMatch {
                workflow_id: workflow.id.clone(),
                workflow_name: workflow.name.clone(),
                binding_target: Some(expected.to_string()),
            });
        }
    }

    matches.sort_by(|left, right| left.workflow_name.cmp(&right.workflow_name));
    Ok(matches)
}

pub async fn trigger_matching_webhook_workflows(
    repo: &SpecRepository,
    db: &Database,
    config: &Config,
    webhook_id: &str,
    provided_secret: Option<&str>,
    input: &str,
) -> Result<Vec<TriggeredWorkflowRun>> {
    let matches = list_matching_webhook_workflows(repo, webhook_id, provided_secret)?;
    let mut runs = Vec::new();

    for matched in matches {
        let workflow = repo.load_workflow(&matched.workflow_id)?;
        let result = workflow_runtime::start_new_run(repo, db, config, &workflow, input).await?;
        runs.push(TriggeredWorkflowRun {
            workflow_id: matched.workflow_id,
            workflow_name: matched.workflow_name,
            binding_target: matched.binding_target,
            run_id: result.run.run_id,
            final_output: result.final_output,
        });
    }

    Ok(runs)
}

pub async fn trigger_matching_workflows(
    repo: &SpecRepository,
    db: &Database,
    config: &Config,
    channel_kind: &str,
    targets: &[String],
    input: &str,
) -> Result<Vec<TriggeredWorkflowRun>> {
    let matches = list_matching_workflows(repo, channel_kind, targets)?;
    let mut runs = Vec::new();

    for matched in matches {
        let workflow = repo.load_workflow(&matched.workflow_id)?;
        let result = workflow_runtime::start_new_run(repo, db, config, &workflow, input).await?;
        runs.push(TriggeredWorkflowRun {
            workflow_id: matched.workflow_id,
            workflow_name: matched.workflow_name,
            binding_target: matched.binding_target,
            run_id: result.run.run_id,
            final_output: result.final_output,
        });
    }

    Ok(runs)
}

pub fn collect_file_watch_registrations(
    repo: &SpecRepository,
    workspace_root: &Path,
) -> Result<Vec<FileWatchRegistration>> {
    let mut registrations = Vec::new();

    for workflow in repo.list_workflows()? {
        if !workflow.enabled {
            continue;
        }

        for binding in workflow
            .file_watches
            .iter()
            .filter(|binding| binding.enabled)
        {
            let path = resolve_watch_path(workspace_root, &binding.path);
            registrations.push(FileWatchRegistration {
                workflow_id: workflow.id.clone(),
                workflow_name: workflow.name.clone(),
                path,
                recursive: binding.recursive,
            });
        }
    }

    registrations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.workflow_name.cmp(&right.workflow_name))
    });
    registrations.dedup_by(|left, right| {
        left.workflow_id == right.workflow_id
            && left.path == right.path
            && left.recursive == right.recursive
    });
    Ok(registrations)
}

pub fn list_matching_file_watch_workflows(
    repo: &SpecRepository,
    workspace_root: &Path,
    changed_path: &Path,
    event_kind: &str,
) -> Result<Vec<WorkflowTriggerMatch>> {
    let normalized_event = normalize_watch_event(event_kind);
    let mut matches = Vec::new();

    for workflow in repo.list_workflows()? {
        if !workflow.enabled {
            continue;
        }

        let binding_path = workflow
            .file_watches
            .iter()
            .find(|binding| {
                file_watch_binding_matches(binding, workspace_root, changed_path, &normalized_event)
            })
            .map(|binding| binding.path.clone());

        if let Some(binding_target) = binding_path {
            matches.push(WorkflowTriggerMatch {
                workflow_id: workflow.id.clone(),
                workflow_name: workflow.name.clone(),
                binding_target: Some(binding_target),
            });
        }
    }

    matches.sort_by(|left, right| left.workflow_name.cmp(&right.workflow_name));
    Ok(matches)
}

pub async fn trigger_matching_file_watch_workflows(
    repo: &SpecRepository,
    db: &Database,
    config: &Config,
    workspace_root: &Path,
    changed_path: &Path,
    event_kind: &str,
    input: &str,
) -> Result<Vec<TriggeredWorkflowRun>> {
    let matches =
        list_matching_file_watch_workflows(repo, workspace_root, changed_path, event_kind)?;
    let mut runs = Vec::new();

    for matched in matches {
        let workflow = repo.load_workflow(&matched.workflow_id)?;
        let result = workflow_runtime::start_new_run(repo, db, config, &workflow, input).await?;
        runs.push(TriggeredWorkflowRun {
            workflow_id: matched.workflow_id,
            workflow_name: matched.workflow_name,
            binding_target: matched.binding_target,
            run_id: result.run.run_id,
            final_output: result.final_output,
        });
    }

    Ok(runs)
}

fn has_wildcard_binding(workflow: &WorkflowSpec, channel_kind: &str) -> bool {
    workflow.channels.iter().any(|binding| {
        binding.enabled
            && binding.kind.eq_ignore_ascii_case(channel_kind)
            && binding
                .target
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
    })
}

fn binding_matches(binding: &ChannelBinding, channel_kind: &str, targets: &[String]) -> bool {
    if !binding.enabled || !binding.kind.eq_ignore_ascii_case(channel_kind) {
        return false;
    }

    let Some(target) = binding
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };

    targets
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(target))
}

fn normalize_targets(targets: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for target in targets {
        let value = target.trim();
        if value.is_empty() {
            continue;
        }
        if !normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(value))
        {
            normalized.push(value.to_string());
        }
    }
    normalized
}

pub fn normalize_watch_event(event_kind: &str) -> String {
    match event_kind.trim().to_ascii_lowercase().as_str() {
        "create" => "create".to_string(),
        "modify" => "modify".to_string(),
        "remove" => "remove".to_string(),
        _ => "any".to_string(),
    }
}

fn webhook_binding_matches(
    binding: &WebhookBinding,
    webhook_id: &str,
    provided_secret: Option<&str>,
) -> bool {
    if !binding.enabled || !binding.id.eq_ignore_ascii_case(webhook_id) {
        return false;
    }

    match binding
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(expected) => provided_secret
            .map(str::trim)
            .is_some_and(|provided| provided == expected),
        None => true,
    }
}

fn file_watch_binding_matches(
    binding: &FileWatchBinding,
    workspace_root: &Path,
    changed_path: &Path,
    event_kind: &str,
) -> bool {
    if !binding.enabled {
        return false;
    }

    if !binding.events.is_empty()
        && !binding.events.iter().any(|event| {
            let normalized = normalize_watch_event(event);
            normalized == "any" || normalized.eq_ignore_ascii_case(event_kind)
        })
    {
        return false;
    }

    let root = resolve_watch_path(workspace_root, &binding.path);
    if binding.recursive {
        changed_path == root || changed_path.starts_with(&root)
    } else {
        changed_path == root || changed_path.parent().is_some_and(|parent| parent == root)
    }
}

fn resolve_watch_path(workspace_root: &Path, configured_path: &str) -> PathBuf {
    let path = PathBuf::from(configured_path.trim());
    if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_file_watch_registrations, default_channel_targets,
        list_matching_file_watch_workflows, list_matching_webhook_workflows,
        list_matching_workflows,
    };
    use crate::system::specs::SpecRepository;
    use sdk::{ChannelBinding, FileWatchBinding, WebhookBinding, WorkflowSpec, WorkflowStepSpec};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn sample_workflow(id: &str) -> WorkflowSpec {
        WorkflowSpec {
            id: id.to_string(),
            name: id.to_string(),
            description: "test".to_string(),
            steps: vec![WorkflowStepSpec {
                id: "step-1".to_string(),
                name: "Step 1".to_string(),
                prompt: "echo".to_string(),
                agent_id: None,
                worker_preset: None,
                continue_on_error: false,
                branches: Vec::new(),
            }],
            ..WorkflowSpec::default()
        }
    }

    #[test]
    fn default_targets_include_default_and_extra_once() {
        let targets = default_channel_targets(Some("chat:123"));
        assert_eq!(targets, vec!["default".to_string(), "chat:123".to_string()]);

        let targets = default_channel_targets(Some("default"));
        assert_eq!(targets, vec!["default".to_string()]);
    }

    #[test]
    fn channel_matching_prefers_enabled_bindings() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let mut workflow = sample_workflow("wf-telegram");
        workflow.channels.push(ChannelBinding {
            kind: "telegram".to_string(),
            target: Some("default".to_string()),
            enabled: true,
            provenance: None,
        });
        repo.save_workflow(&workflow).unwrap();

        let mut disabled = sample_workflow("wf-disabled");
        disabled.channels.push(ChannelBinding {
            kind: "telegram".to_string(),
            target: Some("default".to_string()),
            enabled: false,
            provenance: None,
        });
        repo.save_workflow(&disabled).unwrap();

        let matches = list_matching_workflows(&repo, "telegram", &["default".to_string()]).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].workflow_id, "wf-telegram");
    }

    #[test]
    fn channel_matching_supports_specific_targets_and_wildcards() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let mut wildcard = sample_workflow("wf-any");
        wildcard.channels.push(ChannelBinding {
            kind: "telegram".to_string(),
            target: None,
            enabled: true,
            provenance: None,
        });
        repo.save_workflow(&wildcard).unwrap();

        let mut specific = sample_workflow("wf-chat");
        specific.channels.push(ChannelBinding {
            kind: "telegram".to_string(),
            target: Some("chat:42".to_string()),
            enabled: true,
            provenance: None,
        });
        repo.save_workflow(&specific).unwrap();

        let matches = list_matching_workflows(&repo, "telegram", &["chat:42".to_string()]).unwrap();
        assert_eq!(matches.len(), 2);
        assert!(matches
            .iter()
            .any(|matched| matched.workflow_id == "wf-any"));
        assert!(matches
            .iter()
            .any(|matched| matched.workflow_id == "wf-chat"));
    }

    #[test]
    fn webhook_matching_requires_secret_when_configured() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let mut workflow = sample_workflow("wf-webhook");
        workflow.webhooks.push(WebhookBinding {
            id: "github-push".to_string(),
            secret: Some("shared-secret".to_string()),
            enabled: true,
            provenance: None,
        });
        repo.save_workflow(&workflow).unwrap();

        let matches =
            list_matching_webhook_workflows(&repo, "github-push", Some("shared-secret")).unwrap();
        assert_eq!(matches.len(), 1);

        let missing = list_matching_webhook_workflows(&repo, "github-push", None).unwrap();
        assert!(missing.is_empty());
    }

    #[test]
    fn file_watch_matching_stays_out_of_general_workspace_paths() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let mut workflow = sample_workflow("wf-watch");
        workflow.file_watches.push(FileWatchBinding {
            path: "src".to_string(),
            recursive: true,
            events: vec!["modify".to_string()],
            enabled: true,
            provenance: None,
        });
        repo.save_workflow(&workflow).unwrap();

        let workspace_root = PathBuf::from("/tmp/project");
        let matches = list_matching_file_watch_workflows(
            &repo,
            &workspace_root,
            &workspace_root.join("src/lib.rs"),
            "modify",
        )
        .unwrap();
        assert_eq!(matches.len(), 1);

        let no_match = list_matching_file_watch_workflows(
            &repo,
            &workspace_root,
            &workspace_root.join("README.md"),
            "modify",
        )
        .unwrap();
        assert!(no_match.is_empty());
    }

    #[test]
    fn file_watch_registrations_resolve_relative_paths_against_workspace() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let mut workflow = sample_workflow("wf-relative");
        workflow.file_watches.push(FileWatchBinding {
            path: "docs".to_string(),
            recursive: false,
            events: Vec::new(),
            enabled: true,
            provenance: None,
        });
        repo.save_workflow(&workflow).unwrap();

        let registrations =
            collect_file_watch_registrations(&repo, &PathBuf::from("/tmp/project")).unwrap();
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].path, PathBuf::from("/tmp/project/docs"));
        assert!(!registrations[0].recursive);
    }
}
