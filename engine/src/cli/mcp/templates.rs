use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::Config;
use crate::runtime::SandboxProfile;

#[derive(Debug, Clone)]
pub(super) struct McpTemplate {
    pub(super) key: String,
    pub(super) description: String,
    pub(super) command: Option<String>,
    pub(super) args: Vec<String>,
    pub(super) profile: SandboxProfile,
    pub(super) secrets: Vec<String>,
    pub(super) source: String,
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

pub(super) fn list_templates(config: &Config) -> Result<()> {
    let templates = load_templates(config)?;
    println!("Available MCP templates:");
    for template in templates.values() {
        println!("- {} [{}]", template.key, template.source);
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

pub(super) fn load_templates(config: &Config) -> Result<BTreeMap<String, McpTemplate>> {
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
            description:
                "Networked GitHub MCP server profile. Community templates can fill in the exact command."
                    .to_string(),
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
