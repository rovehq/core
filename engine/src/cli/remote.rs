use anyhow::Result;

use crate::config::Config;
use crate::remote::RemoteManager;

pub enum RemoteAction {
    Status,
    Nodes,
    Rename(String),
    Pair(String),
    Unpair(String),
    Trust(String),
    Send { node: String, prompt: String },
}

pub fn handle(action: RemoteAction, config: &Config) -> Result<()> {
    let manager = RemoteManager::new(config.clone());
    match action {
        RemoteAction::Status => status(&manager),
        RemoteAction::Nodes => nodes(&manager),
        RemoteAction::Rename(name) => rename(&manager, &name),
        RemoteAction::Pair(target) => pair(&manager, &target),
        RemoteAction::Unpair(name) => unpair(&manager, &name),
        RemoteAction::Trust(name) => trust(&manager, &name),
        RemoteAction::Send { node, prompt } => send(&manager, &node, &prompt),
    }
}

fn status(manager: &RemoteManager) -> Result<()> {
    let status = manager.status()?;
    println!("remote: {}", if status.enabled { "enabled" } else { "disabled" });
    println!("node_name: {}", status.node.node_name);
    println!("node_id: {}", status.node.node_id);
    println!("execution_role: {:?}", status.profile.execution_role);
    println!("paired_nodes: {}", status.paired_nodes);
    Ok(())
}

fn nodes(manager: &RemoteManager) -> Result<()> {
    let peers = manager.nodes()?;
    if peers.is_empty() {
        println!("No paired nodes.");
        return Ok(());
    }
    println!("Paired nodes:");
    for peer in peers {
        println!(
            "- {} [{}] {}",
            peer.identity.node_name,
            if peer.trusted { "trusted" } else { "paired" },
            peer.target
        );
    }
    Ok(())
}

fn rename(manager: &RemoteManager, name: &str) -> Result<()> {
    manager.rename(name)?;
    println!("Local node renamed to '{}'.", name);
    Ok(())
}

fn pair(manager: &RemoteManager, target: &str) -> Result<()> {
    manager.pair(target)?;
    println!("Paired remote node '{}'.", target);
    Ok(())
}

fn unpair(manager: &RemoteManager, name: &str) -> Result<()> {
    manager.unpair(name)?;
    println!("Removed remote node '{}'.", name);
    Ok(())
}

fn trust(manager: &RemoteManager, name: &str) -> Result<()> {
    manager.trust(name)?;
    println!("Trusted remote node '{}'.", name);
    Ok(())
}

fn send(manager: &RemoteManager, node: &str, prompt: &str) -> Result<()> {
    let preview = manager.send_preview(node, prompt)?;
    println!(
        "Remote send preview: coordinator='{}' target='{}' prompt='{}'",
        preview.envelope.coordinator_node, preview.envelope.target_node, prompt
    );
    println!("{}", preview.message);
    Ok(())
}
