use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tracing::warn;

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::runtime::mcp::{McpSandbox, McpServerConfig, McpSpawner, McpToolDescriptor};
use crate::storage::{Database, InstalledPlugin};

use super::templates::load_templates;
use super::AddServerRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
enum McpServerSource {
    Config,
    Package {
        plugin_id: String,
        plugin_name: String,
        version: String,
    },
}

#[derive(Debug, Clone)]
struct ManagedMcpServer {
    config: McpServerConfig,
    source: McpServerSource,
}

pub(super) async fn list_servers(config: &Config) -> Result<()> {
    let servers = merged_servers(config).await?;
    if servers.is_empty() {
        println!("No MCP servers configured.");
        println!("Use `rove mcp templates` and `rove mcp add ...` to configure one.");
        return Ok(());
    }

    println!("MCP servers:");
    for server in &servers {
        let status = if server.config.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let template = server.config.template.as_deref().unwrap_or("custom");
        println!(
            "- {} [{}] source={} template={} command={} {}",
            server.config.name,
            status,
            server.source.label(),
            template,
            server.config.command,
            server.config.args.join(" ")
        );
    }

    Ok(())
}

pub(super) async fn show_server(config: &Config, selector: &str) -> Result<()> {
    let server = resolve_server(config, selector).await?;

    println!("name: {}", server.config.name);
    println!("source: {}", server.source.label());
    if let McpServerSource::Package {
        plugin_id,
        plugin_name,
        version,
    } = &server.source
    {
        println!("plugin_id: {}", plugin_id);
        println!("plugin_name: {}", plugin_name);
        println!("plugin_version: {}", version);
    }
    println!("enabled: {}", server.config.enabled);
    println!(
        "template: {}",
        server.config.template.as_deref().unwrap_or("custom")
    );
    if let Some(description) = &server.config.description {
        println!("description: {}", description);
    }
    println!("command: {}", server.config.command);
    if server.config.args.is_empty() {
        println!("args: []");
    } else {
        println!("args:");
        for arg in &server.config.args {
            println!("- {}", arg);
        }
    }
    println!("sandbox:");
    println!("  network: {}", server.config.profile.allow_network);
    println!("  tmp: {}", server.config.profile.allow_tmp);
    println!("  read_paths:");
    if server.config.profile.read_paths.is_empty() {
        println!("  - (none)");
    } else {
        for path in &server.config.profile.read_paths {
            println!("  - {}", path.to_string_lossy());
        }
    }
    println!("  write_paths:");
    if server.config.profile.write_paths.is_empty() {
        println!("  - (none)");
    } else {
        for path in &server.config.profile.write_paths {
            println!("  - {}", path.to_string_lossy());
        }
    }

    Ok(())
}

pub(super) async fn add_server(config: &Config, request: AddServerRequest) -> Result<()> {
    let templates = load_templates(config)?;
    let template = templates
        .get(&request.template)
        .with_context(|| format!("unknown MCP template '{}'", request.template))?;

    if merged_servers(config)
        .await?
        .iter()
        .any(|server| server.config.name == request.name)
    {
        bail!("MCP server '{}' already exists", request.name);
    }

    let command = request
        .command
        .clone()
        .or_else(|| template.command.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "template '{}' does not define a command; pass --command explicitly",
                template.key
            )
        })?;

    let args = if request.args.is_empty() {
        template.args.clone()
    } else {
        request.args.clone()
    };

    let mut profile = template.profile.clone();
    if request.allow_network {
        profile.allow_network = true;
    }
    if request.allow_tmp {
        profile.allow_tmp = true;
    }
    if request.workspace_read {
        profile.read_paths.push(config.core.workspace.clone());
    }
    if request.workspace_write {
        profile.write_paths.push(config.core.workspace.clone());
    }
    profile.read_paths.extend(request.read_paths.clone());
    profile.write_paths.extend(request.write_paths.clone());
    normalize_profile(&mut profile);

    let mut writable = Config::load_or_create()?;
    writable.mcp.servers.push(McpServerConfig {
        name: request.name.clone(),
        template: Some(template.key.clone()),
        description: request
            .description
            .clone()
            .or_else(|| Some(template.description.clone())),
        command,
        args,
        profile,
        cached_tools: Vec::new(),
        enabled: !request.disabled,
    });
    writable.save()?;

    println!("Added MCP server '{}'.", request.name);
    println!("Config path: {}", Config::config_path()?.display());
    if !template.secrets.is_empty() {
        println!(
            "Template '{}' expects secrets/config such as: {}",
            template.key,
            template.secrets.join(", ")
        );
    }
    println!("Run `rove mcp test {}` to verify startup.", request.name);

    Ok(())
}

