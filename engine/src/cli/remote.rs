use anyhow::Result;

use crate::config::Config;
use crate::remote::RemoteManager;

pub enum RemoteAction {
    Status,
    Nodes,
    Rename(String),
    Pair {
        target: String,
        url: Option<String>,
        token: Option<String>,
        executor_only: bool,
        tags: Vec<String>,
    },
    Unpair(String),
    Trust(String),
    Send { node: String, prompt: String },
}

pub async fn handle(action: RemoteAction, config: &Config) -> Result<()> {
    let manager = RemoteManager::new(config.clone());
    match action {
        RemoteAction::Status => status(&manager),
        RemoteAction::Nodes => nodes(&manager),
        RemoteAction::Rename(name) => rename(&manager, &name),
        RemoteAction::Pair {
            target,
            url,
            token,
            executor_only,
            tags,
        } => pair(&manager, &target, url.as_deref(), token.as_deref(), executor_only, &tags).await,
        RemoteAction::Unpair(name) => unpair(&manager, &name).await,
        RemoteAction::Trust(name) => trust(&manager, &name),
        RemoteAction::Send { node, prompt } => send(&manager, &node, &prompt).await,
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
            "- {} [{}] {} role={:?}",
            peer.identity.node_name,
            if peer.trusted { "trusted" } else { "paired" },
            peer.target,
            peer.profile.execution_role
        );
    }
    Ok(())
}

fn rename(manager: &RemoteManager, name: &str) -> Result<()> {
    manager.rename(name)?;
    println!("Local node renamed to '{}'.", name);
    Ok(())
}

async fn pair(
    manager: &RemoteManager,
    target: &str,
    url: Option<&str>,
    token: Option<&str>,
    executor_only: bool,
    tags: &[String],
) -> Result<()> {
    let peer = manager
        .pair(target, url, token, executor_only, tags)
        .await?;
    println!(
        "Paired remote node '{}' at {}.",
        peer.identity.node_name, peer.target
    );
    if peer.auth_secret_key.is_some() {
        println!("Stored bearer token for '{}'.", peer.identity.node_name);
    } else {
        println!(
            "No bearer token stored for '{}'. Remote sends will use the configured remote auth token if available.",
            peer.identity.node_name
        );
    }
    Ok(())
}

async fn unpair(manager: &RemoteManager, name: &str) -> Result<()> {
    manager.unpair(name).await?;
    println!("Removed remote node '{}'.", name);
    Ok(())
}

fn trust(manager: &RemoteManager, name: &str) -> Result<()> {
    manager.trust(name)?;
    println!("Trusted remote node '{}'.", name);
    Ok(())
}

async fn send(manager: &RemoteManager, node: &str, prompt: &str) -> Result<()> {
    let result = manager.send(node, prompt).await?;
    println!(
        "Remote send: coordinator='{}' target='{}' remote_task_id='{}' status='{}'",
        result.envelope.coordinator_node,
        result.envelope.target_node,
        result.remote_task_id,
        result.status
    );
    if !result.events.is_empty() {
        println!("Streamed {} remote event(s):", result.events.len());
        for event in &result.events {
            println!(
                "- [{}] {} {}",
                event.step_num, event.event_type, event.payload
            );
        }
    }
    if let Some(answer) = result.answer {
        println!();
        println!("{}", answer);
    } else if let Some(message) = result.message {
        println!("{}", message);
    }
    Ok(())
}
