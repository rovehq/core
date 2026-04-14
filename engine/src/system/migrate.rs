use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use sdk::{AgentSpec, ChannelBinding, SpecProvenance, WorkflowSpec, WorkflowStepSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::specs::{slugify, SpecRepository};

const MAX_SCAN_FILES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationSource {
    OpenClaw,
    ZeroClaw,
    Moltis,
}

impl MigrationSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenClaw => "openclaw",
            Self::ZeroClaw => "zeroclaw",
            Self::Moltis => "moltis",
        }
    }

    pub fn default_root(&self) -> Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        Ok(match self {
            Self::OpenClaw => home.join(".openclaw"),
            Self::ZeroClaw => home.join(".zeroclaw"),
            Self::Moltis => home.join(".moltis"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationArtifact {
    pub kind: String,
    pub path: String,
    pub supported: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub source: String,
    pub root: String,
    pub exists: bool,
    pub config_files: Vec<String>,
    pub agent_candidates: Vec<MigrationArtifact>,
    pub workflow_candidates: Vec<MigrationArtifact>,
    pub detected_channels: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationImportResult {
    pub report: MigrationReport,
    pub imported_agents: Vec<String>,
    pub imported_workflows: Vec<String>,
    pub warnings: Vec<String>,
    pub dry_run: bool,
}

pub fn inspect(source: MigrationSource, root_override: Option<&Path>) -> Result<MigrationReport> {
    let root = root_override
        .map(PathBuf::from)
        .unwrap_or(source.default_root()?);
    let mut warnings = Vec::new();

    if !root.exists() {
        warnings.push(format!(
            "No {} installation was found at {}.",
            source.as_str(),
            root.display()
        ));
        return Ok(MigrationReport {
            source: source.as_str().to_string(),
            root: root.display().to_string(),
            exists: false,
            config_files: Vec::new(),
            agent_candidates: Vec::new(),
            workflow_candidates: Vec::new(),
            detected_channels: Vec::new(),
            warnings,
        });
    }

    let files = scan_candidate_files(&root, &mut warnings)?;
    let mut config_files = Vec::new();
    let mut agent_candidates = Vec::new();
    let mut workflow_candidates = Vec::new();
    let mut detected_channels = BTreeSet::new();

    for path in files {
        match classify_candidate(&path) {
            CandidateKind::Config => {
                config_files.push(path.display().to_string());
                detected_channels.extend(detect_channels_for_path(&path));
            }
            CandidateKind::Agent => {
                detected_channels.extend(detect_channels(&path));
                agent_candidates.push(MigrationArtifact {
                    kind: "agent".to_string(),
                    path: path.display().to_string(),
                    supported: is_supported_import_file(&path),
                    summary: summarize_candidate(&path),
                });
            }
            CandidateKind::Workflow => {
                detected_channels.extend(detect_channels(&path));
                workflow_candidates.push(MigrationArtifact {
                    kind: "workflow".to_string(),
                    path: path.display().to_string(),
                    supported: is_supported_import_file(&path),
                    summary: summarize_candidate(&path),
                });
            }
            CandidateKind::Ignore => {}
        }
    }

    if config_files.is_empty() && agent_candidates.is_empty() && workflow_candidates.is_empty() {
        warnings.push(format!(
            "No importable personas, agents, workflows, or config files were detected under {}.",
            root.display()
        ));
    }

    Ok(MigrationReport {
        source: source.as_str().to_string(),
        root: root.display().to_string(),
        exists: true,
        config_files,
        agent_candidates,
        workflow_candidates,
        detected_channels: detected_channels.into_iter().collect(),
        warnings,
    })
}

pub fn import(
    repo: &SpecRepository,
    source: MigrationSource,
    root_override: Option<&Path>,
    dry_run: bool,
) -> Result<MigrationImportResult> {
    let report = inspect(source, root_override)?;
    if !report.exists {
        bail!("{}", report.warnings.join(" "));
    }

    let root = PathBuf::from(&report.root);
    let mut warnings = report.warnings.clone();
    let mut imported_agents = Vec::new();
    let mut imported_workflows = Vec::new();

    for artifact in &report.agent_candidates {
        let path = PathBuf::from(&artifact.path);
        if dry_run {
            // Simulate: build spec but don't save
            match build_imported_agent_simulated(source, &path) {
                Ok(id) => imported_agents.push(id),
                Err(error) => warnings.push(format!(
                    "Would fail to import agent candidate {}: {}",
                    path.display(),
                    error
                )),
            }
        } else {
            match build_imported_agent(repo, source, &path) {
                Ok(spec) => imported_agents.push(spec.id),
                Err(error) => warnings.push(format!(
                    "Failed to import agent candidate {}: {}",
                    path.display(),
                    error
                )),
            }
        }
    }

    for artifact in &report.workflow_candidates {
        let path = PathBuf::from(&artifact.path);
        if dry_run {
            match build_imported_workflow_simulated(source, &path) {
                Ok(id) => imported_workflows.push(id),
                Err(error) => warnings.push(format!(
                    "Would fail to import workflow candidate {}: {}",
                    path.display(),
                    error
                )),
            }
        } else {
            match build_imported_workflow(repo, source, &path) {
                Ok(spec) => imported_workflows.push(spec.id),
                Err(error) => warnings.push(format!(
                    "Failed to import workflow candidate {}: {}",
                    path.display(),
                    error
                )),
            }
        }
    }

    if !report.config_files.is_empty() {
        warnings.push(format!(
            "Config files from {} were inspected for hints, but live Rove daemon config was not modified automatically.",
            root.display()
        ));
    }

    Ok(MigrationImportResult {
        report,
        imported_agents,
        imported_workflows,
        warnings,
        dry_run,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateKind {
    Config,
    Agent,
    Workflow,
    Ignore,
}

fn scan_candidate_files(root: &Path, warnings: &mut Vec<String>) -> Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(format!("Failed to read {}: {}", dir.display(), error));
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(format!(
                        "Failed to inspect an entry under {}: {}",
                        dir.display(),
                        error
                    ));
                    continue;
                }
            };

            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(kind) => kind,
                Err(error) => {
                    warnings.push(format!("Failed to inspect {}: {}", path.display(), error));
                    continue;
                }
            };

            if file_type.is_dir() {
                if should_skip_dir(&path) {
                    continue;
                }
                stack.push(path);
                continue;
            }

            if file_type.is_file() && is_supported_import_file(&path) {
                files.push(path);
                if files.len() >= MAX_SCAN_FILES {
                    warnings.push(format!(
                        "Scan limit reached at {} files; some source files may not be reported.",
                        MAX_SCAN_FILES
                    ));
                    return Ok(files);
                }
            }
        }
    }

    Ok(files)
}

fn should_skip_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        ".git" | "node_modules" | "target" | "dist" | "build" | ".next" | "__pycache__" | ".venv"
    )
}

