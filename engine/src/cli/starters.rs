use anyhow::Result;
use sdk::{StarterCatalogEntry, StarterCatalogKind};

use crate::config::Config;
use crate::system::starter_catalog;

use super::commands::StarterAction;

pub async fn handle_starters(action: StarterAction, config: &Config) -> Result<()> {
    match action {
        StarterAction::List => list_starters(config).await,
    }
}

async fn list_starters(config: &Config) -> Result<()> {
    let starters = starter_catalog::list(config).await?;
    if starters.is_empty() {
        println!("No official starters are currently available.");
        return Ok(());
    }

    let mut current_kind = None;
    for entry in starters {
        if current_kind != Some(entry.kind) {
            if current_kind.is_some() {
                println!();
            }
            println!("{}:", kind_heading(entry.kind));
            current_kind = Some(entry.kind);
        }

        println!(
            "- {} [{}] {}",
            entry.name,
            entry.status.as_str(),
            entry.description
        );
        if !entry.components.is_empty() {
            println!("  components: {}", entry.components.join(", "));
        }
        if let Some(route) = &entry.action_route {
            println!("  action: {} ({})", entry.action_label, route);
        } else {
            println!("  action: {}", entry.action_label);
        }
        if let Some(command) = &entry.command_hint {
            println!("  command: {}", command);
        }
        if let Some(note) = primary_note(&entry) {
            println!("  note: {}", note);
        }
    }

    Ok(())
}

fn kind_heading(kind: StarterCatalogKind) -> &'static str {
    match kind {
        StarterCatalogKind::AgentTemplate => "Agent templates",
        StarterCatalogKind::WorkflowTemplate => "Workflow templates",
        StarterCatalogKind::WorkerPreset => "Worker presets",
        StarterCatalogKind::ChannelStarter => "Channel starters",
        StarterCatalogKind::CapabilityPack => "Capability packs",
    }
}

fn primary_note(entry: &StarterCatalogEntry) -> Option<&str> {
    entry.notes.first().map(String::as_str)
}
