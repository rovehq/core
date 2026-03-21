use anyhow::Result;

use crate::config::Config;
use crate::remote::{RemoteManager, RemoteSendOptions};

pub enum RemoteAction {
    Status,
    Nodes,
    Rename(String),
    ProfileShow,
    ProfileExecutorOnly,
    ProfileFull,
    ProfileTags(Vec<String>),
    ProfileCapabilities(Vec<String>),
    Pair {
        target: String,
        url: Option<String>,
        token: Option<String>,
        executor_only: bool,
        tags: Vec<String>,
        capabilities: Vec<String>,
    },
    Unpair(String),
    Trust(String),
    Send {
        node: String,
        prompt: String,
        tags: Vec<String>,
        capabilities: Vec<String>,
        allow_executor_only: bool,
        prefer_executor_only: bool,
    },
}

pub async fn handle(action: RemoteAction, config: &Config) -> Result<()> {
    let manager = RemoteManager::new(config.clone());
    match action {
        RemoteAction::Status => status(&manager),
        RemoteAction::Nodes => nodes(&manager),
        RemoteAction::Rename(name) => rename(&manager, &name),
        RemoteAction::ProfileShow => profile_show(&manager),
        RemoteAction::ProfileExecutorOnly => {
            profile_set_role(&manager, sdk::NodeExecutionRole::ExecutorOnly)
        }
        RemoteAction::ProfileFull => profile_set_role(&manager, sdk::NodeExecutionRole::Full),
        RemoteAction::ProfileTags(tags) => profile_tags(&manager, &tags),
        RemoteAction::ProfileCapabilities(capabilities) => {
            profile_capabilities(&manager, &capabilities)
        }
        RemoteAction::Pair {
            target,
            url,
            token,
            executor_only,
            tags,
            capabilities,
        } => {
            pair(
                &manager,
                &target,
                url.as_deref(),
                token.as_deref(),
                executor_only,
                &tags,
                &capabilities,
            )
            .await
        }
        RemoteAction::Unpair(name) => unpair(&manager, &name).await,
        RemoteAction::Trust(name) => trust(&manager, &name),
        RemoteAction::Send {
            node,
            prompt,
            tags,
            capabilities,
            allow_executor_only,
            prefer_executor_only,
        } => {
            send(
                &manager,
                &node,
                &prompt,
                &tags,
                &capabilities,
                allow_executor_only,
                prefer_executor_only,
            )
            .await
        }
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

fn profile_show(manager: &RemoteManager) -> Result<()> {
    let profile = manager.local_profile()?;
    println!("execution_role: {:?}", profile.execution_role);
    if profile.tags.is_empty() {
        println!("tags: (none)");
    } else {
        println!("tags: {}", profile.tags.join(", "));
    }
    if profile.capabilities.is_empty() {
        println!("capabilities: (none)");
    } else {
        println!("capabilities: {}", profile.capabilities.join(", "));
    }
    Ok(())
}

fn profile_set_role(manager: &RemoteManager, role: sdk::NodeExecutionRole) -> Result<()> {
    let profile = manager.set_execution_role(role)?;
    println!("Local node execution role set to {:?}.", profile.execution_role);
    Ok(())
}

fn profile_tags(manager: &RemoteManager, tags: &[String]) -> Result<()> {
    let profile = manager.replace_tags(tags)?;
    if profile.tags.is_empty() {
        println!("Cleared local node tags.");
    } else {
        println!("Local node tags: {}", profile.tags.join(", "));
    }
    Ok(())
}

fn profile_capabilities(manager: &RemoteManager, capabilities: &[String]) -> Result<()> {
    let profile = manager.replace_capabilities(capabilities)?;
    if profile.capabilities.is_empty() {
        println!("Cleared local node capabilities.");
    } else {
        println!("Local node capabilities: {}", profile.capabilities.join(", "));
    }
    Ok(())
}

async fn pair(
    manager: &RemoteManager,
    target: &str,
    url: Option<&str>,
    token: Option<&str>,
    executor_only: bool,
    tags: &[String],
    capabilities: &[String],
) -> Result<()> {
    let peer = manager
        .pair(target, url, token, executor_only, tags, capabilities)
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

async fn send(
    manager: &RemoteManager,
    node: &str,
    prompt: &str,
    tags: &[String],
    capabilities: &[String],
    allow_executor_only: bool,
    prefer_executor_only: bool,
) -> Result<()> {
    let result = manager
        .send_with_options(
            prompt,
            RemoteSendOptions {
                node: Some(node.to_string()),
                required_tags: tags.to_vec(),
                required_capabilities: capabilities.to_vec(),
                allow_executor_only,
                prefer_executor_only,
            },
        )
        .await?;
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
