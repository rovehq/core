use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use dashmap::DashMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::config::{ApprovalMode, Config};
use sdk::TaskSource;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub task_id: String,
    pub tool_name: String,
    pub risk_tier: u8,
    pub summary: String,
    pub created_at: i64,
    pub auto_resolve_after_secs: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRuleAction {
    Allow,
    RequireApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRule {
    pub id: String,
    pub action: ApprovalRuleAction,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub risk_tier: Option<u8>,
    #[serde(default)]
    pub effect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApprovalRulesFile {
    #[serde(default)]
    pub rules: Vec<ApprovalRule>,
}

#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    AutoAllow { reason: String },
    RequireApproval { reason: Option<String> },
}

type ApprovalWaiter = oneshot::Sender<bool>;

fn pending_approvals() -> &'static DashMap<String, ApprovalRequest> {
    static MAP: OnceLock<DashMap<String, ApprovalRequest>> = OnceLock::new();
    MAP.get_or_init(DashMap::new)
}

fn approval_waiters() -> &'static DashMap<String, ApprovalWaiter> {
    static MAP: OnceLock<DashMap<String, ApprovalWaiter>> = OnceLock::new();
    MAP.get_or_init(DashMap::new)
}

pub fn current_mode(config: &Config) -> ApprovalMode {
    config.approvals.mode
}

pub fn rules_path(config: &Config) -> Result<PathBuf> {
    if let Some(path) = config.approvals.rules_path.clone() {
        return Ok(path);
    }

    let root = if let Some(config_path) =
        std::env::var_os("ROVE_CONFIG_PATH").filter(|value| !value.is_empty())
    {
        PathBuf::from(config_path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        config
            .core
            .data_dir
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| config.core.data_dir.clone())
    };
    Ok(root.join("approvals").join("rules.toml"))
}

