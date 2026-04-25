use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sdk::{AgentSpec, CapabilityRef, WorkflowSpec};

use crate::config::Config;
use crate::system::worker_presets;

const DEFAULT_AGENT_ID: &str = "default-assistant";

pub struct SpecRepository {
    root: PathBuf,
}

impl SpecRepository {
    pub fn new() -> Result<Self> {
        let config_path = Config::config_path()?;
        let root = config_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Config path is missing a parent directory"))?
            .to_path_buf();
        let repo = Self { root };
        repo.ensure_layout()?;
        repo.ensure_default_assistant()?;
        Ok(repo)
    }

    pub fn list_agents(&self) -> Result<Vec<AgentSpec>> {
        let mut specs = self.load_specs::<AgentSpec>(&self.agents_dir())?;
        specs.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(specs)
    }

    pub fn load_agent(&self, selector: &str) -> Result<AgentSpec> {
        self.load_spec::<AgentSpec>(&self.agents_dir(), selector)
    }

    pub fn save_agent(&self, spec: &AgentSpec) -> Result<AgentSpec> {
        validate_agent(spec)?;
        self.write_spec(&self.agents_dir(), &spec.id, spec)?;
        Ok(spec.clone())
    }

    pub fn remove_agent(&self, selector: &str) -> Result<bool> {
        self.remove_spec(&self.agents_dir(), selector)
    }

    pub fn list_workflows(&self) -> Result<Vec<WorkflowSpec>> {
        let mut specs = self.load_specs::<WorkflowSpec>(&self.workflows_dir())?;
        specs.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(specs)
    }

    pub fn load_workflow(&self, selector: &str) -> Result<WorkflowSpec> {
        self.load_spec::<WorkflowSpec>(&self.workflows_dir(), selector)
    }

    pub fn save_workflow(&self, spec: &WorkflowSpec) -> Result<WorkflowSpec> {
        validate_workflow(spec)?;
        self.write_spec(&self.workflows_dir(), &spec.id, spec)?;
        Ok(spec.clone())
    }

    pub fn remove_workflow(&self, selector: &str) -> Result<bool> {
        self.remove_spec(&self.workflows_dir(), selector)
    }

    pub fn export_agent(&self, selector: &str, path: &Path) -> Result<PathBuf> {
        let spec = self.load_agent(selector)?;
        self.export_spec(path, &spec.id, &spec)
    }

    pub fn import_agent(&self, path: &Path) -> Result<AgentSpec> {
        let spec: AgentSpec = self.read_spec_file(path)?;
        self.save_agent(&spec)
    }

    pub fn export_workflow(&self, selector: &str, path: &Path) -> Result<PathBuf> {
        let spec = self.load_workflow(selector)?;
        self.export_spec(path, &spec.id, &spec)
    }

    pub fn import_workflow(&self, path: &Path) -> Result<WorkflowSpec> {
        let spec: WorkflowSpec = self.read_spec_file(path)?;
        self.save_workflow(&spec)
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.root.join("agents")
    }

    pub fn workflows_dir(&self) -> PathBuf {
        self.root.join("workflows")
    }

    fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.agents_dir()).context("Failed to create agents directory")?;
        fs::create_dir_all(self.workflows_dir()).context("Failed to create workflows directory")?;
        Ok(())
    }

    fn ensure_default_assistant(&self) -> Result<()> {
        let path = self.agents_dir().join(format!("{DEFAULT_AGENT_ID}.toml"));
        if path.exists() {
            return Ok(());
        }

        let spec = AgentSpec {
            id: DEFAULT_AGENT_ID.to_string(),
            name: "Default Assistant".to_string(),
            purpose: "General-purpose Rove assistant for everyday local tasks.".to_string(),
            instructions: "Help the user complete local tasks safely and directly. Use tools when needed, prefer concise answers, and respect approvals and policy.".to_string(),
            capabilities: vec![
                CapabilityRef {
                    kind: "tool".to_string(),
                    name: "read_file".to_string(),
                    required: false,
                },
                CapabilityRef {
                    kind: "tool".to_string(),
                    name: "write_file".to_string(),
                    required: false,
                },
                CapabilityRef {
                    kind: "tool".to_string(),
                    name: "list_dir".to_string(),
                    required: false,
                },
                CapabilityRef {
                    kind: "tool".to_string(),
                    name: "run_command".to_string(),
                    required: false,
                },
            ],
            ui: sdk::AgentUiSchema {
                icon: Some("◎".to_string()),
                accent: Some("primary".to_string()),
            },
            tags: vec!["default".to_string(), "assistant".to_string()],
            ..AgentSpec::default()
        };
        self.save_agent(&spec)?;
        Ok(())
    }

    fn load_specs<T>(&self, dir: &Path) -> Result<Vec<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut specs = Vec::new();
        for entry in
            fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            if entry.path().extension().and_then(|value| value.to_str()) != Some("toml") {
                continue;
            }
            specs.push(self.read_spec_file(&entry.path())?);
        }
        Ok(specs)
    }

    fn load_spec<T>(&self, dir: &Path, selector: &str) -> Result<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let direct = dir.join(format!("{}.toml", slugify(selector)));
        if direct.exists() {
            return self.read_spec_file(&direct);
        }

        let selector_lower = selector.trim().to_ascii_lowercase();
        for entry in self.load_specs::<T>(dir)? {
            let value = toml::Value::try_from(&entry)
                .context("Failed to inspect loaded spec while resolving selector")?;
            let id = value
                .get("id")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let name = value
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if selector_lower == id || selector_lower == name {
                return Ok(entry);
            }
        }

        bail!("No spec found for '{}'", selector);
    }

    fn remove_spec(&self, dir: &Path, selector: &str) -> Result<bool> {
        let direct = dir.join(format!("{}.toml", slugify(selector)));
        if direct.exists() {
            fs::remove_file(&direct)
                .with_context(|| format!("Failed to remove {}", direct.display()))?;
            return Ok(true);
        }

        let selector_lower = selector.trim().to_ascii_lowercase();
        for entry in
            fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let value: toml::Value = self.read_spec_file(&entry.path())?;
            let id = value
                .get("id")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let name = value
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if selector_lower == id || selector_lower == name {
                fs::remove_file(entry.path())
                    .with_context(|| format!("Failed to remove {}", entry.path().display()))?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn export_spec<T>(&self, path: &Path, id: &str, spec: &T) -> Result<PathBuf>
    where
        T: serde::Serialize,
    {
        let target = if path.extension().is_some() {
            path.to_path_buf()
        } else {
            path.join(format!("{id}.toml"))
        };

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let raw = toml::to_string_pretty(spec).context("Failed to serialize spec")?;
        fs::write(&target, raw).with_context(|| format!("Failed to write {}", target.display()))?;
        Ok(target)
    }

    fn read_spec_file<T>(&self, path: &Path) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
    }

    fn write_spec<T>(&self, dir: &Path, id: &str, spec: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        let path = dir.join(format!("{}.toml", slugify(id)));
        let raw = toml::to_string_pretty(spec).context("Failed to serialize spec")?;
        fs::write(&path, raw).with_context(|| format!("Failed to write {}", path.display()))
    }
}

pub fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in value.trim().chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            out.push(normalized);
            previous_dash = false;
        } else if !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn allowed_tools(spec: &AgentSpec) -> Vec<String> {
    spec.capabilities
        .iter()
        .filter(|capability| {
            matches!(
                capability.kind.trim().to_ascii_lowercase().as_str(),
                "tool" | "builtin" | "builtin_tool"
            )
        })
        .map(|capability| capability.name.clone())
        .collect()
}

fn validate_agent(spec: &AgentSpec) -> Result<()> {
    if spec.id.trim().is_empty() {
        bail!("Agent id cannot be empty");
    }
    if slugify(&spec.id).is_empty() {
        bail!("Agent id must contain at least one alphanumeric character");
    }
    if spec.name.trim().is_empty() {
        bail!("Agent name cannot be empty");
    }
    if spec.instructions.trim().is_empty() {
        bail!("Agent instructions cannot be empty");
    }
    Ok(())
}