pub(super) async fn set_enabled(config: &Config, selector: &str, enabled: bool) -> Result<()> {
    let server = resolve_server(config, selector).await?;

    match server.source {
        McpServerSource::Config => {
            let mut writable = Config::load_or_create()?;
            let entry = writable
                .mcp
                .servers
                .iter_mut()
                .find(|candidate| candidate.name == server.config.name)
                .ok_or_else(|| anyhow::anyhow!("unknown MCP server '{}'", selector))?;
            entry.enabled = enabled;
            writable.save()?;
        }
        McpServerSource::Package { plugin_id, .. } => {
            let database = open_database(config).await?;
            database
                .installed_plugins()
                .set_enabled(&plugin_id, enabled)
                .await
                .context("Failed to update installed MCP plugin state")?;
        }
    }

    println!(
        "{} MCP server '{}'.",
        if enabled { "Enabled" } else { "Disabled" },
        server.config.name
    );
    Ok(())
}

pub(super) async fn remove_server(config: &Config, selector: &str) -> Result<()> {
    let server = resolve_server(config, selector).await?;

    match server.source {
        McpServerSource::Config => {
            let mut writable = Config::load_or_create()?;
            let original_len = writable.mcp.servers.len();
            writable
                .mcp
                .servers
                .retain(|candidate| candidate.name != server.config.name);
            if writable.mcp.servers.len() == original_len {
                bail!("unknown MCP server '{}'", selector);
            }
            writable.save()?;
        }
        McpServerSource::Package { plugin_id, .. } => {
            let database = open_database(config).await?;
            database
                .installed_plugins()
                .delete_plugin(&plugin_id)
                .await
                .context("Failed to remove installed MCP plugin")?;
        }
    }

    println!("Removed MCP server '{}'.", server.config.name);
    Ok(())
}

pub(super) async fn test_server(config: &Config, selector: &str) -> Result<()> {
    McpSandbox::check_availability().context("MCP sandbox is not available")?;
    let server = resolve_server(config, selector).await?;
    if !server.config.enabled {
        bail!(
            "MCP server '{}' is configured but disabled",
            server.config.name
        );
    }

    let spawner = Arc::new(McpSpawner::new(vec![server.config.clone()]));
    let tools = spawner.list_tools(&server.config.name).await?;
    cache_tools(config, &server, &tools).await?;
    println!(
        "MCP server '{}' started successfully and exposed {} tool(s).",
        server.config.name,
        tools.len()
    );
    spawner.stop_all().await;
    Ok(())
}

pub(super) async fn list_server_tools(config: &Config, selector: &str) -> Result<()> {
    McpSandbox::check_availability().context("MCP sandbox is not available")?;
    let server = resolve_server(config, selector).await?;
    if !server.config.enabled {
        bail!(
            "MCP server '{}' is configured but disabled",
            server.config.name
        );
    }

    let spawner = Arc::new(McpSpawner::new(vec![server.config.clone()]));
    let tools = spawner.list_tools(&server.config.name).await?;
    cache_tools(config, &server, &tools).await?;
    print_tools(&server.config.name, &tools)?;
    spawner.stop_all().await;
    Ok(())
}