pub fn load_rules(config: &Config) -> Result<ApprovalRulesFile> {
    let path = rules_path(config)?;
    if !path.exists() {
        return Ok(ApprovalRulesFile::default());
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

pub fn save_rules(config: &Config, file: &ApprovalRulesFile) -> Result<()> {
    let path = rules_path(config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(&path, toml::to_string_pretty(file)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to chmod {}", path.display()))?;
    }
    Ok(())
}

pub fn add_rule(config: &Config, rule: ApprovalRule) -> Result<ApprovalRulesFile> {
    let mut file = load_rules(config)?;
    file.rules.retain(|existing| existing.id != rule.id);
    file.rules.push(rule);
    file.rules.sort_by(|left, right| left.id.cmp(&right.id));
    save_rules(config, &file)?;
    Ok(file)
}

pub fn remove_rule(config: &Config, id: &str) -> Result<bool> {
    let mut file = load_rules(config)?;
    let original = file.rules.len();
    file.rules.retain(|rule| rule.id != id);
    if file.rules.len() == original {
        return Ok(false);
    }
    save_rules(config, &file)?;
    Ok(true)
}

pub fn evaluate(
    config: &Config,
    tool_name: &str,
    args: &Value,
    source: &TaskSource,
    risk_tier: u8,
) -> Result<ApprovalDecision> {
    let mode = current_mode(config);
    if matches!(mode, ApprovalMode::Open) {
        return Ok(ApprovalDecision::AutoAllow {
            reason: "approval mode is set to open".to_string(),
        });
    }

    if matches!(mode, ApprovalMode::Allowlist) {
        let rules = load_rules(config)?;
        if let Some(rule) = rules
            .rules
            .iter()
            .find(|rule| rule_matches(rule, tool_name, args, source, risk_tier))
        {
            return Ok(match rule.action {
                ApprovalRuleAction::Allow => ApprovalDecision::AutoAllow {
                    reason: format!("approval allowlist matched rule '{}'", rule.id),
                },
                ApprovalRuleAction::RequireApproval => ApprovalDecision::RequireApproval {
                    reason: Some(format!("approval allowlist matched rule '{}'", rule.id)),
                },
            });
        }
    }

    Ok(ApprovalDecision::RequireApproval { reason: None })
}

pub async fn request_approval(
    task_id: &str,
    tool_name: &str,
    risk_tier: u8,
    summary: impl Into<String>,
    timeout: Option<Duration>,
    default_on_timeout: bool,
) -> bool {
    let id = Uuid::new_v4().to_string();
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default();

    let approval = ApprovalRequest {
        id: id.clone(),
        task_id: task_id.to_string(),
        tool_name: tool_name.to_string(),
        risk_tier,
        summary: summary.into(),
        created_at,
        auto_resolve_after_secs: timeout.map(|value| value.as_secs()),
    };

    let (tx, rx) = oneshot::channel();
    pending_approvals().insert(id.clone(), approval);
    approval_waiters().insert(id.clone(), tx);

    let resolved = match timeout {
        Some(timeout) => tokio::time::timeout(timeout, rx)
            .await
            .ok()
            .and_then(|outcome| outcome.ok()),
        None => rx.await.ok(),
    };

    pending_approvals().remove(&id);
    approval_waiters().remove(&id);
    resolved.unwrap_or(default_on_timeout)
}

pub fn list_pending() -> Vec<ApprovalRequest> {
    let mut values = pending_approvals()
        .iter()
        .map(|entry| entry.value().clone())
        .collect::<Vec<_>>();
    values.sort_by_key(|approval| approval.created_at);
    values
}

pub fn resolve(id: &str, approved: bool) -> bool {
    let waiter = approval_waiters().remove(id).map(|(_, waiter)| waiter);
    pending_approvals().remove(id);
    if let Some(waiter) = waiter {
        let _ = waiter.send(approved);
        true
    } else {
        false
    }
}

fn rule_matches(
    rule: &ApprovalRule,
    tool_name: &str,
    args: &Value,
    source: &TaskSource,
    risk_tier: u8,
) -> bool {
    if let Some(expected_tool) = rule.tool.as_deref() {
        if !pattern_matches(expected_tool, tool_name) {
            return false;
        }
    }

    if let Some(expected_tier) = rule.risk_tier {
        if expected_tier != risk_tier {
            return false;
        }
    }

    if let Some(effect) = rule.effect.as_deref() {
        if !effect.eq_ignore_ascii_case(&effect_for_request(tool_name, args)) {
            return false;
        }
    }

    if !rule.commands.is_empty() {
        let Some(command) = extract_command(args) else {
            return false;
        };
        if !rule
            .commands
            .iter()
            .any(|pattern| pattern_matches(pattern, &command))
        {
            return false;
        }
    }

    if !rule.paths.is_empty() {
        let paths = extract_string_values(args);
        if !rule.paths.iter().any(|pattern| {
            paths
                .iter()
                .any(|path| path_looks_like_path(path) && pattern_matches(pattern, path))
        }) {
            return false;
        }
    }

    if !rule.nodes.is_empty() {
        let Some(node) = (match source {
            TaskSource::Remote(node) => Some(node.as_str()),
            _ => None,
        }) else {
            return false;
        };
        if !rule
            .nodes
            .iter()
            .any(|pattern| pattern_matches(pattern, node))
        {
            return false;
        }
    }

    if !rule.channels.is_empty() {
        let actual = match source {
            TaskSource::Telegram(user_id) => format!("telegram:{}", user_id),
            TaskSource::Cli => "cli".to_string(),
            TaskSource::WebUI => "webui".to_string(),
            TaskSource::Remote(node) => format!("remote:{}", node),
        };
        if !rule
            .channels
            .iter()
            .any(|pattern| pattern_matches(pattern, &actual))
        {
            return false;
        }
    }

    true
}

fn effect_for_request(tool_name: &str, args: &Value) -> String {
    match tool_name {
        "read_file" | "list_dir" | "file_exists" | "search_files" | "screenshot" => {
            "read_only".to_string()
        }
        "write_file" | "delete_file" | "rename_file" => "mutating".to_string(),
        "run_command" => {
            let command = extract_command(args).unwrap_or_default();
            if command.starts_with("git status")
                || command.starts_with("git diff")
                || command.starts_with("git log")
                || command.starts_with("ls")
                || command.starts_with("pwd")
                || command.starts_with("cat ")
                || command.starts_with("rg ")
            {
                "read_only".to_string()
            } else {
                "execute".to_string()
            }
        }
        _ => "execute".to_string(),
    }
}

fn extract_command(args: &Value) -> Option<String> {
    match args {
        Value::Object(map) => map
            .get("command")
            .and_then(Value::as_str)
            .or_else(|| map.get("cmd").and_then(Value::as_str))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        _ => None,
    }
}

fn extract_string_values(args: &Value) -> Vec<String> {
    let mut values = Vec::new();
    collect_strings(args, &mut values);
    values
}

fn collect_strings(value: &Value, values: &mut Vec<String>) {
    match value {
        Value::String(text) => values.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, values);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_strings(value, values);
            }
        }
        _ => {}
    }
}