fn validate_workflow(spec: &WorkflowSpec) -> Result<()> {
    if spec.id.trim().is_empty() {
        bail!("Workflow id cannot be empty");
    }
    if slugify(&spec.id).is_empty() {
        bail!("Workflow id must contain at least one alphanumeric character");
    }
    if spec.name.trim().is_empty() {
        bail!("Workflow name cannot be empty");
    }
    if spec.steps.is_empty() {
        bail!("Workflow must contain at least one step");
    }
    let step_ids: BTreeSet<&str> = spec.steps.iter().map(|step| step.id.trim()).collect();
    if step_ids.len() != spec.steps.len() {
        bail!("Workflow step ids must be unique");
    }
    for step in &spec.steps {
        if step.id.trim().is_empty() || step.name.trim().is_empty() || step.prompt.trim().is_empty()
        {
            bail!("Workflow steps require non-empty id, name, and prompt");
        }
        if step
            .agent_id
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("Workflow step '{}' has an empty agent id", step.id);
        }
        if step
            .worker_preset
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("Workflow step '{}' has an empty worker preset", step.id);
        }
        if step.agent_id.is_some() && step.worker_preset.is_some() {
            bail!(
                "Workflow step '{}' cannot define both `agent_id` and `worker_preset`",
                step.id
            );
        }
        if let Some(worker_preset) = step.worker_preset.as_deref() {
            worker_presets::worker_preset(worker_preset)?;
        }
        for branch in &step.branches {
            if branch.contains.trim().is_empty() {
                bail!(
                    "Workflow step '{}' contains a branch rule with an empty match string",
                    step.id
                );
            }
            if branch.next_step_id.trim().is_empty() {
                bail!(
                    "Workflow step '{}' contains a branch rule with an empty target step id",
                    step.id
                );
            }
            if !step_ids.contains(branch.next_step_id.trim()) {
                bail!(
                    "Workflow step '{}' branches to unknown step '{}'",
                    step.id,
                    branch.next_step_id
                );
            }
        }
    }
    for binding in &spec.channels {
        if binding.kind.trim().is_empty() {
            bail!("Workflow channel bindings require a non-empty kind");
        }
    }
    for binding in &spec.webhooks {
        if binding.id.trim().is_empty() {
            bail!("Workflow webhook bindings require a non-empty id");
        }
    }
    for binding in &spec.file_watches {
        if binding.path.trim().is_empty() {
            bail!("Workflow file-watch bindings require a non-empty path");
        }
        for event in &binding.events {
            match event.trim().to_ascii_lowercase().as_str() {
                "any" | "create" | "modify" | "remove" => {}
                other => {
                    bail!(
                        "Workflow file-watch binding '{}' has unsupported event '{}'",
                        binding.path,
                        other
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{slugify, SpecRepository};
    use sdk::{WorkflowBranchSpec, WorkflowSpec, WorkflowStepSpec};
    use tempfile::TempDir;

    #[test]
    fn slugify_normalizes_ids() {
        assert_eq!(slugify("Default Assistant"), "default-assistant");
        assert_eq!(slugify("  Release_Manager  "), "release-manager");
    }

    #[test]
    fn repository_seeds_default_agent() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();
        let agents = repo.list_agents().unwrap();
        assert!(agents.iter().any(|agent| agent.id == "default-assistant"));
    }

    #[test]
    fn repository_rejects_step_with_agent_and_worker_preset() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let workflow = WorkflowSpec {
            id: "bad-workflow".to_string(),
            name: "Bad Workflow".to_string(),
            steps: vec![WorkflowStepSpec {
                id: "step-1".to_string(),
                name: "Conflicted".to_string(),
                prompt: "Do the thing".to_string(),
                agent_id: Some("default-assistant".to_string()),
                worker_preset: Some("executor".to_string()),
                outcome_contract: None,
                continue_on_error: false,
                branches: Vec::new(),
            }],
            ..WorkflowSpec::default()
        };

        let error = repo
            .save_workflow(&workflow)
            .expect_err("workflow should be rejected");
        assert!(error
            .to_string()
            .contains("cannot define both `agent_id` and `worker_preset`"));
    }

    #[test]
    fn repository_rejects_branch_to_unknown_step() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("ROVE_CONFIG_PATH", temp_dir.path().join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let workflow = WorkflowSpec {
            id: "branchy".to_string(),
            name: "Branchy".to_string(),
            steps: vec![WorkflowStepSpec {
                id: "step-1".to_string(),
                name: "Branch".to_string(),
                prompt: "Do the thing".to_string(),
                agent_id: None,
                worker_preset: None,
                outcome_contract: None,
                continue_on_error: false,
                branches: vec![WorkflowBranchSpec {
                    contains: "retry".to_string(),
                    next_step_id: "step-9".to_string(),
                }],
            }],
            ..WorkflowSpec::default()
        };

        let error = repo
            .save_workflow(&workflow)
            .expect_err("workflow should be rejected");
        assert!(error.to_string().contains("branches to unknown step"));
    }
}