fn is_supported_import_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()).map(|value| value.to_ascii_lowercase()),
        Some(ext) if matches!(ext.as_str(), "md" | "txt" | "json" | "toml" | "prompt")
    )
}

fn classify_candidate(path: &Path) -> CandidateKind {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.contains("config") || lower.contains("settings") || lower.contains("preferences") {
        return CandidateKind::Config;
    }
    if lower.contains("workflow")
        || lower.contains("pipeline")
        || lower.contains("runbook")
        || lower.contains("/flows/")
        || lower.contains("/flow/")
    {
        return CandidateKind::Workflow;
    }
    if lower.contains("agent")
        || lower.contains("assistant")
        || lower.contains("persona")
        || lower.contains("soul")
        || lower.contains("prompt")
        || lower.contains("bot")
    {
        return CandidateKind::Agent;
    }
    CandidateKind::Ignore
}

fn summarize_candidate(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_ascii_uppercase();
    format!("{extension} file")
}

fn detect_channels(path: &Path) -> BTreeSet<String> {
    let mut channels = detect_channels_for_path(path);
    if let Ok(raw) = fs::read_to_string(path) {
        let lowered = raw.to_ascii_lowercase();
        for channel in channel_keywords() {
            if lowered.contains(channel) {
                channels.insert(channel.to_string());
            }
        }
    }
    channels
}

fn detect_channels_for_path(path: &Path) -> BTreeSet<String> {
    let lowered = path.to_string_lossy().to_ascii_lowercase();
    let mut channels = BTreeSet::new();
    for channel in channel_keywords() {
        if lowered.contains(channel) {
            channels.insert(channel.to_string());
        }
    }
    channels
}

fn channel_keywords() -> &'static [&'static str] {
    &[
        "telegram", "slack", "discord", "whatsapp", "wechat", "wecom", "lark", "feishu",
    ]
}

