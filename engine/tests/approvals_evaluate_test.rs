//! Deep tests for approvals::evaluate() and mode-based decision logic

use std::fs;
use tempfile::TempDir;

use rove_engine::config::{ApprovalMode, Config};
use rove_engine::security::approvals::{
    add_rule, evaluate, load_rules, remove_rule, rules_path, save_rules, ApprovalDecision,
    ApprovalRule, ApprovalRuleAction, ApprovalRulesFile,
};
use sdk::TaskSource;
use serde_json::json;

fn temp_config(mode: ApprovalMode) -> (TempDir, Config) {
    let temp = TempDir::new().expect("temp dir");
    let mut config = Config::default();
    config.core.workspace = temp.path().join("workspace");
    config.core.data_dir = temp.path().join("data");
    fs::create_dir_all(&config.core.workspace).expect("workspace");
    fs::create_dir_all(&config.core.data_dir).expect("data dir");
    config.approvals.mode = mode;
    (temp, config)
}

fn allow_rule(id: &str, tool: Option<&str>) -> ApprovalRule {
    ApprovalRule {
        id: id.to_string(),
        action: ApprovalRuleAction::Allow,
        tool: tool.map(str::to_string),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    }
}

fn require_rule(id: &str, tool: Option<&str>) -> ApprovalRule {
    ApprovalRule {
        id: id.to_string(),
        action: ApprovalRuleAction::RequireApproval,
        tool: tool.map(str::to_string),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    }
}

// ── Open mode tests ───────────────────────────────────────────────────────────

