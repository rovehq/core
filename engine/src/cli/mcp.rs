use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::cli::McpAction;
use crate::config::Config;
use crate::runtime::mcp::{
    McpSandbox, McpServerConfig, McpSpawner, McpToolDescriptor, SandboxProfile,
};

#[derive(Debug, Clone)]
struct McpTemplate {
    key: String,
    description: String,
    command: Option<String>,
    args: Vec<String>,
    profile: SandboxProfile,
    secrets: Vec<String>,
    source: String,
}

#[derive(Debug, Deserialize)]
struct TemplateFile {
    description: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    allow_network: bool,
    #[serde(default)]
    allow_tmp: bool,
    #[serde(default)]
    read_paths: Vec<PathBuf>,
    #[serde(default)]
    write_paths: Vec<PathBuf>,
    #[serde(default)]
    secrets: Vec<String>,
}

pub async fn handle(action: McpAction, config: &Config) -> Result<()> {
    match action {
        McpAction::List => list_servers(config),
        McpAction::Show { name } => show_server(config, &name),
        McpAction::Templates => list_templates(config),
        McpAction::Add {
            name,
            template,
            command,
            args,
            description,
            allow_network,
            allow_tmp,
            workspace_read,
            workspace_write,
            read_paths,
            write_paths,
            disabled,
        } => {
            let request = AddServerRequest {
                name,
                template,
                command,
                args,
                description,
                allow_network,
                allow_tmp,
                workspace_read,
                workspace_write,
                read_paths,
                write_paths,
                disabled,
            };
            add_server(config, request)
        }
        McpAction::Enable { name } => set_enabled(&name, true),
        McpAction::Disable { name } => set_enabled(&name, false),
        McpAction::Remove { name } => remove_server(&name),
        McpAction::Test { name } => test_server(config, &name).await,
        McpAction::Tools { name } => list_server_tools(config, &name).await,
    }
}

#[derive(Debug)]
struct AddServerRequest {
    name: String,
    template: String,
    command: Option<String>,
    args: Vec<String>,
    description: Option<String>,
    allow_network: bool,
    allow_tmp: bool,
    workspace_read: bool,
    workspace_write: bool,
    read_paths: Vec<PathBuf>,
    write_paths: Vec<PathBuf>,
    disabled: bool,
}

fn list_servers(config: &Config) -> Result<()> {
    if config.mcp.servers.is_empty() {
        println!("No MCP servers configured.");
        println!("Use `rove mcp templates` and `rove mcp add ...` to configure one.");
        return Ok(());
    }

    println!("Configured MCP servers:");
    for server in &config.mcp.servers {
        let status = if server.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let template = server.template.as_deref().unwrap_or("custom");
        println!(
            "- {} [{}] template={} command={} {}",
            server.name,
            status,
            template,
            server.command,
            server.args.join(" ")
        );
    }

    Ok(())
}

fn show_server(config: &Config, name: &str) -> Result<()> {
    let server = resolve_server(config, name)?;

    println!("name: {}", server.name);
    println!("enabled: {}", server.enabled);
    println!(
        "template: {}",
        server.template.as_deref().unwrap_or("custom")
    );
    if let Some(description) = &server.description {
        println!("description: {}", description);
    }
    println!("command: {}", server.command);
    if server.args.is_empty() {
        println!("args: []");
    } else {
        println!("args:");
        for arg in &server.args {
            println!("- {}", arg);
        }
    }
    println!("sandbox:");
    println!("  network: {}", server.profile.allow_network);
    println!("  tmp: {}", server.profile.allow_tmp);
    println!("  read_paths:");
    if server.profile.read_paths.is_empty() {
        println!("  - (none)");
    } else {
        for path in &server.profile.read_paths {
            println!("  - {}", path.to_string_lossy());
        }
    }
    println!("  write_paths:");
    if server.profile.write_paths.is_empty() {
        println!("  - (none)");
    } else {
        for path in &server.profile.write_paths {
            println!("  - {}", path.to_string_lossy());
        }
    }

    Ok(())
}

fn list_templates(config: &Config) -> Result<()> {
    let templates = load_templates(config)?;
    println!("Available MCP templates:");
    for template in templates.values() {
        println!("- {} [{}]", template.key, template.source,);
        println!("  {}", template.description);
        if let Some(command) = &template.command {
            let args = if template.args.is_empty() {
                String::new()
            } else {
                format!(" {}", template.args.join(" "))
            };
            println!("  default command: {}{}", command, args);
        }
        if !template.secrets.is_empty() {
            println!("  secrets: {}", template.secrets.join(", "));
        }
    }
    Ok(())
}