fn print_tools(server_name: &str, tools: &[McpToolDescriptor]) -> Result<()> {
    println!("MCP tools for '{}':", server_name);
    if tools.is_empty() {
        println!("- (none)");
        return Ok(());
    }

    for tool in tools {
        println!("- {}", tool.name);
        if !tool.description.is_empty() {
            println!("  {}", tool.description);
        }
        println!(
            "  schema: {}",
            serde_json::to_string_pretty(&tool.input_schema)?
        );
    }
    Ok(())
}

async fn merged_servers(config: &Config) -> Result<Vec<ManagedMcpServer>> {
    let mut merged = BTreeMap::new();

    for server in config.mcp.servers.iter().cloned() {
        merged.insert(
            server.name.clone(),
            ManagedMcpServer {
                config: server,
                source: McpServerSource::Config,
            },
        );
    }

    for server in load_installed_package_servers(config).await? {
        if merged
            .insert(server.config.name.clone(), server.clone())
            .is_some()
        {
            warn!(
                server = %server.config.name,
                "Installed MCP package overrides config-backed MCP server"
            );
        }
    }

    Ok(merged.into_values().collect())
}

async fn load_installed_package_servers(config: &Config) -> Result<Vec<ManagedMcpServer>> {
    let database = open_database(config).await?;
    let plugins = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins")?;

    let mut servers = Vec::new();
    for plugin in plugins {
        if let Some(server) = installed_plugin_server(plugin)? {
            servers.push(server);
        }
    }

    Ok(servers)
}

fn installed_plugin_server(plugin: InstalledPlugin) -> Result<Option<ManagedMcpServer>> {
    if plugin.plugin_type != "Mcp" {
        return Ok(None);
    }

    let raw = match plugin.config.as_deref() {
        Some(raw) if !raw.trim().is_empty() => raw,
        _ => {
            warn!(
                plugin = %plugin.name,
                "Skipping installed MCP plugin because runtime config is missing"
            );
            return Ok(None);
        }
    };

    let mut config: McpServerConfig =
        serde_json::from_str(raw).context("Invalid installed MCP runtime config")?;
    config.enabled = plugin.enabled;

    Ok(Some(ManagedMcpServer {
        config,
        source: McpServerSource::Package {
            plugin_id: plugin.id,
            plugin_name: plugin.name,
            version: plugin.version,
        },
    }))
}

async fn resolve_server(config: &Config, selector: &str) -> Result<ManagedMcpServer> {
    let servers = merged_servers(config).await?;
    servers
        .into_iter()
        .find(|server| server.matches_selector(selector))
        .ok_or_else(|| anyhow::anyhow!("unknown MCP server '{}'", selector))
}

async fn cache_tools(
    config: &Config,
    server: &ManagedMcpServer,
    tools: &[McpToolDescriptor],
) -> Result<()> {
    match &server.source {
        McpServerSource::Config => {
            let mut writable = Config::load_or_create()?;
            let entry = writable
                .mcp
                .servers
                .iter_mut()
                .find(|candidate| candidate.name == server.config.name)
                .ok_or_else(|| anyhow::anyhow!("unknown MCP server '{}'", server.config.name))?;
            entry.cached_tools = tools.to_vec();
            writable.save()?;
        }
        McpServerSource::Package { plugin_id, .. } => {
            let database = open_database(config).await?;
            let mut plugin = database
                .installed_plugins()
                .get_plugin(plugin_id)
                .await
                .context("Failed to load installed MCP plugin for tool cache")?
                .ok_or_else(|| anyhow::anyhow!("unknown MCP plugin '{}'", plugin_id))?;
            let mut runtime_config: McpServerConfig = serde_json::from_str(
                plugin
                    .config
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("installed MCP plugin config is missing"))?,
            )
            .context("Invalid installed MCP runtime config")?;
            runtime_config.cached_tools = tools.to_vec();
            runtime_config.enabled = plugin.enabled;
            plugin.config = Some(serde_json::to_string_pretty(&runtime_config)?);
            database
                .installed_plugins()
                .upsert_plugin(&plugin)
                .await
                .context("Failed to store installed MCP tool cache")?;
        }
    }

    Ok(())
}