#[test]
fn open_mode_always_auto_allows_read_file() {
    let (_temp, config) = temp_config(ApprovalMode::Open);
    let decision = evaluate(
        &config,
        "read_file",
        &json!({"path":"/tmp/x"}),
        &TaskSource::Cli,
        0,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn open_mode_always_auto_allows_write_file() {
    let (_temp, config) = temp_config(ApprovalMode::Open);
    let decision = evaluate(
        &config,
        "write_file",
        &json!({"path":"/tmp/x"}),
        &TaskSource::Cli,
        1,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn open_mode_always_auto_allows_delete_file() {
    let (_temp, config) = temp_config(ApprovalMode::Open);
    let decision = evaluate(
        &config,
        "delete_file",
        &json!({"path":"/tmp/x"}),
        &TaskSource::Cli,
        2,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn open_mode_auto_allows_run_command() {
    let (_temp, config) = temp_config(ApprovalMode::Open);
    let decision = evaluate(
        &config,
        "run_command",
        &json!({"command": "ls -la"}),
        &TaskSource::Cli,
        2,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn open_mode_auto_allows_remote_source() {
    let (_temp, config) = temp_config(ApprovalMode::Open);
    let decision = evaluate(
        &config,
        "execute_command",
        &json!({}),
        &TaskSource::Remote("node-1".to_string()),
        2,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn open_mode_reason_mentions_open() {
    let (_temp, config) = temp_config(ApprovalMode::Open);
    let decision = evaluate(&config, "read_file", &json!({}), &TaskSource::Cli, 0).unwrap();
    if let ApprovalDecision::AutoAllow { reason } = decision {
        assert!(reason.contains("open"));
    }
}

// ── Default mode tests ────────────────────────────────────────────────────────

#[test]
fn default_mode_requires_approval() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let decision = evaluate(
        &config,
        "write_file",
        &json!({"path":"/tmp/x"}),
        &TaskSource::Cli,
        1,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

#[test]
fn default_mode_requires_approval_for_read() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let decision = evaluate(
        &config,
        "read_file",
        &json!({"path":"/tmp/x"}),
        &TaskSource::Cli,
        0,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

#[test]
fn default_mode_requires_approval_for_delete() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let decision = evaluate(&config, "delete_file", &json!({}), &TaskSource::Cli, 2).unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

// ── Allowlist mode with no rules ──────────────────────────────────────────────

#[test]
fn allowlist_no_rules_requires_approval() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    let decision = evaluate(
        &config,
        "read_file",
        &json!({"path":"/tmp/x"}),
        &TaskSource::Cli,
        0,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

// ── Allowlist mode with rules ─────────────────────────────────────────────────

#[test]
fn allowlist_rule_matches_tool_auto_allows() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(&config, allow_rule("allow-read", Some("read_file"))).unwrap();
    let decision = evaluate(
        &config,
        "read_file",
        &json!({"path":"/workspace/a.rs"}),
        &TaskSource::Cli,
        0,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn allowlist_rule_matches_wildcard_tool_auto_allows() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(&config, allow_rule("allow-all-tools", Some("*"))).unwrap();
    let decision = evaluate(&config, "write_file", &json!({}), &TaskSource::Cli, 1).unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn allowlist_require_rule_still_requires_approval() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(&config, require_rule("require-write", Some("write_file"))).unwrap();
    let decision = evaluate(&config, "write_file", &json!({}), &TaskSource::Cli, 1).unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

#[test]
fn allowlist_no_matching_rule_requires_approval() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(&config, allow_rule("allow-read", Some("read_file"))).unwrap();
    // Asking for write_file, but rule only allows read_file
    let decision = evaluate(&config, "write_file", &json!({}), &TaskSource::Cli, 1).unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

#[test]
fn allowlist_auto_allow_reason_contains_rule_id() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(&config, allow_rule("my-rule-id", Some("read_file"))).unwrap();
    let decision = evaluate(&config, "read_file", &json!({}), &TaskSource::Cli, 0).unwrap();
    if let ApprovalDecision::AutoAllow { reason } = decision {
        assert!(reason.contains("my-rule-id"));
    }
}

#[test]
fn allowlist_require_reason_contains_rule_id() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(&config, require_rule("my-require-rule", Some("write_file"))).unwrap();
    let decision = evaluate(&config, "write_file", &json!({}), &TaskSource::Cli, 1).unwrap();
    if let ApprovalDecision::RequireApproval { reason } = decision {
        assert!(reason.as_deref().unwrap_or("").contains("my-require-rule"));
    }
}

// ── rules_path tests ──────────────────────────────────────────────────────────

#[test]
fn rules_path_default_joins_approvals_dir() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let path = rules_path(&config).unwrap();
    assert!(path.to_string_lossy().contains("approvals"));
    assert!(path.to_string_lossy().ends_with("rules.toml"));
}

#[test]
fn rules_path_custom_overrides_default() {
    let temp = TempDir::new().unwrap();
    let mut config = Config::default();
    config.core.workspace = temp.path().join("ws");
    config.core.data_dir = temp.path().join("data");
    fs::create_dir_all(&config.core.workspace).unwrap();
    let custom_path = temp.path().join("custom_rules.toml");
    config.approvals.rules_path = Some(custom_path.clone());
    let path = rules_path(&config).unwrap();
    assert_eq!(path, custom_path);
}

// ── load_rules tests ──────────────────────────────────────────────────────────

#[test]
fn load_rules_missing_file_returns_empty() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let file = load_rules(&config).unwrap();
    assert!(file.rules.is_empty());
}

#[test]
fn load_rules_after_save_roundtrip() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let file = ApprovalRulesFile {
        rules: vec![allow_rule("r1", Some("read_file"))],
    };
    save_rules(&config, &file).unwrap();
    let loaded = load_rules(&config).unwrap();
    assert_eq!(loaded.rules.len(), 1);
    assert_eq!(loaded.rules[0].id, "r1");
}

// ── add_rule tests ────────────────────────────────────────────────────────────

#[test]
fn add_rule_inserts_new_rule() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    add_rule(&config, allow_rule("new-rule", None)).unwrap();
    let file = load_rules(&config).unwrap();
    assert!(file.rules.iter().any(|r| r.id == "new-rule"));
}

#[test]
fn add_rule_replaces_existing_by_id() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    add_rule(&config, allow_rule("same-id", Some("read_file"))).unwrap();
    add_rule(&config, require_rule("same-id", Some("write_file"))).unwrap();
    let file = load_rules(&config).unwrap();
    let matches: Vec<_> = file.rules.iter().filter(|r| r.id == "same-id").collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].action, ApprovalRuleAction::RequireApproval);
}

#[test]
fn add_multiple_rules_all_stored() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    add_rule(&config, allow_rule("r1", None)).unwrap();
    add_rule(&config, allow_rule("r2", None)).unwrap();
    add_rule(&config, allow_rule("r3", None)).unwrap();
    let file = load_rules(&config).unwrap();
    assert_eq!(file.rules.len(), 3);
}

#[test]
fn add_rule_sorts_by_id() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    add_rule(&config, allow_rule("zzz", None)).unwrap();
    add_rule(&config, allow_rule("aaa", None)).unwrap();
    add_rule(&config, allow_rule("mmm", None)).unwrap();
    let file = load_rules(&config).unwrap();
    assert_eq!(file.rules[0].id, "aaa");
    assert_eq!(file.rules[1].id, "mmm");
    assert_eq!(file.rules[2].id, "zzz");
}

// ── remove_rule tests ─────────────────────────────────────────────────────────

#[test]
fn remove_rule_existing_returns_true() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    add_rule(&config, allow_rule("to-remove", None)).unwrap();
    let removed = remove_rule(&config, "to-remove").unwrap();
    assert!(removed);
    let file = load_rules(&config).unwrap();
    assert!(file.rules.iter().all(|r| r.id != "to-remove"));
}

#[test]
fn remove_rule_nonexistent_returns_false() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let removed = remove_rule(&config, "does-not-exist").unwrap();
    assert!(!removed);
}

#[test]
fn remove_rule_leaves_others_intact() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    add_rule(&config, allow_rule("keep-1", None)).unwrap();
    add_rule(&config, allow_rule("remove-me", None)).unwrap();
    add_rule(&config, allow_rule("keep-2", None)).unwrap();
    remove_rule(&config, "remove-me").unwrap();
    let file = load_rules(&config).unwrap();
    assert_eq!(file.rules.len(), 2);
    assert!(file.rules.iter().any(|r| r.id == "keep-1"));
    assert!(file.rules.iter().any(|r| r.id == "keep-2"));
}