fn path_looks_like_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
        || value.contains(std::path::MAIN_SEPARATOR)
}

fn pattern_matches(pattern: &str, actual: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern.eq_ignore_ascii_case(actual);
    }

    let regex = glob_to_regex(pattern);
    Regex::new(&regex)
        .map(|regex| regex.is_match(actual))
        .unwrap_or(false)
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '.' => regex.push_str("\\."),
            '?' => regex.push('.'),
            '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }
    regex.push('$');
    regex
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn temp_config() -> (TempDir, Config) {
        let temp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.core.workspace = temp.path().join("workspace");
        config.core.data_dir = temp.path().join("data");
        fs::create_dir_all(&config.core.workspace).expect("workspace");
        fs::create_dir_all(&config.core.data_dir).expect("data");
        (temp, config)
    }

    #[tokio::test]
    async fn request_approval_tracks_pending_and_resolves() {
        let approval_task = tokio::spawn(async {
            request_approval(
                "task-1",
                "write_file",
                2,
                "Allow write_file for task-1",
                None,
                false,
            )
            .await
        });

        let approval = loop {
            if let Some(approval) = list_pending()
                .into_iter()
                .find(|value| value.task_id == "task-1")
            {
                break approval;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };

        assert_eq!(approval.tool_name, "write_file");
        assert!(resolve(&approval.id, true));
        assert!(approval_task.await.expect("approval task"));
    }

    #[test]
    fn allowlist_rule_matches_command_and_effect() {
        let rule = ApprovalRule {
            id: "git-safe".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("run_command".to_string()),
            commands: vec!["git status*".to_string()],
            paths: Vec::new(),
            nodes: Vec::new(),
            channels: Vec::new(),
            risk_tier: Some(1),
            effect: Some("read_only".to_string()),
        };
        assert!(rule_matches(
            &rule,
            "run_command",
            &serde_json::json!({ "command": "git status --short" }),
            &TaskSource::Cli,
            1,
        ));
    }

    #[test]
    fn open_mode_auto_allows() {
        let (_temp, mut config) = temp_config();
        config.approvals.mode = ApprovalMode::Open;
        let decision = evaluate(
            &config,
            "write_file",
            &serde_json::json!({ "path": "/tmp/demo.txt" }),
            &TaskSource::Cli,
            2,
        )
        .expect("evaluate");
        assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
    }

    #[test]
    fn allowlist_mode_uses_rules_file() {
        let (_temp, mut config) = temp_config();
        config.approvals.mode = ApprovalMode::Allowlist;
        add_rule(
            &config,
            ApprovalRule {
                id: "office-read".to_string(),
                action: ApprovalRuleAction::Allow,
                tool: Some("read_file".to_string()),
                commands: Vec::new(),
                paths: vec!["/workspace/**".to_string()],
                nodes: Vec::new(),
                channels: Vec::new(),
                risk_tier: Some(1),
                effect: Some("read_only".to_string()),
            },
        )
        .expect("add rule");

        let decision = evaluate(
            &config,
            "read_file",
            &serde_json::json!({ "path": "/workspace/demo.txt" }),
            &TaskSource::Cli,
            1,
        )
        .expect("evaluate");
        assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
    }
}