async fn open_database(config: &Config) -> Result<Database> {
    Database::new(&database_path(config))
        .await
        .context("Failed to open database")
}

fn normalize_profile(profile: &mut crate::runtime::SandboxProfile) {
    profile.read_paths = dedup_paths(&profile.read_paths);
    profile.write_paths = dedup_paths(&profile.write_paths);
}

fn dedup_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            unique.push(path.clone());
        }
    }
    unique
}

impl McpServerSource {
    fn label(&self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Package { .. } => "package",
        }
    }
}

impl ManagedMcpServer {
    fn matches_selector(&self, selector: &str) -> bool {
        if self.config.name == selector {
            return true;
        }

        match &self.source {
            McpServerSource::Config => false,
            McpServerSource::Package {
                plugin_id,
                plugin_name,
                ..
            } => plugin_id == selector || plugin_name == selector,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::{McpServerConfig, SandboxProfile};
    use crate::storage::InstalledPlugin;

    use super::{installed_plugin_server, ManagedMcpServer, McpServerSource};

    fn config_server(name: &str, command: &str) -> ManagedMcpServer {
        ManagedMcpServer {
            config: McpServerConfig {
                name: name.to_string(),
                template: Some("custom".to_string()),
                description: None,
                command: command.to_string(),
                args: Vec::new(),
                profile: SandboxProfile::default(),
                cached_tools: Vec::new(),
                enabled: true,
            },
            source: McpServerSource::Config,
        }
    }

    #[test]
    fn installed_plugin_server_uses_plugin_enabled_state() {
        let plugin = InstalledPlugin {
            id: "github-package".to_string(),
            name: "github-package".to_string(),
            version: "0.2.0".to_string(),
            plugin_type: "Mcp".to_string(),
            trust_tier: 1,
            manifest: "{}".to_string(),
            binary_path: None,
            binary_hash: "abc123".to_string(),
            signature: "deadbeef".to_string(),
            enabled: false,
            installed_at: 1_710_000_000,
            last_used: None,
            config: Some(
                r#"{
                    "name": "github",
                    "template": "github",
                    "description": "GitHub MCP",
                    "command": "github-mcp",
                    "args": [],
                    "profile": {
                        "allow_network": true,
                        "read_paths": [],
                        "write_paths": [],
                        "allow_tmp": false
                    },
                    "cached_tools": [],
                    "enabled": true
                }"#
                .to_string(),
            ),
        };

        let server = installed_plugin_server(plugin)
            .expect("installed MCP server")
            .expect("server");

        assert!(!server.config.enabled);
    }

    #[test]
    fn package_selector_matches_server_name_plugin_id_and_plugin_name() {
        let server = ManagedMcpServer {
            config: McpServerConfig {
                name: "github".to_string(),
                template: Some("github".to_string()),
                description: None,
                command: "github-mcp".to_string(),
                args: Vec::new(),
                profile: SandboxProfile::default(),
                cached_tools: Vec::new(),
                enabled: true,
            },
            source: McpServerSource::Package {
                plugin_id: "github-package".to_string(),
                plugin_name: "GitHub Package".to_string(),
                version: "0.2.0".to_string(),
            },
        };

        assert!(server.matches_selector("github"));
        assert!(server.matches_selector("github-package"));
        assert!(server.matches_selector("GitHub Package"));
        assert!(!server.matches_selector("slack"));
    }

    #[test]
    fn config_selector_matches_server_name_only() {
        let server = config_server("slack", "slack-mcp");

        assert!(server.matches_selector("slack"));
        assert!(!server.matches_selector("slack-package"));
    }
}
