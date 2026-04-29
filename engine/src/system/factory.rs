use std::collections::BTreeSet;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

use crate::specs::{slugify, SpecRepository};
use crate::storage::{Database, TaskRepository};

#[derive(Debug, Clone, Serialize)]
pub struct TemplateSummary {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub fn list_agent_templates() -> Vec<TemplateSummary> {
    vec![
        TemplateSummary {
            id: "general-assistant".to_string(),
            name: "General Assistant".to_string(),
            description: "Balanced local agent for common filesystem and command tasks."
                .to_string(),
        },
        TemplateSummary {
            id: "research-runner".to_string(),
            name: "Research Runner".to_string(),
            description: "Read-focused agent for investigation, summaries, and verification."
                .to_string(),
        },
        TemplateSummary {
            id: "telegram-support".to_string(),
            name: "Telegram Support".to_string(),
            description: "Default inbound Telegram support/triage handler.".to_string(),
        },
    ]
}

pub fn list_workflow_templates() -> Vec<TemplateSummary> {
    vec![
        TemplateSummary {
            id: "one-shot".to_string(),
            name: "One Shot".to_string(),
            description: "Single-step workflow that forwards the current input.".to_string(),
        },
        TemplateSummary {
            id: "research-pipeline".to_string(),
            name: "Research Pipeline".to_string(),
            description: "Survey, analyze, and summarize a topic across three stages.".to_string(),
        },
        TemplateSummary {
            id: "remote-ops".to_string(),
            name: "Remote Ops".to_string(),
            description: "Inspect, change, and verify an operational target.".to_string(),
        },
    ]
}

pub fn preview_agent(
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::AgentSpec> {
    let requirement = requirement.trim();
    if requirement.is_empty() {
        anyhow::bail!("Agent factory requires a non-empty requirement");
    }

    let template = template_id.unwrap_or("general-assistant");
    let mut spec = agent_template(template)?;
    let inferred_tools = infer_tools_from_text(requirement);

    if !inferred_tools.is_empty() {
        spec.capabilities = merge_tool_capabilities(spec.capabilities, inferred_tools);
    }

    spec.id = slugify(id.unwrap_or_else(|| name.unwrap_or(requirement)));
    if spec.id.is_empty() {
        spec.id = "generated-agent".to_string();
    }
    spec.name = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| title_case(&spec.id));
    spec.purpose = summarize_requirement(requirement);
    spec.instructions = build_agent_instructions(requirement, &spec.instructions);
    spec.enabled = false;
    spec.tags = dedupe_tags(
        spec.tags
            .into_iter()
            .chain(["generated".to_string(), format!("template:{template}")])
            .collect(),
    );
    spec.provenance = Some(sdk::SpecProvenance {
        source: Some("generated".to_string()),
        import_source: None,
        notes: Some(format!(
            "Generated from requirement using template {}",
            template
        )),
        imported_at: Some(Utc::now().timestamp()),
        draft_for: None,
        review_status: None,
        reviewed_at: None,
    });

    let requirement_lower = requirement.to_ascii_lowercase();
    if requirement_lower.contains("telegram")
        && !spec.channels.iter().any(|binding| {
            binding.kind == "telegram" && binding.target.as_deref() == Some("default")
        })
    {
        spec.channels.push(sdk::ChannelBinding {
            kind: "telegram".to_string(),
            target: Some("default".to_string()),
            enabled: true,
            provenance: None,
        });
    }
    if mentions_remote_execution(requirement) {
        spec.runtime_profile = Some("headless".to_string());
        spec.node_placement.require_executor = true;
    }

    Ok(spec)
}

pub fn preview_agent_result(
    repo: Option<&SpecRepository>,
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::AgentFactoryResult> {
    let spec = preview_agent(requirement, template_id, id, name)?;
    let review = build_agent_review(repo, &spec, None)?;
    Ok(sdk::AgentFactoryResult { spec, review })
}

pub fn create_agent(
    repo: &SpecRepository,
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::AgentFactoryResult> {
    let spec = preview_agent(requirement, template_id, id, name)?;
    save_agent_draft(repo, spec)
}

pub async fn agent_from_task(
    repo: &SpecRepository,
    database: &Database,
    task_id: &str,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::AgentFactoryResult> {
    let task_uuid = uuid::Uuid::parse_str(task_id).context("Invalid task id")?;
    let task_repo = TaskRepository::new(database.pool().clone());
    let task = task_repo
        .get_task(&task_uuid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' was not found", task_id))?;
    let events = task_repo
        .get_agent_events(task_id)
        .await
        .unwrap_or_default();
    let answer = task_repo.get_latest_answer(task_id).await.unwrap_or(None);

    let inferred_tools = extract_tool_names(&events);
    let mut spec = preview_agent(&task.input, Some("general-assistant"), id, name)?;
    if !inferred_tools.is_empty() {
        spec.capabilities =
            merge_tool_capabilities(Vec::new(), inferred_tools.into_iter().collect());
    }
    spec.instructions = build_from_task_agent_instructions(&task.input, answer.as_deref());
    spec.tags = dedupe_tags(
        spec.tags
            .into_iter()
            .chain(["from-task".to_string(), format!("task:{task_id}")])
            .collect(),
    );
    spec.provenance = Some(sdk::SpecProvenance {
        source: Some("from_task".to_string()),
        import_source: None,
        notes: Some(format!("Generated from task {}", task_id)),
        imported_at: Some(Utc::now().timestamp()),
        draft_for: None,
        review_status: None,
        reviewed_at: None,
    });
    save_agent_draft(repo, spec)
}

pub fn preview_workflow(
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::WorkflowSpec> {
    let requirement = requirement.trim();
    if requirement.is_empty() {
        anyhow::bail!("Workflow factory requires a non-empty requirement");
    }

    let template = template_id.unwrap_or("one-shot");
    let mut spec = workflow_template(template)?;
    let inferred_steps = infer_workflow_steps(requirement);
    if !inferred_steps.is_empty() {
        spec.steps = inferred_steps;
    }

    spec.id = slugify(id.unwrap_or_else(|| name.unwrap_or(requirement)));
    if spec.id.is_empty() {
        spec.id = "generated-workflow".to_string();
    }
    spec.name = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| title_case(&spec.id));
    spec.description = summarize_requirement(requirement);
    spec.enabled = false;
    spec.tags = dedupe_tags(
        spec.tags
            .into_iter()
            .chain(["generated".to_string(), format!("template:{template}")])
            .collect(),
    );
    spec.provenance = Some(sdk::SpecProvenance {
        source: Some("generated".to_string()),
        import_source: None,
        notes: Some(format!(
            "Generated from requirement using template {}",
            template
        )),
        imported_at: Some(Utc::now().timestamp()),
        draft_for: None,
        review_status: None,
        reviewed_at: None,
    });
    if mentions_remote_execution(requirement) {
        spec.runtime_profile = Some("headless".to_string());
    }

    Ok(spec)
}

pub fn preview_workflow_result(
    repo: Option<&SpecRepository>,
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::WorkflowFactoryResult> {
    let spec = preview_workflow(requirement, template_id, id, name)?;
    let review = build_workflow_review(repo, &spec, None)?;
    Ok(sdk::WorkflowFactoryResult { spec, review })
}

pub fn create_workflow(
    repo: &SpecRepository,
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::WorkflowFactoryResult> {
    let spec = preview_workflow(requirement, template_id, id, name)?;
    save_workflow_draft(repo, spec)
}

pub async fn workflow_from_task(
    repo: &SpecRepository,
    database: &Database,
    task_id: &str,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::WorkflowFactoryResult> {
    let task_uuid = uuid::Uuid::parse_str(task_id).context("Invalid task id")?;
    let task_repo = TaskRepository::new(database.pool().clone());
    let task = task_repo
        .get_task(&task_uuid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' was not found", task_id))?;
    let events = task_repo
        .get_agent_events(task_id)
        .await
        .unwrap_or_default();
    let tool_names = extract_tool_names(&events);

    let steps = if tool_names.len() > 1 {
        tool_names
            .iter()
            .enumerate()
            .map(|(index, tool_name)| sdk::WorkflowStepSpec {
                id: format!("step-{}", index + 1),
                name: title_case(tool_name),
                prompt: format!(
                    "For this workflow input, handle the `{tool_name}` portion of the task.\n\nReference objective:\n{}",
                    task.input
                ),
                agent_id: None,
                worker_preset: Some(worker_preset_for_tool(tool_name).to_string()),
                thread_key: None,
                outcome_contract: None,
                continue_on_error: false,
                branches: Vec::new(),
            })
            .collect()
    } else {
        vec![sdk::WorkflowStepSpec {
            id: "step-1".to_string(),
            name: "Replay Task".to_string(),
            prompt: format!(
                "Complete this task:\n{{{{input}}}}\n\nUse this successful task as a pattern:\n{}",
                task.input
            ),
            agent_id: None,
            worker_preset: Some("executor".to_string()),
            thread_key: None,
            outcome_contract: None,
            continue_on_error: false,
            branches: Vec::new(),
        }]
    };

    let mut spec = sdk::WorkflowSpec {
        id: slugify(id.unwrap_or_else(|| name.unwrap_or(&task.input))),
        name: name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| title_case(&slugify(&task.input))),
        description: format!("Workflow generated from task {}", task_id),
        enabled: false,
        steps,
        tags: vec![
            "generated".to_string(),
            "from-task".to_string(),
            format!("task:{task_id}"),
        ],
        ..sdk::WorkflowSpec::default()
    };
    if spec.id.is_empty() {
        spec.id = "generated-workflow".to_string();
    }
    spec.tags = dedupe_tags(spec.tags);
    spec.provenance = Some(sdk::SpecProvenance {
        source: Some("from_task".to_string()),
        import_source: None,
        notes: Some(format!("Generated from task {}", task_id)),
        imported_at: Some(Utc::now().timestamp()),
        draft_for: None,
        review_status: None,
        reviewed_at: None,
    });
    save_workflow_draft(repo, spec)
}

pub fn get_agent_review(repo: &SpecRepository, selector: &str) -> Result<sdk::FactoryReview> {
    let spec = repo.load_agent(selector)?;
    build_agent_review(Some(repo), &spec, Some(spec.id.clone()))
}

pub fn approve_agent(repo: &SpecRepository, selector: &str) -> Result<sdk::AgentSpec> {
    let draft = repo.load_agent(selector)?;
    if !is_draft_provenance(draft.provenance.as_ref()) {
        anyhow::bail!("Agent '{}' is not a pending draft", draft.id);
    }

    let target_id = target_id_for_spec(&draft.id, draft.provenance.as_ref());
    let existing = find_agent_by_id(repo, &target_id)?;
    let mut approved = draft.clone();
    approved.id = target_id;
    approved.enabled = existing
        .as_ref()
        .map(|spec| spec.enabled)
        .unwrap_or(draft.enabled);

    let mut provenance = approved.provenance.clone().unwrap_or_default();
    provenance.draft_for = None;
    provenance.review_status = Some("approved".to_string());
    provenance.reviewed_at = Some(Utc::now().timestamp());
    approved.provenance = Some(provenance);

    let saved = repo.save_agent(&approved)?;
    if draft.id != saved.id {
        let _ = repo.remove_agent(&draft.id)?;
    }
    Ok(saved)
}

pub fn get_workflow_review(repo: &SpecRepository, selector: &str) -> Result<sdk::FactoryReview> {
    let spec = repo.load_workflow(selector)?;
    build_workflow_review(Some(repo), &spec, Some(spec.id.clone()))
}

pub fn approve_workflow(repo: &SpecRepository, selector: &str) -> Result<sdk::WorkflowSpec> {
    let draft = repo.load_workflow(selector)?;
    if !is_draft_provenance(draft.provenance.as_ref()) {
        anyhow::bail!("Workflow '{}' is not a pending draft", draft.id);
    }

    let target_id = target_id_for_spec(&draft.id, draft.provenance.as_ref());
    let existing = find_workflow_by_id(repo, &target_id)?;
    let mut approved = draft.clone();
    approved.id = target_id;
    approved.enabled = existing
        .as_ref()
        .map(|spec| spec.enabled)
        .unwrap_or(draft.enabled);

    let mut provenance = approved.provenance.clone().unwrap_or_default();
    provenance.draft_for = None;
    provenance.review_status = Some("approved".to_string());
    provenance.reviewed_at = Some(Utc::now().timestamp());
    approved.provenance = Some(provenance);

    let saved = repo.save_workflow(&approved)?;
    if draft.id != saved.id {
        let _ = repo.remove_workflow(&draft.id)?;
    }
    Ok(saved)
}

fn save_agent_draft(
    repo: &SpecRepository,
    mut spec: sdk::AgentSpec,
) -> Result<sdk::AgentFactoryResult> {
    let target_id = spec.id.clone();
    let draft_id = unique_agent_draft_id(repo, &target_id)?;
    spec.id = draft_id.clone();
    spec.enabled = false;

    let mut provenance = spec.provenance.clone().unwrap_or_default();
    provenance.draft_for = Some(target_id);
    provenance.review_status = Some("draft".to_string());
    provenance.reviewed_at = None;
    if provenance.imported_at.is_none() {
        provenance.imported_at = Some(Utc::now().timestamp());
    }
    spec.provenance = Some(provenance);

    let saved = repo.save_agent(&spec)?;
    let review = build_agent_review(Some(repo), &saved, Some(draft_id))?;
    Ok(sdk::AgentFactoryResult {
        spec: saved,
        review,
    })
}

fn save_workflow_draft(
    repo: &SpecRepository,
    mut spec: sdk::WorkflowSpec,
) -> Result<sdk::WorkflowFactoryResult> {
    let target_id = spec.id.clone();
    let draft_id = unique_workflow_draft_id(repo, &target_id)?;
    spec.id = draft_id.clone();
    spec.enabled = false;

    let mut provenance = spec.provenance.clone().unwrap_or_default();
    provenance.draft_for = Some(target_id);
    provenance.review_status = Some("draft".to_string());
    provenance.reviewed_at = None;
    if provenance.imported_at.is_none() {
        provenance.imported_at = Some(Utc::now().timestamp());
    }
    spec.provenance = Some(provenance);

    let saved = repo.save_workflow(&spec)?;
    let review = build_workflow_review(Some(repo), &saved, Some(draft_id))?;
    Ok(sdk::WorkflowFactoryResult {
        spec: saved,
        review,
    })
}

fn build_agent_review(
    repo: Option<&SpecRepository>,
    spec: &sdk::AgentSpec,
    draft_id: Option<String>,
) -> Result<sdk::FactoryReview> {
    let target_id = target_id_for_spec(&spec.id, spec.provenance.as_ref());
    let current = match repo {
        Some(repo) => find_agent_by_id(repo, &target_id)?,
        None => None,
    };
    let target_exists = current.is_some();
    let current_value = current
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or(Value::Null);
    let proposed_value = serde_json::to_value(spec)?;
    let changes = diff_structured_values("", &current_value, &proposed_value);
    let warnings = agent_review_warnings(spec, current.as_ref());

    Ok(sdk::FactoryReview {
        kind: "agent".to_string(),
        target_id: target_id.clone(),
        draft_id,
        target_exists,
        review_status: review_status_for_spec(spec.provenance.as_ref()),
        suggested_action: if is_draft_provenance(spec.provenance.as_ref()) {
            if target_exists {
                "approve_update".to_string()
            } else {
                "approve_create".to_string()
            }
        } else if target_exists {
            "create_draft_update".to_string()
        } else {
            "create_draft".to_string()
        },
        summary: if is_draft_provenance(spec.provenance.as_ref()) {
            if target_exists {
                format!("Review this draft before updating agent '{}'.", target_id)
            } else {
                format!("Review this draft before creating agent '{}'.", target_id)
            }
        } else if target_exists {
            format!(
                "Factory output would update existing agent '{}'.",
                target_id
            )
        } else {
            format!("Factory output would create new agent '{}'.", target_id)
        },
        warnings,
        changes,
    })
}

fn build_workflow_review(
    repo: Option<&SpecRepository>,
    spec: &sdk::WorkflowSpec,
    draft_id: Option<String>,
) -> Result<sdk::FactoryReview> {
    let target_id = target_id_for_spec(&spec.id, spec.provenance.as_ref());
    let current = match repo {
        Some(repo) => find_workflow_by_id(repo, &target_id)?,
        None => None,
    };
    let target_exists = current.is_some();
    let current_value = current
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or(Value::Null);
    let proposed_value = serde_json::to_value(spec)?;
    let changes = diff_structured_values("", &current_value, &proposed_value);
    let warnings = workflow_review_warnings(spec, current.as_ref());

    Ok(sdk::FactoryReview {
        kind: "workflow".to_string(),
        target_id: target_id.clone(),
        draft_id,
        target_exists,
        review_status: review_status_for_spec(spec.provenance.as_ref()),
        suggested_action: if is_draft_provenance(spec.provenance.as_ref()) {
            if target_exists {
                "approve_update".to_string()
            } else {
                "approve_create".to_string()
            }
        } else if target_exists {
            "create_draft_update".to_string()
        } else {
            "create_draft".to_string()
        },
        summary: if is_draft_provenance(spec.provenance.as_ref()) {
            if target_exists {
                format!(
                    "Review this draft before updating workflow '{}'.",
                    target_id
                )
            } else {
                format!(
                    "Review this draft before creating workflow '{}'.",
                    target_id
                )
            }
        } else if target_exists {
            format!(
                "Factory output would update existing workflow '{}'.",
                target_id
            )
        } else {
            format!("Factory output would create new workflow '{}'.", target_id)
        },
        warnings,
        changes,
    })
}

fn find_agent_by_id(repo: &SpecRepository, id: &str) -> Result<Option<sdk::AgentSpec>> {
    Ok(repo.list_agents()?.into_iter().find(|spec| spec.id == id))
}

fn find_workflow_by_id(repo: &SpecRepository, id: &str) -> Result<Option<sdk::WorkflowSpec>> {
    Ok(repo
        .list_workflows()?
        .into_iter()
        .find(|spec| spec.id == id))
}

fn unique_agent_draft_id(repo: &SpecRepository, target_id: &str) -> Result<String> {
    let existing = repo
        .list_agents()?
        .into_iter()
        .map(|spec| spec.id)
        .collect::<BTreeSet<_>>();
    Ok(pick_draft_id(&existing, target_id))
}

fn unique_workflow_draft_id(repo: &SpecRepository, target_id: &str) -> Result<String> {
    let existing = repo
        .list_workflows()?
        .into_iter()
        .map(|spec| spec.id)
        .collect::<BTreeSet<_>>();
    Ok(pick_draft_id(&existing, target_id))
}

fn pick_draft_id(existing: &BTreeSet<String>, target_id: &str) -> String {
    let base = format!("{}-draft", slugify(target_id));
    if !existing.contains(&base) {
        return base;
    }

    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn target_id_for_spec(id: &str, provenance: Option<&sdk::SpecProvenance>) -> String {
    provenance
        .and_then(|item| item.draft_for.clone())
        .unwrap_or_else(|| id.to_string())
}

fn review_status_for_spec(provenance: Option<&sdk::SpecProvenance>) -> String {
    provenance
        .and_then(|item| item.review_status.clone())
        .unwrap_or_else(|| "preview".to_string())
}

fn is_draft_provenance(provenance: Option<&sdk::SpecProvenance>) -> bool {
    provenance
        .and_then(|item| item.review_status.as_deref())
        .is_some_and(|status| status == "draft")
        || provenance
            .and_then(|item| item.draft_for.as_deref())
            .is_some()
}

fn agent_review_warnings(spec: &sdk::AgentSpec, current: Option<&sdk::AgentSpec>) -> Vec<String> {
    let mut warnings = Vec::new();
    if spec.capabilities.is_empty() {
        warnings.push(
            "This agent has no bound capabilities, so it will have no tool access.".to_string(),
        );
    }
    let tool_names = spec
        .capabilities
        .iter()
        .filter(|capability| {
            matches!(
                capability.kind.trim().to_ascii_lowercase().as_str(),
                "tool" | "builtin" | "builtin_tool"
            )
        })
        .map(|capability| capability.name.as_str())
        .collect::<Vec<_>>();
    if spec.approval_mode.is_none()
        && tool_names
            .iter()
            .any(|tool| matches!(*tool, "write_file" | "run_command"))
    {
        warnings.push(
            "This agent can modify files or run commands without an explicit approval mode."
                .to_string(),
        );
    }
    if spec
        .channels
        .iter()
        .any(|binding| binding.kind == "telegram" && binding.target.as_deref() == Some("default"))
        && spec.runtime_profile.as_deref() != Some("headless")
    {
        warnings.push(
            "Telegram default handlers should usually use the headless runtime profile."
                .to_string(),
        );
    }
    if current.is_some_and(|item| item.enabled) {
        warnings.push(
            "Approving this draft will update an agent that is currently enabled.".to_string(),
        );
    }
    warnings
}

fn workflow_review_warnings(
    spec: &sdk::WorkflowSpec,
    current: Option<&sdk::WorkflowSpec>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if spec.steps.is_empty() {
        warnings.push("This workflow has no steps and cannot run.".to_string());
    }
    for step in &spec.steps {
        if step.agent_id.is_none() && step.worker_preset.is_none() {
            warnings.push(format!(
                "Step '{}' has no agent profile or worker preset, so it will run without bounded execution guidance.",
                step.name
            ));
        }
    }
    if current.is_some_and(|item| item.enabled) {
        warnings.push(
            "Approving this draft will update a workflow that is currently enabled.".to_string(),
        );
    }
    warnings
}

fn diff_structured_values(
    path: &str,
    current: &Value,
    proposed: &Value,
) -> Vec<sdk::FactoryFieldChange> {
    match (current, proposed) {
        (Value::Object(current_map), Value::Object(proposed_map)) => {
            let keys = current_map
                .keys()
                .chain(proposed_map.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            let mut changes = Vec::new();
            for key in keys {
                let next_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let current_value = current_map.get(&key).unwrap_or(&Value::Null);
                let proposed_value = proposed_map.get(&key).unwrap_or(&Value::Null);
                changes.extend(diff_structured_values(
                    &next_path,
                    current_value,
                    proposed_value,
                ));
            }
            changes
        }
        _ if current == proposed => Vec::new(),
        _ => vec![sdk::FactoryFieldChange {
            field: if path.is_empty() {
                "root".to_string()
            } else {
                path.to_string()
            },
            current: display_review_value(current),
            proposed: display_review_value(proposed),
        }],
    }
}

fn display_review_value(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    serde_json::to_string_pretty(value).ok()
}

fn agent_template(template_id: &str) -> Result<sdk::AgentSpec> {
    let mut spec = match template_id {
        "general-assistant" => sdk::AgentSpec {
            name: "General Assistant".to_string(),
            purpose: "General-purpose local assistant".to_string(),
            instructions: "Help the user complete tasks safely and directly. Prefer clear answers and use only the capabilities needed for the task.".to_string(),
            capabilities: tool_caps(&["read_file", "write_file", "list_dir", "file_exists", "run_command"]),
            tags: vec!["assistant".to_string()],
            ..sdk::AgentSpec::default()
        },
        "research-runner" => sdk::AgentSpec {
            name: "Research Runner".to_string(),
            purpose: "Read-focused investigation agent".to_string(),
            instructions: "Investigate, verify, and summarize. Stay read-oriented unless the user explicitly asks for changes.".to_string(),
            capabilities: tool_caps(&["read_file", "list_dir", "file_exists"]),
            tags: vec!["research".to_string()],
            ..sdk::AgentSpec::default()
        },
        "telegram-support" => sdk::AgentSpec {
            name: "Telegram Support".to_string(),
            purpose: "Telegram support and triage handler".to_string(),
            instructions: "Handle inbound support questions from Telegram. Ask concise clarifying questions when needed, summarize issues clearly, and keep responses short and operational.".to_string(),
            capabilities: tool_caps(&["read_file", "write_file", "list_dir", "file_exists", "run_command"]),
            channels: vec![sdk::ChannelBinding {
                kind: "telegram".to_string(),
                target: Some("default".to_string()),
                enabled: true,
                provenance: None,
            }],
            runtime_profile: Some("headless".to_string()),
            approval_mode: Some("allowlist".to_string()),
            tags: vec!["telegram".to_string(), "support".to_string()],
            ..sdk::AgentSpec::default()
        },
        other => anyhow::bail!("Unknown agent template '{}'", other),
    };
    spec.enabled = false;
    Ok(spec)
}

fn workflow_template(template_id: &str) -> Result<sdk::WorkflowSpec> {
    let spec = match template_id {
        "one-shot" => sdk::WorkflowSpec {
            name: "One Shot".to_string(),
            description: "Single-step workflow".to_string(),
            steps: vec![sdk::WorkflowStepSpec {
                id: "step-1".to_string(),
                name: "Run Input".to_string(),
                prompt: "{{input}}".to_string(),
                agent_id: None,
                worker_preset: Some("executor".to_string()),
                thread_key: None,
                outcome_contract: None,
                continue_on_error: false,
                branches: Vec::new(),
            }],
            ..sdk::WorkflowSpec::default()
        },
        "research-pipeline" => sdk::WorkflowSpec {
            name: "Research Pipeline".to_string(),
            description: "Survey, analyze, and summarize".to_string(),
            steps: vec![
                sdk::WorkflowStepSpec {
                    id: "step-1".to_string(),
                    name: "Survey".to_string(),
                    prompt: "Survey the topic:\n{{input}}".to_string(),
                    agent_id: None,
                    worker_preset: Some("researcher".to_string()),
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
                sdk::WorkflowStepSpec {
                    id: "step-2".to_string(),
                    name: "Analyze".to_string(),
                    prompt: "Analyze the survey findings:\n{{last_output}}".to_string(),
                    agent_id: None,
                    worker_preset: Some("researcher".to_string()),
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
                sdk::WorkflowStepSpec {
                    id: "step-3".to_string(),
                    name: "Summarize".to_string(),
                    prompt: "Produce the final summary from:\n{{last_output}}".to_string(),
                    agent_id: None,
                    worker_preset: Some("summariser".to_string()),
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
            ],
            tags: vec!["research".to_string()],
            ..sdk::WorkflowSpec::default()
        },
        "remote-ops" => sdk::WorkflowSpec {
            name: "Remote Ops".to_string(),
            description: "Inspect, change, and verify".to_string(),
            steps: vec![
                sdk::WorkflowStepSpec {
                    id: "step-1".to_string(),
                    name: "Inspect".to_string(),
                    prompt: "Inspect the target state for:\n{{input}}".to_string(),
                    agent_id: None,
                    worker_preset: Some("researcher".to_string()),
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
                sdk::WorkflowStepSpec {
                    id: "step-2".to_string(),
                    name: "Change".to_string(),
                    prompt: "Apply the required change based on:\n{{last_output}}".to_string(),
                    agent_id: None,
                    worker_preset: Some("executor".to_string()),
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
                sdk::WorkflowStepSpec {
                    id: "step-3".to_string(),
                    name: "Verify".to_string(),
                    prompt: "Verify the final state using:\n{{last_output}}".to_string(),
                    agent_id: None,
                    worker_preset: Some("verifier".to_string()),
                    thread_key: None,
                    outcome_contract: None,
                    continue_on_error: false,
                    branches: Vec::new(),
                },
            ],
            runtime_profile: Some("headless".to_string()),
            tags: vec!["operations".to_string()],
            ..sdk::WorkflowSpec::default()
        },
        other => anyhow::bail!("Unknown workflow template '{}'", other),
    };
    Ok(spec)
}

fn infer_tools_from_text(text: &str) -> Vec<String> {
    let text = text.to_ascii_lowercase();
    let mut tools = Vec::new();

    if ["file", "read", "open", "inspect", "cat", "show", "view"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        tools.push("read_file".to_string());
    }
    if [
        "write", "save", "create", "put in", "temp.txt", "edit", "update", "append",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        tools.push("write_file".to_string());
    }
    if ["list", "directory", "folder", "tree", "workspace", "repo"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        tools.push("list_dir".to_string());
    }
    if ["exists", "check path", "present", "missing"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        tools.push("file_exists".to_string());
    }
    if [
        "command", "shell", "mkdir", "git", "cargo", "npm", "pnpm", "run ",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        tools.push("run_command".to_string());
    }
    if ["screenshot", "screen", "capture"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        tools.push("capture_screen".to_string());
    }

    dedupe_strings(tools)
}

fn infer_workflow_steps(requirement: &str) -> Vec<sdk::WorkflowStepSpec> {
    let parts = requirement
        .split(['\n'])
        .flat_map(|line| line.split(" then "))
        .flat_map(|line| line.split(" and then "))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.len() <= 1 {
        return Vec::new();
    }

    parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| sdk::WorkflowStepSpec {
            id: format!("step-{}", index + 1),
            name: format!("Step {}", index + 1),
            prompt: if index == 0 {
                format!("{}\n\nContext input:\n{{{{input}}}}", part)
            } else {
                format!("{}\n\nPrior output:\n{{{{last_output}}}}", part)
            },
            agent_id: None,
            worker_preset: Some(infer_worker_preset(part).to_string()),
            thread_key: None,
            outcome_contract: None,
            continue_on_error: false,
            branches: Vec::new(),
        })
        .collect()
}

fn infer_worker_preset(text: &str) -> &'static str {
    let text = text.to_ascii_lowercase();
    if [
        "verify",
        "validation",
        "validate",
        "check",
        "confirm",
        "assert",
        "test",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        return "verifier";
    }
    if [
        "summary",
        "summarize",
        "summarise",
        "recap",
        "synthesize",
        "synthesise",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        return "summariser";
    }
    if [
        "research",
        "survey",
        "inspect",
        "investigate",
        "analyze",
        "analyse",
        "review",
        "read",
        "discover",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        return "researcher";
    }
    "executor"
}

fn worker_preset_for_tool(tool_name: &str) -> &'static str {
    match tool_name {
        "read_file" | "list_dir" | "file_exists" | "capture_screen" => "researcher",
        "write_file" | "run_command" => "executor",
        _ => "executor",
    }
}

fn extract_tool_names(events: &[crate::storage::AgentEvent]) -> Vec<String> {
    dedupe_strings(
        events
            .iter()
            .filter(|event| event.event_type == "tool_call")
            .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event.payload).ok())
            .filter_map(|payload| {
                payload
                    .get("tool_name")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .collect(),
    )
}

fn build_agent_instructions(requirement: &str, base: &str) -> String {
    format!(
        "{base}\n\nPrimary requirement:\n{requirement}\n\nKeep all capabilities and policies explicit. Do not assume tools outside this spec."
    )
}

fn build_from_task_agent_instructions(task_input: &str, answer: Option<&str>) -> String {
    let mut instructions = format!(
        "Handle tasks similar to this successful example:\n{task_input}\n\nUse only the explicit capabilities in this agent."
    );
    if let Some(answer) = answer.filter(|value| !value.trim().is_empty()) {
        instructions.push_str("\n\nReference successful answer pattern:\n");
        instructions.push_str(answer);
    }
    instructions
}

fn summarize_requirement(requirement: &str) -> String {
    let trimmed = requirement.trim();
    let mut summary = trimmed.lines().next().unwrap_or(trimmed).trim().to_string();
    if summary.len() > 120 {
        summary.truncate(117);
        summary.push_str("...");
    }
    summary
}

fn mentions_remote_execution(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "remote",
        "headless",
        "server",
        "daemon mesh",
        "executor",
        "home-mac",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn tool_caps(names: &[&str]) -> Vec<sdk::CapabilityRef> {
    names
        .iter()
        .map(|name| sdk::CapabilityRef {
            kind: "tool".to_string(),
            name: (*name).to_string(),
            required: false,
        })
        .collect()
}

fn merge_tool_capabilities(
    existing: Vec<sdk::CapabilityRef>,
    inferred: Vec<String>,
) -> Vec<sdk::CapabilityRef> {
    let mut caps = existing;
    for tool in inferred {
        if !caps
            .iter()
            .any(|capability| capability.kind == "tool" && capability.name == tool)
        {
            caps.push(sdk::CapabilityRef {
                kind: "tool".to_string(),
                name: tool,
                required: false,
            });
        }
    }
    caps
}

fn dedupe_tags(tags: Vec<String>) -> Vec<String> {
    dedupe_strings(tags)
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.iter().any(|current| current == &value) {
            out.push(value);
        }
    }
    out
}

fn title_case(value: &str) -> String {
    value
        .split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{
        agent_from_task, approve_agent, approve_workflow, create_agent, create_workflow,
        preview_agent, preview_agent_result, preview_workflow, workflow_from_task,
    };
    use crate::specs::SpecRepository;
    use crate::storage::Database;

    #[test]
    fn preview_agent_is_disabled_and_explicit() {
        let spec = preview_agent(
            "make a telegram support agent that can read files and run commands",
            Some("telegram-support"),
            None,
            None,
        )
        .unwrap();
        assert!(!spec.enabled);
        assert!(spec.tags.contains(&"generated".to_string()));
        assert!(spec
            .channels
            .iter()
            .any(|binding| binding.kind == "telegram"
                && binding.target.as_deref() == Some("default")));
    }

    #[test]
    fn preview_workflow_splits_then_chains() {
        let spec = preview_workflow(
            "inspect logs then summarize the failure then propose the fix",
            None,
            None,
            None,
        )
        .unwrap();
        assert!(spec.steps.len() >= 3);
        assert!(!spec.enabled);
    }

    #[test]
    fn preview_result_reports_existing_target_changes() {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();
        repo.save_agent(&sdk::AgentSpec {
            id: "ops-agent".to_string(),
            name: "Ops Agent".to_string(),
            purpose: "Current agent".to_string(),
            instructions: "Current instructions".to_string(),
            enabled: true,
            ..sdk::AgentSpec::default()
        })
        .unwrap();

        let result = preview_agent_result(
            Some(&repo),
            "create an ops agent that can run commands",
            Some("general-assistant"),
            Some("ops-agent"),
            Some("Ops Agent"),
        )
        .unwrap();

        assert!(result.review.target_exists);
        assert_eq!(result.review.suggested_action, "create_draft_update");
        assert!(result
            .review
            .warnings
            .iter()
            .any(|warning| warning.contains("currently enabled")));
        assert!(result
            .review
            .changes
            .iter()
            .any(|change| change.field == "instructions"));
    }

    #[test]
    fn create_and_approve_agent_promotes_draft() {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let created = create_agent(
            &repo,
            "create a support agent that can read files",
            Some("general-assistant"),
            Some("support-agent"),
            Some("Support Agent"),
        )
        .unwrap();

        assert_eq!(created.review.review_status, "draft");
        assert_eq!(created.review.target_id, "support-agent");
        assert!(created.spec.id.starts_with("support-agent-draft"));
        assert_eq!(
            created
                .spec
                .provenance
                .as_ref()
                .and_then(|item| item.draft_for.as_deref()),
            Some("support-agent")
        );

        let approved = approve_agent(&repo, &created.spec.id).unwrap();
        assert_eq!(approved.id, "support-agent");
        assert_eq!(
            approved
                .provenance
                .as_ref()
                .and_then(|item| item.review_status.as_deref()),
            Some("approved")
        );
        assert!(repo.load_agent(&created.spec.id).is_err());
    }

    #[test]
    fn create_and_approve_workflow_promotes_draft() {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let created = create_workflow(
            &repo,
            "inspect the target then verify the state",
            Some("remote-ops"),
            Some("ops-flow"),
            Some("Ops Flow"),
        )
        .unwrap();

        assert_eq!(created.review.review_status, "draft");
        assert_eq!(created.review.target_id, "ops-flow");
        assert!(created.spec.id.starts_with("ops-flow-draft"));

        let approved = approve_workflow(&repo, &created.spec.id).unwrap();
        assert_eq!(approved.id, "ops-flow");
        assert_eq!(
            approved
                .provenance
                .as_ref()
                .and_then(|item| item.review_status.as_deref()),
            Some("approved")
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn from_task_generates_specs() {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let db = Database::new(&temp_dir.path().join("factory.db"))
            .await
            .unwrap();
        let task_repo = db.tasks();
        let task_id = uuid::Uuid::new_v4();
        task_repo
            .create_task(&task_id, "write temp.txt")
            .await
            .unwrap();
        task_repo
            .update_task_status(&task_id, crate::storage::TaskStatus::Running)
            .await
            .unwrap();
        task_repo
            .insert_agent_event(
                &task_id,
                "tool_call",
                r#"{"tool_name":"write_file","tool_args":"{\"path\":\"temp.txt\"}","tool_id":"1"}"#,
                1,
                Some("general"),
            )
            .await
            .unwrap();
        task_repo
            .insert_agent_event(
                &task_id,
                "answer",
                r#"{"answer":"done"}"#,
                2,
                Some("general"),
            )
            .await
            .unwrap();
        task_repo.complete_task(&task_id, "mock", 10).await.unwrap();

        let repo = crate::specs::SpecRepository::new().unwrap();
        let agent = agent_from_task(&repo, &db, &task_id.to_string(), None, None)
            .await
            .unwrap();
        let workflow = workflow_from_task(&repo, &db, &task_id.to_string(), None, None)
            .await
            .unwrap();

        assert!(!agent.spec.enabled);
        assert!(agent.spec.tags.iter().any(|tag| tag == "from-task"));
        assert!(agent
            .spec
            .capabilities
            .iter()
            .any(|cap| cap.name == "write_file"));
        assert_eq!(agent.review.review_status, "draft");
        assert!(!workflow.spec.enabled);
        assert!(workflow.spec.tags.iter().any(|tag| tag == "from-task"));
        assert_eq!(workflow.review.review_status, "draft");
    }
}
