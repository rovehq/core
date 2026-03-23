use anyhow::Result;

use crate::config::Config;
use crate::remote::{RemoteManager, RemoteSendOptions};
use crate::zerotier::ZeroTierManager;

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
    ZeroTierInstall,
    ZeroTierUninstall,
    ZeroTierStatus,
    ZeroTierSetup {
        network_id: String,
        token_key: Option<String>,
        managed_name_sync: bool,
    },
    ZeroTierJoin {
        network_id: Option<String>,
    },
    ZeroTierRefresh,
    DiscoverList,
    DiscoverRefresh,
    DiscoverTrust(String),
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
        RemoteAction::ZeroTierInstall => zerotier_install(config).await,
        RemoteAction::ZeroTierUninstall => zerotier_uninstall(config).await,
        RemoteAction::ZeroTierStatus => zerotier_status(config).await,
        RemoteAction::ZeroTierSetup {
            network_id,
            token_key,
            managed_name_sync,
        } => zerotier_setup(config, &network_id, token_key.as_deref(), managed_name_sync).await,
        RemoteAction::ZeroTierJoin { network_id } => {
            zerotier_join(config, network_id.as_deref()).await
        }
        RemoteAction::ZeroTierRefresh => zerotier_refresh(config).await,
        RemoteAction::DiscoverList => discover_list(config).await,
        RemoteAction::DiscoverRefresh => discover_refresh(config).await,
        RemoteAction::DiscoverTrust(candidate_id) => discover_trust(config, &candidate_id).await,
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
    println!(
        "remote: {}",
        if status.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("node_name: {}", status.node.node_name);
    println!("node_id: {}", status.node.node_id);
    println!("execution_role: {:?}", status.profile.execution_role);
    println!("paired_nodes: {}", status.paired_nodes);
    Ok(())
}

async fn zerotier_install(config: &Config) -> Result<()> {
    let status = ZeroTierManager::new(config.clone()).install().await?;
    print_zerotier_status(&status);
    Ok(())
}

async fn zerotier_uninstall(config: &Config) -> Result<()> {
    let status = ZeroTierManager::new(config.clone()).uninstall().await?;
    print_zerotier_status(&status);
    Ok(())
}

async fn zerotier_status(config: &Config) -> Result<()> {
    let status = ZeroTierManager::new(config.clone()).status().await?;
    print_zerotier_status(&status);
    Ok(())
}

async fn zerotier_setup(
    config: &Config,
    network_id: &str,
    token_key: Option<&str>,
    managed_name_sync: bool,
) -> Result<()> {
    let status = ZeroTierManager::new(config.clone())
        .setup(network_id, token_key, managed_name_sync)
        .await?;
    print_zerotier_status(&status);
    Ok(())
}

async fn zerotier_join(config: &Config, network_id: Option<&str>) -> Result<()> {
    let status = ZeroTierManager::new(config.clone())
        .join(network_id)
        .await?;
    print_zerotier_status(&status);
    Ok(())
}

async fn zerotier_refresh(config: &Config) -> Result<()> {
    let status = ZeroTierManager::new(config.clone()).refresh().await?;
    print_zerotier_status(&status);
    Ok(())
}

async fn discover_list(config: &Config) -> Result<()> {
    let candidates = ZeroTierManager::new(config.clone())
        .list_candidates()
        .await?;
    if candidates.is_empty() {
        println!("No ZeroTier discovery candidates found.");
        return Ok(());
    }
    println!("Discoverable ZeroTier nodes:");
    for candidate in candidates {
        let label = candidate
            .node_name_hint
            .as_deref()
            .or(candidate.member_name.as_deref())
            .unwrap_or(candidate.member_id.as_str());
        println!(
            "- {} [{}] trusted={} addresses={} transports={}",
            label,
            candidate.candidate_id,
            candidate.trusted,
            if candidate.assigned_addresses.is_empty() {
                "none".to_string()
            } else {
                candidate.assigned_addresses.join(", ")
            },
            candidate
                .transports
                .iter()
                .map(|record| record
                    .base_url
                    .clone()
                    .unwrap_or_else(|| record.address.clone()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

async fn discover_refresh(config: &Config) -> Result<()> {
    let status = ZeroTierManager::new(config.clone()).refresh().await?;
    print_zerotier_status(&status);
    Ok(())
}

async fn discover_trust(config: &Config, candidate_id: &str) -> Result<()> {
    let candidate = ZeroTierManager::new(config.clone())
        .trust_candidate(candidate_id)
        .await?;
    println!(
        "Trusted ZeroTier candidate '{}' as node '{}'.",
        candidate.candidate_id,
        candidate
            .paired_node_name
            .or(candidate.node_name_hint)
            .unwrap_or(candidate.member_id)
    );
    Ok(())
}

fn print_zerotier_status(status: &sdk::ZeroTierStatus) {
    println!(
        "zerotier: enabled={} installed={} service_online={} joined={} controller_access={}",
        status.enabled,
        status.installed,
        status.service_online,
        status.joined,
        status.controller_access
    );
    println!(
        "network_id: {}",
        status.network_id.as_deref().unwrap_or("(none)")
    );
    println!("sync_state: {}", status.sync_state);
    println!("candidate_count: {}", status.candidate_count);
    if !status.assigned_addresses.is_empty() {
        println!(
            "assigned_addresses: {}",
            status.assigned_addresses.join(", ")
        );
    }
    if !status.transport_records.is_empty() {
        println!(
            "transports: {}",
            status
                .transport_records
                .iter()
                .map(|record| record
                    .base_url
                    .clone()
                    .unwrap_or_else(|| record.address.clone()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if let Some(message) = &status.message {
        println!("message: {}", message);
    }
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
    println!(
        "Local node execution role set to {:?}.",
        profile.execution_role
    );
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
        println!(
            "Local node capabilities: {}",
            profile.capabilities.join(", ")
        );
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
    let mut result = manager
        .send_with_options(
            prompt,
            RemoteSendOptions {
                node: Some(node.to_string()),
                required_tags: tags.to_vec(),
                required_capabilities: capabilities.to_vec(),
                allow_executor_only,
                prefer_executor_only,
                execution_plan: None,
            },
        )
        .await;
    if result.is_err() && manager.status()?.enabled {
        let _ = ZeroTierManager::new(Config::load_or_create()?)
            .refresh()
            .await;
        result = manager
            .send_with_options(
                prompt,
                RemoteSendOptions {
                    node: Some(node.to_string()),
                    required_tags: tags.to_vec(),
                    required_capabilities: capabilities.to_vec(),
                    allow_executor_only,
                    prefer_executor_only,
                    execution_plan: None,
                },
            )
            .await;
    }
    let result = result?;
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