fn build_imported_agent(
    repo: &SpecRepository,
    source: MigrationSource,
    path: &Path,
) -> Result<AgentSpec> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let structured = read_structured_value(path, &raw).ok();
    let extracted = extract_agent_content(path, &raw, structured.as_ref());

    let existing_ids = repo
        .list_agents()?
        .into_iter()
        .map(|spec| spec.id)
        .collect::<HashSet<_>>();
    let mut spec = AgentSpec {
        id: unique_id(
            &existing_ids,
            &format!(
                "{}-{}",
                source.as_str(),
                slugify(extracted.name.as_deref().unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("imported-agent")
                }))
            ),
        ),
        name: extracted
            .name
            .unwrap_or_else(|| humanize_file_stem(path, "Imported Agent")),
        purpose: extracted
            .purpose
            .unwrap_or_else(|| format!("Imported {} agent", source.as_str())),
        instructions: extracted.instructions,
        enabled: false,
        channels: extracted.channels,
        tags: dedupe_strings(vec![
            "imported".to_string(),
            format!("source:{}", source.as_str()),
        ]),
        provenance: Some(import_provenance(source, path)),
        ..AgentSpec::default()
    };
    spec.channels = spec
        .channels
        .into_iter()
        .map(|mut binding| {
            binding.enabled = false;
            binding.provenance = Some(import_provenance(source, path));
            binding
        })
        .collect();
    repo.save_agent(&spec)
}

fn build_imported_workflow(
    repo: &SpecRepository,
    source: MigrationSource,
    path: &Path,
) -> Result<WorkflowSpec> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let structured = read_structured_value(path, &raw).ok();
    let extracted = extract_workflow_content(path, &raw, structured.as_ref());

    let existing_ids = repo
        .list_workflows()?
        .into_iter()
        .map(|spec| spec.id)
        .collect::<HashSet<_>>();
    let spec = WorkflowSpec {
        id: unique_id(
            &existing_ids,
            &format!(
                "{}-{}",
                source.as_str(),
                slugify(extracted.name.as_deref().unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("imported-workflow")
                }))
            ),
        ),
        name: extracted
            .name
            .unwrap_or_else(|| humanize_file_stem(path, "Imported Workflow")),
        description: extracted
            .description
            .unwrap_or_else(|| format!("Imported {} workflow", source.as_str())),
        enabled: false,
        steps: extracted.steps,
        tags: dedupe_strings(vec![
            "imported".to_string(),
            format!("source:{}", source.as_str()),
        ]),
        provenance: Some(import_provenance(source, path)),
        ..WorkflowSpec::default()
    };
    repo.save_workflow(&spec)
}

/// Simulate building an imported agent — computes the ID without saving.
fn build_imported_agent_simulated(source: MigrationSource, path: &Path) -> Result<String> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let extracted = extract_agent_content(path, &raw, None);
    let id = format!(
        "{}-{}",
        source.as_str(),
        slugify(extracted.name.as_deref().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("imported-agent")
        }))
    );
    Ok(id)
}

/// Simulate building an imported workflow — computes the ID without saving.
fn build_imported_workflow_simulated(source: MigrationSource, path: &Path) -> Result<String> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let extracted = extract_workflow_content(path, &raw, None);
    let id = format!(
        "{}-{}",
        source.as_str(),
        slugify(extracted.name.as_deref().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("imported-workflow")
        }))
    );
    Ok(id)
}

fn read_structured_value(path: &Path, raw: &str) -> Result<Value> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => serde_json::from_str(raw)
            .with_context(|| format!("Failed to parse JSON from {}", path.display())),
        "toml" => {
            let value: toml::Value = toml::from_str(raw)
                .with_context(|| format!("Failed to parse TOML from {}", path.display()))?;
            serde_json::to_value(value)
                .with_context(|| format!("Failed to convert TOML from {}", path.display()))
        }
        _ => bail!("Structured parsing is not supported for {}", path.display()),
    }
}

struct ExtractedAgentContent {
    name: Option<String>,
    purpose: Option<String>,
    instructions: String,
    channels: Vec<ChannelBinding>,
}

fn extract_agent_content(
    path: &Path,
    raw: &str,
    structured: Option<&Value>,
) -> ExtractedAgentContent {
    let name = structured
        .and_then(|value| string_field(value, &["name", "title", "id"]))
        .or_else(|| markdown_heading(raw))
        .or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(title_case)
        });
    let purpose =
        structured.and_then(|value| string_field(value, &["purpose", "description", "summary"]));
    let instructions = structured
        .and_then(|value| {
            string_field(
                value,
                &[
                    "instructions",
                    "prompt",
                    "system_prompt",
                    "system",
                    "persona",
                    "soul",
                ],
            )
        })
        .unwrap_or_else(|| raw.trim().to_string());
    let channels = detect_channel_bindings(raw, structured);

    ExtractedAgentContent {
        name,
        purpose,
        instructions,
        channels,
    }
}

