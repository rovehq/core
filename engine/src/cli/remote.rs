use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result};

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::remote::{RemoteDriverSyncItem, RemoteManager, RemoteSendOptions};
use crate::storage::Database;
use crate::zerotier::ZeroTierManager;

pub enum RemoteAction {
    Status,
    Init {
        network_id: Option<String>,
        token_key: Option<String>,
    },
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
    SyncDrivers {
        node: Option<String>,
        dry_run: bool,
    },
    /// Phase 2: show all live peers with presence scores.
    PresenceList,
    /// Phase 3: open an interactive PTY terminal on a remote node.
    Terminal {
        node: String,
        shell: Option<String>,
    },
}

pub async fn handle(action: RemoteAction, config: &Config) -> Result<()> {
    let manager = RemoteManager::new(config.clone());
    match action {
        RemoteAction::Status => status(&manager),
        RemoteAction::Init {
            network_id,
            token_key,
        } => init(config, network_id.as_deref(), token_key.as_deref()).await,
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
        RemoteAction::SyncDrivers { node, dry_run } => {
            sync_drivers(config, node.as_deref(), dry_run).await
        }
        RemoteAction::PresenceList => presence_list(&manager),
        RemoteAction::Terminal { node, shell } => {
            terminal(&manager, &node, shell.as_deref()).await
        }
    }
}

async fn init(config: &Config, network_id: Option<&str>, token_key: Option<&str>) -> Result<()> {
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    let current = &config.remote.transports.zerotier;
    let mut manager = ZeroTierManager::new(config.clone());
    let mut status = manager.status().await?;

    println!("Rove remote init");
    println!("Transport: ZeroTier");
    println!(
        "State: enabled={} installed={} configured={} joined={}",
        status.enabled, status.installed, status.configured, status.joined
    );

    if !status.installed {
        if interactive && !prompt_confirm("Install ZeroTierOne now?", true)? {
            println!("Skipped install. Run `rove remote transport install zerotier` when ready.");
            return Ok(());
        }
        let _ = manager.install().await?;
        manager = ZeroTierManager::new(Config::load_or_create()?);
        println!("Installed and enabled ZeroTier transport.");
    }

    let network_id = match network_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| current.network_id.clone())
    {
        Some(value) => value,
        None if interactive => prompt_required("ZeroTier network id", "")?,
        None => anyhow::bail!(
            "No ZeroTier network id provided. Use `rove remote init --network <id>` or run interactively."
        ),
    };

    let token_key = token_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| current.api_token_key.clone())
        .unwrap_or_else(|| "zerotier_api_token".to_string());

    let managed_name_sync = if interactive {
        prompt_confirm(
            "Enable ZeroTier managed-name sync?",
            current.managed_name_sync,
        )?
    } else {
        current.managed_name_sync
    };

    status = manager
        .setup(&network_id, Some(token_key.as_str()), managed_name_sync)
        .await?;
    println!("Configured ZeroTier network '{}'.", network_id);

    if !status.token_configured {
        println!(
            "Token secret '{}' is not configured. Rove will use the local ZeroTier auth token if present.",
            token_key
        );
        println!(
            "If controller access stays unavailable, set it with: rove secrets set {}",
            token_key
        );
    }

    if interactive {
        let should_join = if status.joined {
            prompt_confirm("Refresh the network join + discovery state now?", true)?
        } else {
            prompt_confirm("Join the configured ZeroTier network now?", true)?
        };
        if should_join {
            let _ = manager.join(Some(&network_id)).await?;
        }
    } else {
        let _ = manager.join(Some(&network_id)).await?;
    }

    status = manager.refresh().await?;
    print_zerotier_status(&status);

    if status.candidate_count > 0 {
        println!("Next: run `rove remote discover list` to inspect and trust visible nodes.");
    } else {
        println!(
            "Next: run `rove remote discover refresh` after the other node joins the same network."
        );
    }
    println!("Then: trust a candidate with `rove remote discover trust <candidate-id>`.");
    Ok(())
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
    println!(
        "Note: ZeroTier is available as an optional transport. Iroh is recommended for most users."
    );
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

fn presence_list(manager: &RemoteManager) -> Result<()> {
    let entries = manager.presence_list();
    if entries.is_empty() {
        println!("No live peers in the presence cache (wait up to 30s for heartbeats).");
        return Ok(());
    }
    println!("Live peers (presence score, descending):");
    for (entry, score) in entries {
        println!(
            "- {} score={:.2} load={:.0}% active_tui={} last_seen={}s ago",
            entry.node_id,
            score,
            entry.load * 100.0,
            entry.active_tui,
            entry.last_seen.elapsed().as_secs(),
        );
    }
    Ok(())
}

