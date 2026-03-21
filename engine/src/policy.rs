use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::config::Config;
use crate::steering::loader::{PolicyEngine, PolicyRecord};

#[derive(Debug, Clone, Serialize)]
pub struct PolicySummary {
    pub id: String,
    pub path: PathBuf,
    pub active: bool,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyExplainReport {
    pub task: String,
    pub domain: String,
    pub active_policies: Vec<String>,
    pub matched_hints: Vec<String>,
    pub system_prefix: String,
    pub system_suffix: String,
    pub verification_commands: Vec<String>,
    pub preferred_providers: Vec<String>,
    pub preferred_tools: Vec<String>,
    pub memory_tags: Vec<String>,
}

pub struct PolicyManager {
    config: Config,
    policy_dir: PathBuf,
    workspace_dir: PathBuf,
    legacy_workspace_dir: PathBuf,
}

impl PolicyManager {
    pub fn new(config: Config, policy_dir_override: Option<PathBuf>) -> Self {
        let policy_dir =
            policy_dir_override.unwrap_or_else(|| config.steering.policy_dir().clone());
        let workspace_dir = config.core.workspace.join(".rove").join("policy");
        let legacy_workspace_dir = config.core.workspace.join(".rove").join("steering");
        Self {
            config,
            policy_dir,
            workspace_dir,
            legacy_workspace_dir,
        }
    }

    pub async fn list(&self) -> Result<Vec<PolicySummary>> {
        let engine = self.load_engine().await?;
        let mut policies = Vec::new();
        for policy in engine.list_policies().await {
            policies.push(PolicySummary {
                id: policy.id.clone(),
                path: policy.file_path.clone(),
                active: engine.is_policy_active(&policy.id).await,
                scope: self.scope_for(&policy.file_path),
            });
        }
        policies.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(policies)
    }

    pub async fn active(&self) -> Result<Vec<String>> {
        let engine = self.load_engine().await?;
        let domain = infer_domain(&self.config.core.workspace).to_string();
        engine.auto_activate_policies("", 0, Some(&domain)).await;
        Ok(engine.active_policies().await)
    }

    pub async fn get(&self, name: &str) -> Result<PolicyRecord> {
        let engine = self.load_engine().await?;
        engine
            .get_policy(name)
            .await
            .with_context(|| format!("Policy '{}' not found", name))
    }

    pub async fn enable(&self, name: &str) -> Result<()> {
        let mut config = self.config.clone();
        if !config
            .steering
            .default_policies()
            .iter()
            .any(|policy| policy.eq_ignore_ascii_case(name))
        {
            config.steering.default_policies_mut().push(name.to_string());
            config.save()?;
        }
        Ok(())
    }

    pub async fn disable(&self, name: &str) -> Result<()> {
        let mut config = self.config.clone();
        config
            .steering
            .default_policies_mut()
            .retain(|policy| !policy.eq_ignore_ascii_case(name));
        config.save()?;
        Ok(())
    }

    pub async fn bootstrap_defaults(&self) -> Result<()> {
        crate::steering::bootstrap_builtins(&self.policy_dir).await?;
        Ok(())
    }

    pub async fn explain(&self, task: &str) -> Result<PolicyExplainReport> {
        let engine = self.load_engine().await?;
        let domain = infer_domain(&self.config.core.workspace).to_string();
        engine.auto_activate_policies(task, 0, Some(&domain)).await;
        let active_policies = engine.active_policies().await;
        let matched_hints = engine.matched_hints(task).await;
        let directives = engine.get_directives_for_task(task).await;
        let routing = engine.get_routing_prefs().await;
        let skills = engine.list_policies().await;
        let mut preferred_tools = Vec::new();
        let mut verification_commands = Vec::new();

        for policy_id in &active_policies {
            let Some(skill) = skills.iter().find(|skill| &skill.id == policy_id) else {
                continue;
            };
            let Some(config) = &skill.config else {
                continue;
            };
            for tool in &config.tools.prefer {
                if !preferred_tools.contains(tool) {
                    preferred_tools.push(tool.clone());
                }
            }
            for command in &config.tools.suggest_after_code {
                if !verification_commands.contains(command) {
                    verification_commands.push(command.clone());
                }
            }
        }

        Ok(PolicyExplainReport {
            task: task.to_string(),
            domain,
            active_policies,
            matched_hints,
            system_prefix: directives.system_prefix,
            system_suffix: directives.system_suffix,
            verification_commands,
            preferred_providers: routing.preferred_providers,
            preferred_tools,
            memory_tags: directives.auto_tags,
        })
    }