struct ExtractedWorkflowContent {
    name: Option<String>,
    description: Option<String>,
    steps: Vec<WorkflowStepSpec>,
}

fn extract_workflow_content(
    path: &Path,
    raw: &str,
    structured: Option<&Value>,
) -> ExtractedWorkflowContent {
    let name = structured
        .and_then(|value| string_field(value, &["name", "title", "id"]))
        .or_else(|| markdown_heading(raw))
        .or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(title_case)
        });
    let description =
        structured.and_then(|value| string_field(value, &["description", "summary", "purpose"]));
    let steps = structured
        .and_then(value_steps)
        .filter(|steps| !steps.is_empty())
        .unwrap_or_else(|| {
            vec![WorkflowStepSpec {
                id: "step-1".to_string(),
                name: "Imported Step".to_string(),
                prompt: raw.trim().to_string(),
                agent_id: None,
                worker_preset: None,
                continue_on_error: false,
            }]
        });

    ExtractedWorkflowContent {
        name,
        description,
        steps,
    }
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match value.get(*key) {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.trim().to_string()),
        _ => None,
    })
}

fn value_steps(value: &Value) -> Option<Vec<WorkflowStepSpec>> {
    let steps = value.get("steps")?.as_array()?;
    let mut extracted = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if let Some(prompt) = step
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            extracted.push(WorkflowStepSpec {
                id: format!("step-{}", index + 1),
                name: format!("Step {}", index + 1),
                prompt: prompt.to_string(),
                agent_id: None,
                worker_preset: None,
                continue_on_error: false,
            });
            continue;
        }

        if let Some(prompt) = string_field(step, &["prompt", "instructions", "task"]) {
            extracted.push(WorkflowStepSpec {
                id: string_field(step, &["id"]).unwrap_or_else(|| format!("step-{}", index + 1)),
                name: string_field(step, &["name", "title"])
                    .unwrap_or_else(|| format!("Step {}", index + 1)),
                prompt,
                agent_id: string_field(step, &["agent_id", "agent"]),
                worker_preset: string_field(step, &["worker_preset"]),
                continue_on_error: step
                    .get("continue_on_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
    }
    Some(extracted)
}

fn detect_channel_bindings(raw: &str, structured: Option<&Value>) -> Vec<ChannelBinding> {
    let mut names = BTreeSet::new();
    if let Some(value) = structured {
        if let Some(channels) = value.get("channels").and_then(Value::as_array) {
            for channel in channels {
                match channel {
                    Value::String(name) => {
                        if !name.trim().is_empty() {
                            names.insert(name.trim().to_ascii_lowercase());
                        }
                    }
                    Value::Object(_) => {
                        if let Some(name) = string_field(channel, &["kind", "name", "channel"]) {
                            names.insert(name.to_ascii_lowercase());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let lowered = raw.to_ascii_lowercase();
    for keyword in channel_keywords() {
        if lowered.contains(keyword) {
            names.insert((*keyword).to_string());
        }
    }

    names
        .into_iter()
        .map(|kind| ChannelBinding {
            kind,
            target: None,
            enabled: false,
            provenance: None,
        })
        .collect()
}

fn import_provenance(source: MigrationSource, path: &Path) -> SpecProvenance {
    SpecProvenance {
        source: Some("imported".to_string()),
        import_source: Some(source.as_str().to_string()),
        notes: Some(format!("Imported from {}", path.display())),
        imported_at: Some(Utc::now().timestamp()),
        draft_for: None,
        review_status: None,
        reviewed_at: None,
    }
}

/// Report previously imported specs and their current state.
pub fn migrate_status(repo: &SpecRepository) -> Result<MigrationStatusReport> {
    let sources = [
        MigrationSource::OpenClaw,
        MigrationSource::ZeroClaw,
        MigrationSource::Moltis,
    ];
    let mut per_source: Vec<MigrationSourceStatus> = Vec::new();

    for source in sources {
        let agents = repo
            .list_agents()?
            .into_iter()
            .filter(|a| {
                a.provenance
                    .as_ref()
                    .and_then(|p| p.import_source.as_ref())
                    .map(|s| s == source.as_str())
                    .unwrap_or(false)
            })
            .map(|a| ImportedSpecStatus {
                id: a.id,
                name: a.name,
                kind: "agent".to_string(),
                enabled: a.enabled,
                imported_at: a.provenance.and_then(|p| p.imported_at).unwrap_or(0),
            })
            .collect::<Vec<_>>();

        let workflows = repo
            .list_workflows()?
            .into_iter()
            .filter(|w| {
                w.provenance
                    .as_ref()
                    .and_then(|p| p.import_source.as_ref())
                    .map(|s| s == source.as_str())
                    .unwrap_or(false)
            })
            .map(|w| ImportedSpecStatus {
                id: w.id,
                name: w.name,
                kind: "workflow".to_string(),
                enabled: w.enabled,
                imported_at: w.provenance.and_then(|p| p.imported_at).unwrap_or(0),
            })
            .collect::<Vec<_>>();

        if !agents.is_empty() || !workflows.is_empty() {
            per_source.push(MigrationSourceStatus {
                source: source.as_str().to_string(),
                agents,
                workflows,
            });
        }
    }

    Ok(MigrationStatusReport { per_source })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStatusReport {
    pub per_source: Vec<MigrationSourceStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationSourceStatus {
    pub source: String,
    pub agents: Vec<ImportedSpecStatus>,
    pub workflows: Vec<ImportedSpecStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedSpecStatus {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub imported_at: i64,
}

fn markdown_heading(raw: &str) -> Option<String> {
    raw.lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn humanize_file_stem(path: &Path, fallback: &str) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(title_case)
        .unwrap_or_else(|| fallback.to_string())
}

fn title_case(value: &str) -> String {
    let mut out = Vec::new();
    for part in value.split(['-', '_', ' ']) {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut chars = trimmed.chars();
        let head = chars
            .next()
            .map(|ch| ch.to_ascii_uppercase())
            .unwrap_or_default();
        let tail = chars.as_str().to_ascii_lowercase();
        out.push(format!("{head}{tail}"));
    }
    if out.is_empty() {
        value.to_string()
    } else {
        out.join(" ")
    }
}

fn unique_id(existing: &HashSet<String>, base: &str) -> String {
    let base = slugify(base);
    if base.is_empty() {
        return "imported".to_string();
    }
    if !existing.contains(&base) {
        return base;
    }
    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("loop always returns");
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn inspect_detects_agents_workflows_and_channels() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("agents")).unwrap();
        fs::create_dir_all(root.join("workflows")).unwrap();
        fs::write(
            root.join("agents").join("support-agent.md"),
            "# Support\nHandle Telegram requests",
        )
        .unwrap();
        fs::write(
            root.join("workflows").join("ops-workflow.json"),
            r#"{"name":"Ops workflow","steps":["check status","restart service"]}"#,
        )
        .unwrap();

        let report = inspect(MigrationSource::OpenClaw, Some(root)).unwrap();
        assert!(report.exists);
        assert_eq!(report.agent_candidates.len(), 1);
        assert_eq!(report.workflow_candidates.len(), 1);
        assert!(report.detected_channels.contains(&"telegram".to_string()));
    }

    #[test]
    fn import_creates_disabled_specs_with_provenance() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("source");
        fs::create_dir_all(root.join("agents")).unwrap();
        fs::create_dir_all(root.join("workflows")).unwrap();
        fs::write(
            root.join("agents").join("assistant.md"),
            "# Helper\nRead files safely",
        )
        .unwrap();
        fs::write(
            root.join("workflows").join("flow.toml"),
            r#"name = "Flow"
description = "Imported flow"
steps = ["step one", "step two"]"#,
        )
        .unwrap();

        let config_dir = temp_dir.path().join("config");
        std::env::set_var("ROVE_CONFIG_PATH", config_dir.join("config.toml"));
        let repo = SpecRepository::new().unwrap();

        let result = import(&repo, MigrationSource::OpenClaw, Some(&root), false).unwrap();
        assert_eq!(result.imported_agents.len(), 1);
        assert_eq!(result.imported_workflows.len(), 1);

        let agent = repo
            .load_agent(result.imported_agents.first().unwrap())
            .unwrap();
        assert!(!agent.enabled);
        assert_eq!(
            agent.provenance.and_then(|item| item.import_source),
            Some("openclaw".to_string())
        );

        let workflow = repo
            .load_workflow(result.imported_workflows.first().unwrap())
            .unwrap();
        assert!(!workflow.enabled);
        assert_eq!(
            workflow.provenance.and_then(|item| item.import_source),
            Some("openclaw".to_string())
        );

        std::env::remove_var("ROVE_CONFIG_PATH");
    }
}