async fn terminal(manager: &RemoteManager, node: &str, shell: Option<&str>) -> Result<()> {
    println!("Opening remote terminal on '{}'...", node);
    manager.open_terminal(node, shell).await
}

async fn sync_drivers(config: &Config, node: Option<&str>, dry_run: bool) -> Result<()> {
    let drivers = collect_syncable_drivers(config).await?;
    if drivers.is_empty() {
        println!(
            "No syncable drivers found. Only official and catalog-managed drivers can be pushed."
        );
        return Ok(());
    }

    let manager = RemoteManager::new(config.clone());
    let results = manager.sync_drivers(node, &drivers, dry_run).await?;
    if results.is_empty() {
        println!("No trusted remote nodes matched the requested target.");
        return Ok(());
    }

    println!(
        "{} driver sync result(s):",
        if dry_run { "Planned" } else { "Applied" }
    );
    for result in results {
        if let Some(detail) = result.detail {
            println!(
                "- {} :: {} {} {} ({})",
                result.node_name, result.driver_id, result.action, result.status, detail
            );
        } else {
            println!(
                "- {} :: {} {} {} version={}",
                result.node_name, result.driver_id, result.action, result.status, result.version
            );
        }
    }

    Ok(())
}

async fn collect_syncable_drivers(config: &Config) -> Result<Vec<RemoteDriverSyncItem>> {
    let database = Database::new(&database_path(config))
        .await
        .context("Failed to open database for driver sync")?;
    let installed = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed drivers")?;

    let mut drivers = Vec::new();
    for plugin in installed
        .into_iter()
        .filter(|plugin| plugin.plugin_type == "Plugin" || plugin.plugin_type == "Workspace")
    {
        if let Some(item) = syncable_driver_from_plugin(&plugin) {
            drivers.push(item);
        }
    }

    drivers.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(drivers)
}

fn syncable_driver_from_plugin(
    plugin: &crate::storage::InstalledPlugin,
) -> Option<RemoteDriverSyncItem> {
    if is_official_driver(&plugin.id) {
        return Some(RemoteDriverSyncItem {
            id: plugin.id.clone(),
            source: plugin.id.clone(),
            registry: None,
            version: plugin.version.clone(),
            enabled: plugin.enabled,
        });
    }

    match plugin.provenance_source.as_deref() {
        Some("public_catalog") | Some("public_catalog_upgrade") => Some(RemoteDriverSyncItem {
            id: plugin.id.clone(),
            source: plugin.id.clone(),
            registry: None,
            version: plugin.version.clone(),
            enabled: plugin.enabled,
        }),
        Some("explicit_registry") => plugin
            .provenance_registry
            .as_deref()
            .filter(|registry| registry.starts_with("http://") || registry.starts_with("https://"))
            .map(|registry| RemoteDriverSyncItem {
                id: plugin.id.clone(),
                source: plugin.id.clone(),
                registry: Some(registry.to_string()),
                version: plugin.version.clone(),
                enabled: plugin.enabled,
            }),
        _ => None,
    }
}

fn is_official_driver(id: &str) -> bool {
    matches!(id, "filesystem" | "terminal" | "vision" | "voice-native")
}

fn prompt_required(label: &str, default: &str) -> Result<String> {
    loop {
        let value = prompt_line(label, default)?;
        if !value.trim().is_empty() {
            return Ok(value);
        }
        println!("A value is required.");
    }
}

fn prompt_confirm(label: &str, default: bool) -> Result<bool> {
    let suffix = if default { "Y/n" } else { "y/N" };
    loop {
        let answer = prompt_line(&format!("{} [{}]", label, suffix), "")?;
        let normalized = answer.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(default);
        }
        match normalized.as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Enter yes or no."),
        }
    }
}

fn prompt_line(label: &str, default: &str) -> Result<String> {
    let mut stdout = io::stdout();
    if default.is_empty() {
        write!(stdout, "{}: ", label)?;
    } else {
        write!(stdout, "{} [{}]: ", label, default)?;
    }
    stdout.flush()?;

    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::prompt_line;

    #[test]
    fn prompt_line_helper_keeps_default_shape() {
        let _ = prompt_line as fn(&str, &str) -> anyhow::Result<String>;
    }
}