fn add_server(config: &Config, request: AddServerRequest) -> Result<()> {
    let templates = load_templates(config)?;
    let template = templates
        .get(&request.template)
        .with_context(|| format!("unknown MCP template '{}'", request.template))?;

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
    if writable
        .mcp
        .servers
        .iter()
        .any(|server| server.name == request.name)
    {
        bail!("MCP server '{}' already exists", request.name);
    }

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

fn set_enabled(name: &str, enabled: bool) -> Result<()> {
    let mut config = Config::load_or_create()?;
    let server = config
        .mcp
        .servers
        .iter_mut()
        .find(|server| server.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown MCP server '{}'", name))?;
    server.enabled = enabled;
    config.save()?;
    println!(
        "{} MCP server '{}'.",
        if enabled { "Enabled" } else { "Disabled" },
        name
    );
    Ok(())
}

fn remove_server(name: &str) -> Result<()> {
    let mut config = Config::load_or_create()?;
    let original_len = config.mcp.servers.len();
    config.mcp.servers.retain(|server| server.name != name);
    if config.mcp.servers.len() == original_len {
        bail!("unknown MCP server '{}'", name);
    }
    config.save()?;
    println!("Removed MCP server '{}'.", name);
    Ok(())
}

async fn test_server(config: &Config, name: &str) -> Result<()> {
    McpSandbox::check_availability().context("MCP sandbox is not available")?;
    let server = resolve_server(config, name)?;
    if !server.enabled {
        bail!("MCP server '{}' is configured but disabled", name);
    }

    let spawner = Arc::new(McpSpawner::new(vec![server]));
    let tools = spawner.list_tools(name).await?;
    cache_tools(name, &tools)?;
    println!(
        "MCP server '{}' started successfully and exposed {} tool(s).",
        name,
        tools.len()
    );
    spawner.stop_all().await;
    Ok(())
}

async fn list_server_tools(config: &Config, name: &str) -> Result<()> {
    McpSandbox::check_availability().context("MCP sandbox is not available")?;
    let server = resolve_server(config, name)?;
    if !server.enabled {
        bail!("MCP server '{}' is configured but disabled", name);
    }

    let spawner = Arc::new(McpSpawner::new(vec![server]));
    let tools = spawner.list_tools(name).await?;
    cache_tools(name, &tools)?;
    print_tools(name, &tools)?;
    spawner.stop_all().await;
    Ok(())
}

fn cache_tools(name: &str, tools: &[McpToolDescriptor]) -> Result<()> {
    let mut config = Config::load_or_create()?;
    let server = config
        .mcp
        .servers
        .iter_mut()
        .find(|server| server.name == name)
        .ok_or_else(|| anyhow::anyhow!("unknown MCP server '{}'", name))?;
    server.cached_tools = tools.to_vec();
    config.save()?;
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

fn resolve_server(config: &Config, name: &str) -> Result<McpServerConfig> {
    config
        .mcp
        .servers
        .iter()
        .find(|server| server.name == name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown MCP server '{}'", name))
}

fn load_templates(config: &Config) -> Result<BTreeMap<String, McpTemplate>> {
    let mut templates = built_in_templates(config);
    for dir in template_dirs(config)? {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read template dir {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }

            let contents = fs::read_to_string(&path)
                .with_context(|| format!("failed to read template {}", path.display()))?;
            let file: TemplateFile = toml::from_str(&contents)
                .with_context(|| format!("failed to parse template {}", path.display()))?;

            let key = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid template filename {}", path.display()))?
                .to_string();

            templates.insert(
                key.clone(),
                McpTemplate {
                    key,
                    description: file.description,
                    command: file.command,
                    args: file.args,
                    profile: SandboxProfile {
                        allow_network: file.allow_network,
                        read_paths: file.read_paths,
                        write_paths: file.write_paths,
                        allow_tmp: file.allow_tmp,
                    },
                    secrets: file.secrets,
                    source: path.display().to_string(),
                },
            );
        }
    }

    Ok(templates)
}

fn built_in_templates(config: &Config) -> BTreeMap<String, McpTemplate> {
    let mut templates = BTreeMap::new();
    templates.insert(
        "custom".to_string(),
        McpTemplate {
            key: "custom".to_string(),
            description: "Bring your own stdio MCP server command and sandbox profile.".to_string(),
            command: None,
            args: Vec::new(),
            profile: SandboxProfile::default(),
            secrets: Vec::new(),
            source: "builtin".to_string(),
        },
    );
    templates.insert(
        "github".to_string(),
        McpTemplate {
            key: "github".to_string(),
            description: "Networked GitHub MCP server profile. Community templates can fill in the exact command.".to_string(),
            command: None,
            args: Vec::new(),
            profile: SandboxProfile::default().with_network().with_tmp(),
            secrets: vec!["github_token".to_string()],
            source: "builtin".to_string(),
        },
    );
    templates.insert(
        "notion".to_string(),
        McpTemplate {
            key: "notion".to_string(),
            description: "Networked Notion MCP server profile.".to_string(),
            command: None,
            args: Vec::new(),
            profile: SandboxProfile::default().with_network().with_tmp(),
            secrets: vec!["notion_token".to_string()],
            source: "builtin".to_string(),
        },
    );
    templates.insert(
        "slack".to_string(),
        McpTemplate {
            key: "slack".to_string(),
            description: "Networked Slack MCP server profile.".to_string(),
            command: None,
            args: Vec::new(),
            profile: SandboxProfile::default().with_network().with_tmp(),
            secrets: vec!["slack_bot_token".to_string()],
            source: "builtin".to_string(),
        },
    );
    templates.insert(
        "workspace-files".to_string(),
        McpTemplate {
            key: "workspace-files".to_string(),
            description: "Workspace-scoped profile for filesystem-oriented MCP servers."
                .to_string(),
            command: None,
            args: Vec::new(),
            profile: SandboxProfile::default()
                .with_read_path(config.core.workspace.clone())
                .with_write_path(config.core.workspace.clone()),
            secrets: Vec::new(),
            source: "builtin".to_string(),
        },
    );
    templates
}

fn template_dirs(config: &Config) -> Result<Vec<PathBuf>> {
    let config_dir = Config::config_path()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("config path has no parent"))?;
    Ok(vec![
        config_dir.join("mcp").join("templates"),
        config
            .core
            .workspace
            .join(".rove")
            .join("mcp")
            .join("templates"),
    ])
}

fn normalize_profile(profile: &mut SandboxProfile) {
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
