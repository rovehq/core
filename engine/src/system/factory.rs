use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

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
            description: "Balanced local agent for common filesystem and command tasks.".to_string(),
        },
        TemplateSummary {
            id: "research-runner".to_string(),
            name: "Research Runner".to_string(),
            description: "Read-focused agent for investigation, summaries, and verification.".to_string(),
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
        notes: Some(format!("Generated from requirement using template {}", template)),
        imported_at: Some(Utc::now().timestamp()),
    });

    let requirement_lower = requirement.to_ascii_lowercase();
    if requirement_lower.contains("telegram")
        && !spec
            .channels
            .iter()
            .any(|binding| binding.kind == "telegram" && binding.target.as_deref() == Some("default"))
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

pub fn create_agent(
    repo: &SpecRepository,
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::AgentSpec> {
    let spec = preview_agent(requirement, template_id, id, name)?;
    repo.save_agent(&spec)
}

pub async fn agent_from_task(
    repo: &SpecRepository,
    database: &Database,
    task_id: &str,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::AgentSpec> {
    let task_uuid = uuid::Uuid::parse_str(task_id).context("Invalid task id")?;
    let task_repo = TaskRepository::new(database.pool().clone());
    let task = task_repo
        .get_task(&task_uuid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' was not found", task_id))?;
    let events = task_repo.get_agent_events(task_id).await.unwrap_or_default();
    let answer = task_repo.get_latest_answer(task_id).await.unwrap_or(None);

    let inferred_tools = extract_tool_names(&events);
    let mut spec = preview_agent(&task.input, Some("general-assistant"), id, name)?;
    if !inferred_tools.is_empty() {
        spec.capabilities = merge_tool_capabilities(
            Vec::new(),
            inferred_tools.into_iter().collect(),
        );
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
    });
    repo.save_agent(&spec)
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
        notes: Some(format!("Generated from requirement using template {}", template)),
        imported_at: Some(Utc::now().timestamp()),
    });
    if mentions_remote_execution(requirement) {
        spec.runtime_profile = Some("headless".to_string());
    }

    Ok(spec)
}

pub fn create_workflow(
    repo: &SpecRepository,
    requirement: &str,
    template_id: Option<&str>,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::WorkflowSpec> {
    let spec = preview_workflow(requirement, template_id, id, name)?;
    repo.save_workflow(&spec)
}

pub async fn workflow_from_task(
    repo: &SpecRepository,
    database: &Database,
    task_id: &str,
    id: Option<&str>,
    name: Option<&str>,
) -> Result<sdk::WorkflowSpec> {
    let task_uuid = uuid::Uuid::parse_str(task_id).context("Invalid task id")?;
    let task_repo = TaskRepository::new(database.pool().clone());
    let task = task_repo
        .get_task(&task_uuid)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' was not found", task_id))?;
    let events = task_repo.get_agent_events(task_id).await.unwrap_or_default();
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
                continue_on_error: false,
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
            continue_on_error: false,
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
        tags: vec!["generated".to_string(), "from-task".to_string(), format!("task:{task_id}")],
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
    });
    repo.save_workflow(&spec)
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
                continue_on_error: false,
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
                    continue_on_error: false,
                },
                sdk::WorkflowStepSpec {
                    id: "step-2".to_string(),
                    name: "Analyze".to_string(),
                    prompt: "Analyze the survey findings:\n{{last_output}}".to_string(),
                    agent_id: None,
                    continue_on_error: false,
                },
                sdk::WorkflowStepSpec {
                    id: "step-3".to_string(),
                    name: "Summarize".to_string(),
                    prompt: "Produce the final summary from:\n{{last_output}}".to_string(),
                    agent_id: None,
                    continue_on_error: false,
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
                    continue_on_error: false,
                },
                sdk::WorkflowStepSpec {
                    id: "step-2".to_string(),
                    name: "Change".to_string(),
                    prompt: "Apply the required change based on:\n{{last_output}}".to_string(),
                    agent_id: None,
                    continue_on_error: false,
                },
                sdk::WorkflowStepSpec {
                    id: "step-3".to_string(),
                    name: "Verify".to_string(),
                    prompt: "Verify the final state using:\n{{last_output}}".to_string(),
                    agent_id: None,
                    continue_on_error: false,
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
    if ["write", "save", "create", "put in", "temp.txt", "edit", "update", "append"]
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
    if ["command", "shell", "mkdir", "git", "cargo", "npm", "pnpm", "run "]
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
            continue_on_error: false,
        })
        .collect()
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
    ["remote", "headless", "server", "daemon mesh", "executor", "home-mac"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn tool_caps(names: &[&str]) -> Vec<sdk::CapabilityRef> {
    names.iter()
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
        if !caps.iter().any(|capability| capability.kind == "tool" && capability.name == tool) {
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
    value.split(['-', '_', ' '])
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

    use super::{agent_from_task, preview_agent, preview_workflow, workflow_from_task};
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
            .any(|binding| binding.kind == "telegram" && binding.target.as_deref() == Some("default")));
    }

    #[test]
    fn preview_workflow_splits_then_chains() {
        let spec = preview_workflow("inspect logs then summarize the failure then propose the fix", None, None, None).unwrap();
        assert!(spec.steps.len() >= 3);
        assert!(!spec.enabled);
    }

    #[tokio::test]
    async fn from_task_generates_specs() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let db = Database::new(&temp_dir.path().join("factory.db")).await.unwrap();
        let task_repo = db.tasks();
        let task_id = uuid::Uuid::new_v4();
        task_repo.create_task(&task_id, "write temp.txt").await.unwrap();
        task_repo.update_task_status(&task_id, crate::storage::TaskStatus::Running).await.unwrap();
        task_repo.insert_agent_event(
            &task_id,
            "tool_call",
            r#"{"tool_name":"write_file","tool_args":"{\"path\":\"temp.txt\"}","tool_id":"1"}"#,
            1,
            Some("general"),
        ).await.unwrap();
        task_repo.insert_agent_event(
            &task_id,
            "answer",
            r#"{"answer":"done"}"#,
            2,
            Some("general"),
        ).await.unwrap();
        task_repo.complete_task(&task_id, "mock", 10).await.unwrap();

        let repo = crate::specs::SpecRepository::new().unwrap();
        let agent = agent_from_task(&repo, &db, &task_id.to_string(), None, None).await.unwrap();
        let workflow = workflow_from_task(&repo, &db, &task_id.to_string(), None, None).await.unwrap();

        assert!(!agent.enabled);
        assert!(agent.tags.iter().any(|tag| tag == "from-task"));
        assert!(agent.capabilities.iter().any(|cap| cap.name == "write_file"));
        assert!(!workflow.enabled);
        assert!(workflow.tags.iter().any(|tag| tag == "from-task"));
    }
}