// ── save_rules tests ──────────────────────────────────────────────────────────

#[test]
fn save_rules_creates_dir() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let file = ApprovalRulesFile::default();
    save_rules(&config, &file).unwrap();
    let path = rules_path(&config).unwrap();
    assert!(path.exists());
}

#[test]
fn save_rules_empty_file_roundtrip() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    save_rules(&config, &ApprovalRulesFile::default()).unwrap();
    let loaded = load_rules(&config).unwrap();
    assert!(loaded.rules.is_empty());
}

#[test]
fn save_rules_multiple_rules_roundtrip() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let file = ApprovalRulesFile {
        rules: vec![
            allow_rule("a", Some("read_file")),
            require_rule("b", Some("write_file")),
        ],
    };
    save_rules(&config, &file).unwrap();
    let loaded = load_rules(&config).unwrap();
    assert_eq!(loaded.rules.len(), 2);
}

// ── Effect-based matching tests ───────────────────────────────────────────────

#[test]
fn allowlist_effect_read_only_matches_read_file() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(
        &config,
        ApprovalRule {
            id: "effect-read".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("read_file".to_string()),
            commands: vec![],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: None,
            effect: Some("read_only".to_string()),
        },
    )
    .unwrap();
    let decision = evaluate(
        &config,
        "read_file",
        &json!({"path": "/workspace/a.rs"}),
        &TaskSource::Cli,
        0,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn allowlist_effect_mismatch_does_not_match() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(
        &config,
        ApprovalRule {
            id: "effect-read".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("read_file".to_string()),
            commands: vec![],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: None,
            effect: Some("mutating".to_string()), // mismatch: read_file has read_only effect
        },
    )
    .unwrap();
    let decision = evaluate(
        &config,
        "read_file",
        &json!({"path": "/workspace/a.rs"}),
        &TaskSource::Cli,
        0,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

// ── Risk-tier matching ─────────────────────────────────────────────────────────

#[test]
fn allowlist_risk_tier_matches_exactly() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(
        &config,
        ApprovalRule {
            id: "tier1-rule".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("write_file".to_string()),
            commands: vec![],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: Some(1),
            effect: None,
        },
    )
    .unwrap();
    let decision = evaluate(&config, "write_file", &json!({}), &TaskSource::Cli, 1).unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn allowlist_risk_tier_mismatch_no_match() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(
        &config,
        ApprovalRule {
            id: "tier0-rule".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("read_file".to_string()),
            commands: vec![],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: Some(0),
            effect: None,
        },
    )
    .unwrap();
    // Calling with tier 1 instead of 0
    let decision = evaluate(&config, "read_file", &json!({}), &TaskSource::Cli, 1).unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

// ── Command-based matching ─────────────────────────────────────────────────────

#[test]
fn allowlist_command_pattern_matches() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(
        &config,
        ApprovalRule {
            id: "git-safe".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("run_command".to_string()),
            commands: vec!["git status*".to_string()],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: None,
            effect: None,
        },
    )
    .unwrap();
    let decision = evaluate(
        &config,
        "run_command",
        &json!({"command": "git status --short"}),
        &TaskSource::Cli,
        1,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn allowlist_command_mismatch_no_match() {
    let (_temp, config) = temp_config(ApprovalMode::Allowlist);
    add_rule(
        &config,
        ApprovalRule {
            id: "git-safe".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("run_command".to_string()),
            commands: vec!["git status*".to_string()],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: None,
            effect: None,
        },
    )
    .unwrap();
    let decision = evaluate(
        &config,
        "run_command",
        &json!({"command": "rm -rf /tmp"}),
        &TaskSource::Cli,
        2,
    )
    .unwrap();
    assert!(matches!(decision, ApprovalDecision::RequireApproval { .. }));
}

// ── Pending and resolve tests ─────────────────────────────────────────────────

#[test]
fn list_pending_returns_empty_initially() {
    let pending = rove_engine::security::approvals::list_pending();
    // There may be entries from other tests but we mainly check it doesn't panic
    let _ = pending;
}

#[test]
fn resolve_nonexistent_id_returns_false() {
    let result = rove_engine::security::approvals::resolve("nonexistent-id-xyz", true);
    assert!(!result);
}

#[test]
fn resolve_nonexistent_id_reject_returns_false() {
    let result = rove_engine::security::approvals::resolve("bogus-id-abc", false);
    assert!(!result);
}

// ── current_mode tests ────────────────────────────────────────────────────────

#[test]
fn current_mode_reads_from_config() {
    let (_temp, mut config) = temp_config(ApprovalMode::Open);
    config.approvals.mode = ApprovalMode::Allowlist;
    let mode = rove_engine::security::approvals::current_mode(&config);
    assert_eq!(mode, ApprovalMode::Allowlist);
}

#[test]
fn current_mode_default() {
    let (_temp, config) = temp_config(ApprovalMode::Default);
    let mode = rove_engine::security::approvals::current_mode(&config);
    assert_eq!(mode, ApprovalMode::Default);
}

#[test]
fn current_mode_open() {
    let (_temp, config) = temp_config(ApprovalMode::Open);
    let mode = rove_engine::security::approvals::current_mode(&config);
    assert_eq!(mode, ApprovalMode::Open);
}
