use anyhow::Result;
use sdk::{StarterCatalogEntry, StarterCatalogKind, StarterCatalogStatus};

use crate::channels::manager::ChannelManager;
use crate::config::Config;

use super::{factory, worker_presets};

pub async fn list(config: &Config) -> Result<Vec<StarterCatalogEntry>> {
    let mut entries = Vec::new();
    entries.extend(agent_template_entries());
    entries.extend(workflow_template_entries());
    entries.extend(worker_preset_entries());
    entries.push(telegram_channel_entry(config).await?);
    entries.extend(capability_pack_entries());

    entries.sort_by(|left, right| {
        starter_kind_rank(left.kind)
            .cmp(&starter_kind_rank(right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(entries)
}

fn agent_template_entries() -> Vec<StarterCatalogEntry> {
    factory::list_agent_templates()
        .into_iter()
        .map(|template| {
            let id = template.id;
            let name = template.name;
            let description = template.description;
            StarterCatalogEntry {
                id: format!("agent-template:{id}"),
                kind: StarterCatalogKind::AgentTemplate,
                name,
                description,
                official: true,
                status: StarterCatalogStatus::Available,
                source: format!("builtin_agent_template:{id}"),
                action_label: "Open agent factory".to_string(),
                action_route: Some("/agents".to_string()),
                command_hint: Some(format!(
                    "rove agent factory preview --template {} \"<requirement>\"",
                    id
                )),
                tags: vec![
                    "official".to_string(),
                    "starter".to_string(),
                    "agent".to_string(),
                ],
                components: vec![format!("template:{id}")],
                notes: vec![
                    "Creates a disabled AgentSpec draft that stays reviewable before approval."
                        .to_string(),
                ],
            }
        })
        .collect()
}

fn workflow_template_entries() -> Vec<StarterCatalogEntry> {
    factory::list_workflow_templates()
        .into_iter()
        .map(|template| {
            let id = template.id;
            let name = template.name;
            let description = template.description;
            StarterCatalogEntry {
                id: format!("workflow-template:{id}"),
                kind: StarterCatalogKind::WorkflowTemplate,
                name,
                description,
                official: true,
                status: StarterCatalogStatus::Available,
                source: format!("builtin_workflow_template:{id}"),
                action_label: "Open workflow factory".to_string(),
                action_route: Some("/workflows".to_string()),
                command_hint: Some(format!(
                    "rove workflow factory preview --template {} \"<requirement>\"",
                    id
                )),
                tags: vec![
                    "official".to_string(),
                    "starter".to_string(),
                    "workflow".to_string(),
                ],
                components: vec![format!("template:{id}")],
                notes: vec![
                "Creates a disabled WorkflowSpec draft with explicit steps and review metadata."
                    .to_string(),
            ],
            }
        })
        .collect()
}

fn worker_preset_entries() -> Vec<StarterCatalogEntry> {
    worker_presets::list_worker_presets()
        .into_iter()
        .map(|preset| {
            let id = preset.id;
            let name = preset.name;
            let description = preset.description;
            let role = preset.role;
            let allowed_tools = preset.allowed_tools;
            let max_steps = preset.max_steps;
            let timeout_secs = preset.timeout_secs;
            StarterCatalogEntry {
                id: format!("worker-preset:{id}"),
                kind: StarterCatalogKind::WorkerPreset,
                name,
                description,
                official: true,
                status: StarterCatalogStatus::Available,
                source: format!("builtin_worker_preset:{id}"),
                action_label: "Use in workflows".to_string(),
                action_route: Some("/workflows".to_string()),
                command_hint: Some("rove workflow worker-presets".to_string()),
                tags: vec![
                    "official".to_string(),
                    "bounded".to_string(),
                    "worker".to_string(),
                    role,
                ],
                components: allowed_tools,
                notes: vec![format!(
                    "Bounded to {} steps and {} seconds.",
                    max_steps, timeout_secs
                )],
            }
        })
        .collect()
}

async fn telegram_channel_entry(config: &Config) -> Result<StarterCatalogEntry> {
    let status = ChannelManager::new(config.clone())
        .telegram_status()
        .await?;
    let starter_status = if status.can_receive {
        StarterCatalogStatus::Ready
    } else if status.token_configured || status.default_agent_id.is_some() || status.enabled {
        StarterCatalogStatus::NeedsSetup
    } else {
        StarterCatalogStatus::Available
    };

    let command_hint = if status.can_receive {
        "rove channel telegram status".to_string()
    } else {
        "rove channel telegram setup --token <bot-token> --agent <agent-id> --allow-user <telegram-user-id>"
            .to_string()
    };

    Ok(StarterCatalogEntry {
        id: "channel-starter:telegram".to_string(),
        kind: StarterCatalogKind::ChannelStarter,
        name: "Telegram Channel Pack".to_string(),
        description:
            "First-class inbound Telegram runtime with allow-list controls, approvals, and a default agent binding."
                .to_string(),
        official: true,
        status: starter_status,
        source: "builtin_channel:telegram".to_string(),
        action_label: if status.can_receive {
            "Open channels".to_string()
        } else {
            "Finish Telegram setup".to_string()
        },
        action_route: Some("/channels".to_string()),
        command_hint: Some(command_hint),
        tags: vec![
            "official".to_string(),
            "channel".to_string(),
            "telegram".to_string(),
        ],
        components: vec![
            "telegram bot token".to_string(),
            "default handler agent".to_string(),
            "allow list".to_string(),
        ],
        notes: status.doctor,
    })
}

fn capability_pack_entries() -> Vec<StarterCatalogEntry> {
    vec![
        capability_pack_entry(
            "capability-pack:workspace-files",
            "Workspace Files Pack",
            "Workspace-scoped filesystem connector profile for local inspection, editing, and document-aware automation.",
            "builtin_connector_template:workspace-files",
            "rove connector add workspace-files --template workspace-files",
            vec![
                "workspace-files template",
                "workspace read access",
                "workspace write access",
            ],
            vec![
                "Uses the official workspace-scoped MCP template instead of an ad hoc unrestricted connector."
                    .to_string(),
            ],
            vec!["connector", "filesystem", "local"],
        ),
        capability_pack_entry(
            "capability-pack:github",
            "GitHub Connector Pack",
            "Networked GitHub connector starter for repository, issue, and pull-request workflows.",
            "builtin_connector_template:github",
            "rove connector add github --template github",
            vec!["github template", "github_token secret", "network access"],
            vec![
                "Trusted one-click install is a separate follow-up. Today this seeds the official connector profile."
                    .to_string(),
            ],
            vec!["connector", "github", "networked"],
        ),
        capability_pack_entry(
            "capability-pack:notion",
            "Notion Connector Pack",
            "Official Notion starter profile for workspace knowledge and document retrieval.",
            "builtin_connector_template:notion",
            "rove connector add notion --template notion",
            vec!["notion template", "notion_token secret", "network access"],
            vec![
                "Pairs naturally with research workflows once knowledge ingest UX is built out."
                    .to_string(),
            ],
            vec!["connector", "knowledge", "notion"],
        ),
        capability_pack_entry(
            "capability-pack:slack",
            "Slack Connector Pack",
            "Official Slack starter profile for operational notification and messaging integrations.",
            "builtin_connector_template:slack",
            "rove connector add slack --template slack",
            vec!["slack template", "slack_bot_token secret", "network access"],
            vec![
                "Connector support can exist before Slack becomes a first-class inbound runtime channel."
                    .to_string(),
            ],
            vec!["connector", "slack", "notifications"],
        ),
    ]
}

fn capability_pack_entry(
    id: &str,
    name: &str,
    description: &str,
    source: &str,
    command_hint: &str,
    components: Vec<&str>,
    notes: Vec<String>,
    tags: Vec<&str>,
) -> StarterCatalogEntry {
    StarterCatalogEntry {
        id: id.to_string(),
        kind: StarterCatalogKind::CapabilityPack,
        name: name.to_string(),
        description: description.to_string(),
        official: true,
        status: StarterCatalogStatus::Available,
        source: source.to_string(),
        action_label: "Use CLI setup".to_string(),
        action_route: None,
        command_hint: Some(command_hint.to_string()),
        tags: tags.into_iter().map(ToOwned::to_owned).collect(),
        components: components.into_iter().map(ToOwned::to_owned).collect(),
        notes,
    }
}

fn starter_kind_rank(kind: StarterCatalogKind) -> u8 {
    match kind {
        StarterCatalogKind::AgentTemplate => 0,
        StarterCatalogKind::WorkflowTemplate => 1,
        StarterCatalogKind::WorkerPreset => 2,
        StarterCatalogKind::ChannelStarter => 3,
        StarterCatalogKind::CapabilityPack => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::capability_pack_entries;

    #[test]
    fn curated_capability_packs_are_present() {
        let entries = capability_pack_entries();
        assert!(entries
            .iter()
            .any(|entry| entry.id == "capability-pack:github"));
        assert!(entries
            .iter()
            .any(|entry| entry.id == "capability-pack:workspace-files"));
    }
}