    pub async fn add(&self, name: &str, scope: sdk::PolicyScope) -> Result<PathBuf> {
        let dir = match scope {
            sdk::PolicyScope::User => self.policy_dir.clone(),
            sdk::PolicyScope::Workspace | sdk::PolicyScope::Project => self.workspace_dir.clone(),
        };

        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{name}.toml"));
        if path.exists() {
            bail!("Policy '{}' already exists at {}", name, path.display());
        }

        let template = format!(
            "[meta]\nid = \"{name}\"\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"Policy {name}\"\nauthor = \"user\"\ntags = []\ndomains = [\"general\"]\n\n[activation]\nmanual = true\nauto_when = []\nconflicts_with = []\napply_only_to = []\nauto_when_file_type = []\n\n[directives]\nsystem_prefix = \"\"\nsystem_suffix = \"\"\n\n[routing]\npreferred_providers = []\navoid_providers = []\nalways_verify = false\n\n[tools]\nprefer = []\nsuggest_after_code = []\n\n[memory]\nauto_tag = []\n\n[hints]\n"
        );
        fs::write(&path, template)?;
        Ok(path)
    }

    pub async fn remove(&self, name: &str) -> Result<PathBuf> {
        let candidates = [
            self.policy_dir.join(format!("{name}.toml")),
            self.workspace_dir.join(format!("{name}.toml")),
            self.legacy_workspace_dir.join(format!("{name}.toml")),
        ];
        for candidate in &candidates {
            if candidate.exists() {
                fs::remove_file(candidate)?;
                return Ok(candidate.clone());
            }
        }
        bail!(
            "Policy '{}' does not exist in user or workspace policy directories",
            name
        )
    }

    async fn load_engine(&self) -> Result<PolicyEngine> {
        let workspace_dir = active_workspace_policy_dir(
            &self.workspace_dir,
            &self.legacy_workspace_dir,
        );
        let engine = PolicyEngine::new_with_workspace(&self.policy_dir, Some(&workspace_dir))
            .await
            .with_context(|| {
                format!(
                    "Failed to load policy engine from '{}' and '{}'",
                    self.policy_dir.display(),
                    workspace_dir.display()
                )
            })?;

        for default_policy in self.config.steering.default_policies() {
            if let Err(error) = engine.activate_policy(default_policy).await {
                tracing::warn!(
                    "Failed to activate persisted policy '{}': {}",
                    default_policy,
                    error
                );
            }
        }

        Ok(engine)
    }

    fn scope_for(&self, path: &Path) -> String {
        if path.starts_with(&self.workspace_dir) || path.starts_with(&self.legacy_workspace_dir) {
            "workspace".to_string()
        } else if path.starts_with(&self.policy_dir) {
            "user".to_string()
        } else {
            "external".to_string()
        }
    }
}

pub fn policy_workspace_dir(workspace: &Path) -> PathBuf {
    workspace.join(".rove").join("policy")
}

pub fn legacy_policy_workspace_dir(workspace: &Path) -> PathBuf {
    workspace.join(".rove").join("steering")
}

pub fn active_workspace_policy_dir(primary: &Path, legacy: &Path) -> PathBuf {
    if primary.exists() || !legacy.exists() {
        primary.to_path_buf()
    } else {
        legacy.to_path_buf()
    }
}

pub fn infer_domain(cwd: &Path) -> &'static str {
    if cwd.join("Cargo.toml").exists() || cwd.join("src").exists() {
        return "code";
    }
    if cwd.join(".git").exists() {
        return "git";
    }
    "general"
}

pub fn explain_as_lines(report: &PolicyExplainReport) -> Vec<String> {
    let mut lines = vec![
        format!("task: {}", report.task),
        format!("domain: {}", report.domain),
        format!(
            "active_policies: {}",
            if report.active_policies.is_empty() {
                "<none>".to_string()
            } else {
                report.active_policies.join(", ")
            }
        ),
    ];

    let mut sections = BTreeMap::new();
    sections.insert("matched_hints", report.matched_hints.clone());
    sections.insert("preferred_providers", report.preferred_providers.clone());
    sections.insert("preferred_tools", report.preferred_tools.clone());
    sections.insert("verification_commands", report.verification_commands.clone());
    sections.insert("memory_tags", report.memory_tags.clone());

    for (name, items) in sections {
        if items.is_empty() {
            continue;
        }
        lines.push(format!("{name}:"));
        for item in items {
            lines.push(format!("- {item}"));
        }
    }

    if !report.system_prefix.is_empty() {
        lines.push(String::new());
        lines.push("system_prefix:".to_string());
        lines.push(report.system_prefix.clone());
    }
    if !report.system_suffix.is_empty() {
        lines.push(String::new());
        lines.push("system_suffix:".to_string());
        lines.push(report.system_suffix.clone());
    }

    lines
}
