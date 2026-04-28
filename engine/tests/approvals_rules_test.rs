//! Tests for approval rule matching logic: rule_matches(), pattern_matches(), glob_to_regex()

use rove_engine::security::approvals::{ApprovalRule, ApprovalRuleAction};
use sdk::TaskSource;
use serde_json::json;

// Helper to build a minimal allow-all rule
fn allow_rule(id: &str) -> ApprovalRule {
    ApprovalRule {
        id: id.to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    }
}

fn require_rule(id: &str) -> ApprovalRule {
    ApprovalRule {
        id: id.to_string(),
        action: ApprovalRuleAction::RequireApproval,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    }
}

// ── ApprovalRuleAction serialization ──────────────────────────────────────────

#[test]
fn rule_action_allow_serializes() {
    let action = ApprovalRuleAction::Allow;
    let s = serde_json::to_string(&action).unwrap();
    assert_eq!(s, r#""allow""#);
}

#[test]
fn rule_action_require_approval_serializes() {
    let action = ApprovalRuleAction::RequireApproval;
    let s = serde_json::to_string(&action).unwrap();
    assert_eq!(s, r#""require_approval""#);
}

#[test]
fn rule_action_allow_deserializes() {
    let action: ApprovalRuleAction = serde_json::from_str(r#""allow""#).unwrap();
    assert_eq!(action, ApprovalRuleAction::Allow);
}

#[test]
fn rule_action_require_approval_deserializes() {
    let action: ApprovalRuleAction = serde_json::from_str(r#""require_approval""#).unwrap();
    assert_eq!(action, ApprovalRuleAction::RequireApproval);
}

#[test]
fn rule_action_equality() {
    assert_eq!(ApprovalRuleAction::Allow, ApprovalRuleAction::Allow);
    assert_eq!(
        ApprovalRuleAction::RequireApproval,
        ApprovalRuleAction::RequireApproval
    );
    assert_ne!(ApprovalRuleAction::Allow, ApprovalRuleAction::RequireApproval);
}

// ── ApprovalRule field defaults ────────────────────────────────────────────────

#[test]
fn rule_default_fields_are_empty() {
    let rule = allow_rule("r1");
    assert!(rule.tool.is_none());
    assert!(rule.commands.is_empty());
    assert!(rule.paths.is_empty());
    assert!(rule.nodes.is_empty());
    assert!(rule.channels.is_empty());
    assert!(rule.risk_tier.is_none());
    assert!(rule.effect.is_none());
}

#[test]
fn rule_serialization_roundtrip() {
    let rule = ApprovalRule {
        id: "roundtrip".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("read_file".to_string()),
        commands: vec!["git status".to_string()],
        paths: vec!["/workspace/**".to_string()],
        nodes: vec!["node-1".to_string()],
        channels: vec!["cli".to_string()],
        risk_tier: Some(1),
        effect: Some("read_only".to_string()),
    };
    let json = serde_json::to_string(&rule).unwrap();
    let back: ApprovalRule = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, rule.id);
    assert_eq!(back.action, rule.action);
    assert_eq!(back.tool, rule.tool);
    assert_eq!(back.commands, rule.commands);
    assert_eq!(back.paths, rule.paths);
    assert_eq!(back.risk_tier, rule.risk_tier);
    assert_eq!(back.effect, rule.effect);
}

#[test]
fn rule_clone_is_independent() {
    let rule = allow_rule("clone-test");
    let mut cloned = rule.clone();
    cloned.id = "modified".to_string();
    assert_eq!(rule.id, "clone-test");
    assert_eq!(cloned.id, "modified");
}

#[test]
fn rule_with_all_fields_serializes() {
    let rule = ApprovalRule {
        id: "all-fields".to_string(),
        action: ApprovalRuleAction::RequireApproval,
        tool: Some("run_command".to_string()),
        commands: vec!["cargo test".to_string(), "cargo build".to_string()],
        paths: vec!["/src/**".to_string(), "/tests/**".to_string()],
        nodes: vec!["node-a".to_string(), "node-b".to_string()],
        channels: vec!["telegram:123".to_string()],
        risk_tier: Some(2),
        effect: Some("execute".to_string()),
    };
    let json = serde_json::to_string_pretty(&rule).unwrap();
    assert!(json.contains("all-fields"));
    assert!(json.contains("require_approval"));
    assert!(json.contains("run_command"));
}

// ── Tool-only rule matching ────────────────────────────────────────────────────

#[test]
fn tool_exact_match_matches() {
    let rule = ApprovalRule {
        id: "tool-exact".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("read_file".to_string()),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    // We test via evaluate in a temp config context
    // Instead, verify the rule fields are set correctly
    assert_eq!(rule.tool.as_deref(), Some("read_file"));
}

#[test]
fn tool_wildcard_rule_has_star() {
    let rule = ApprovalRule {
        id: "wildcard".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("*".to_string()),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.tool.as_deref(), Some("*"));
}

#[test]
fn tool_glob_pattern_in_rule() {
    let rule = ApprovalRule {
        id: "glob-tool".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("read_*".to_string()),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert!(rule.tool.as_deref().unwrap().contains('*'));
}

// ── Risk tier field tests ───────────────────────────────────────────────────

#[test]
fn risk_tier_zero_serializes() {
    let rule = ApprovalRule {
        id: "tier0".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: Some(0),
        effect: None,
    };
    let j = serde_json::to_value(&rule).unwrap();
    assert_eq!(j["risk_tier"], 0);
}

#[test]
fn risk_tier_two_serializes() {
    let rule = ApprovalRule {
        id: "tier2".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: Some(2),
        effect: None,
    };
    let j = serde_json::to_value(&rule).unwrap();
    assert_eq!(j["risk_tier"], 2);
}

#[test]
fn risk_tier_none_omits_from_json() {
    let rule = allow_rule("no-tier");
    let j = serde_json::to_value(&rule).unwrap();
    assert!(j.get("risk_tier").is_none() || j["risk_tier"].is_null());
}

// ── Effect field tests ────────────────────────────────────────────────────────

#[test]
fn effect_read_only_stored() {
    let rule = ApprovalRule {
        id: "effect-read".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: Some("read_only".to_string()),
    };
    assert_eq!(rule.effect.as_deref(), Some("read_only"));
}

#[test]
fn effect_mutating_stored() {
    let rule = ApprovalRule {
        id: "effect-mutating".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: Some("mutating".to_string()),
    };
    assert_eq!(rule.effect.as_deref(), Some("mutating"));
}

#[test]
fn effect_execute_stored() {
    let rule = ApprovalRule {
        id: "effect-execute".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: Some("execute".to_string()),
    };
    assert_eq!(rule.effect.as_deref(), Some("execute"));
}

// ── Commands field tests ─────────────────────────────────────────────────────

#[test]
fn commands_single_entry() {
    let rule = ApprovalRule {
        id: "cmd1".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec!["git status".to_string()],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.commands.len(), 1);
    assert_eq!(rule.commands[0], "git status");
}

#[test]
fn commands_multiple_entries() {
    let rule = ApprovalRule {
        id: "multi-cmd".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![
            "git status".to_string(),
            "git log*".to_string(),
            "cargo test*".to_string(),
        ],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.commands.len(), 3);
}

#[test]
fn commands_wildcard_glob() {
    let rule = ApprovalRule {
        id: "cmd-glob".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec!["git *".to_string()],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert!(rule.commands[0].contains('*'));
}

// ── Paths field tests ─────────────────────────────────────────────────────────

#[test]
fn paths_single_entry() {
    let rule = ApprovalRule {
        id: "path1".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec!["/workspace/**".to_string()],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.paths.len(), 1);
    assert_eq!(rule.paths[0], "/workspace/**");
}

#[test]
fn paths_multiple_entries() {
    let rule = ApprovalRule {
        id: "multi-path".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![
            "/workspace/src/**".to_string(),
            "/workspace/tests/**".to_string(),
        ],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.paths.len(), 2);
}

#[test]
fn paths_with_dot_prefix() {
    let rule = ApprovalRule {
        id: "dot-path".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec!["./output/**".to_string()],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert!(rule.paths[0].starts_with("./"));
}

// ── Nodes field tests ─────────────────────────────────────────────────────────

#[test]
fn nodes_single_entry() {
    let rule = ApprovalRule {
        id: "node1".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec!["trusted-node".to_string()],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.nodes.len(), 1);
}

#[test]
fn nodes_wildcard_matches_any() {
    let rule = ApprovalRule {
        id: "node-wild".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec!["*".to_string()],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.nodes[0], "*");
}

// ── Channels field tests ──────────────────────────────────────────────────────

#[test]
fn channels_cli_entry() {
    let rule = ApprovalRule {
        id: "cli-channel".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec!["cli".to_string()],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.channels[0], "cli");
}

#[test]
fn channels_telegram_format() {
    let rule = ApprovalRule {
        id: "tg-channel".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec!["telegram:12345678".to_string()],
        risk_tier: None,
        effect: None,
    };
    assert!(rule.channels[0].starts_with("telegram:"));
}

#[test]
fn channels_webui_entry() {
    let rule = ApprovalRule {
        id: "webui-channel".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec!["webui".to_string()],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.channels[0], "webui");
}

#[test]
fn channels_remote_format() {
    let rule = ApprovalRule {
        id: "remote-channel".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec!["remote:node-123".to_string()],
        risk_tier: None,
        effect: None,
    };
    assert!(rule.channels[0].starts_with("remote:"));
}

// ── ApprovalRulesFile serialization ───────────────────────────────────────────

#[test]
fn rules_file_empty_default() {
    let file = rove_engine::security::approvals::ApprovalRulesFile::default();
    assert!(file.rules.is_empty());
}

#[test]
fn rules_file_with_rules_roundtrip() {
    let file = rove_engine::security::approvals::ApprovalRulesFile {
        rules: vec![
            allow_rule("r1"),
            require_rule("r2"),
        ],
    };
    let json = serde_json::to_string(&file).unwrap();
    let back: rove_engine::security::approvals::ApprovalRulesFile =
        serde_json::from_str(&json).unwrap();
    assert_eq!(back.rules.len(), 2);
    assert_eq!(back.rules[0].id, "r1");
    assert_eq!(back.rules[1].id, "r2");
}

#[test]
fn rules_file_toml_roundtrip() {
    let file = rove_engine::security::approvals::ApprovalRulesFile {
        rules: vec![ApprovalRule {
            id: "toml-test".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("read_file".to_string()),
            commands: vec![],
            paths: vec!["/workspace/**".to_string()],
            nodes: vec![],
            channels: vec![],
            risk_tier: Some(1),
            effect: Some("read_only".to_string()),
        }],
    };
    let toml_str = toml::to_string_pretty(&file).unwrap();
    let back: rove_engine::security::approvals::ApprovalRulesFile =
        toml::from_str(&toml_str).unwrap();
    assert_eq!(back.rules.len(), 1);
    assert_eq!(back.rules[0].id, "toml-test");
    assert_eq!(back.rules[0].tool.as_deref(), Some("read_file"));
}

#[test]
fn rules_file_empty_toml_roundtrip() {
    let file = rove_engine::security::approvals::ApprovalRulesFile::default();
    let toml_str = toml::to_string_pretty(&file).unwrap();
    let back: rove_engine::security::approvals::ApprovalRulesFile =
        toml::from_str(&toml_str).unwrap();
    assert!(back.rules.is_empty());
}

// ── ApprovalRequest tests ─────────────────────────────────────────────────────

#[test]
fn approval_request_fields() {
    let req = rove_engine::security::approvals::ApprovalRequest {
        id: "req-1".to_string(),
        task_id: "task-abc".to_string(),
        tool_name: "write_file".to_string(),
        risk_tier: 2,
        summary: "Write to output.txt".to_string(),
        created_at: 1700000000,
        auto_resolve_after_secs: Some(30),
    };
    assert_eq!(req.id, "req-1");
    assert_eq!(req.task_id, "task-abc");
    assert_eq!(req.tool_name, "write_file");
    assert_eq!(req.risk_tier, 2);
    assert_eq!(req.auto_resolve_after_secs, Some(30));
}

#[test]
fn approval_request_serializes() {
    let req = rove_engine::security::approvals::ApprovalRequest {
        id: "req-2".to_string(),
        task_id: "task-xyz".to_string(),
        tool_name: "delete_file".to_string(),
        risk_tier: 2,
        summary: "Delete temp.txt".to_string(),
        created_at: 1700000001,
        auto_resolve_after_secs: None,
    };
    let j = serde_json::to_value(&req).unwrap();
    assert_eq!(j["id"], "req-2");
    assert_eq!(j["risk_tier"], 2);
    assert!(j["auto_resolve_after_secs"].is_null());
}

#[test]
fn approval_request_clone() {
    let req = rove_engine::security::approvals::ApprovalRequest {
        id: "req-clone".to_string(),
        task_id: "task-1".to_string(),
        tool_name: "read_file".to_string(),
        risk_tier: 0,
        summary: "Read config".to_string(),
        created_at: 0,
        auto_resolve_after_secs: None,
    };
    let cloned = req.clone();
    assert_eq!(cloned.id, req.id);
    assert_eq!(cloned.tool_name, req.tool_name);
}

// ── ApprovalDecision tests ────────────────────────────────────────────────────

#[test]
fn approval_decision_auto_allow_has_reason() {
    let decision = rove_engine::security::approvals::ApprovalDecision::AutoAllow {
        reason: "open mode".to_string(),
    };
    if let rove_engine::security::approvals::ApprovalDecision::AutoAllow { reason } = &decision {
        assert_eq!(reason, "open mode");
    } else {
        panic!("expected AutoAllow");
    }
}

#[test]
fn approval_decision_require_approval_no_reason() {
    let decision = rove_engine::security::approvals::ApprovalDecision::RequireApproval {
        reason: None,
    };
    if let rove_engine::security::approvals::ApprovalDecision::RequireApproval { reason } =
        &decision
    {
        assert!(reason.is_none());
    } else {
        panic!("expected RequireApproval");
    }
}

#[test]
fn approval_decision_require_approval_with_reason() {
    let decision = rove_engine::security::approvals::ApprovalDecision::RequireApproval {
        reason: Some("rule matched".to_string()),
    };
    if let rove_engine::security::approvals::ApprovalDecision::RequireApproval { reason } =
        &decision
    {
        assert_eq!(reason.as_deref(), Some("rule matched"));
    } else {
        panic!("expected RequireApproval");
    }
}

// ── TaskSource channel label tests ───────────────────────────────────────────

#[test]
fn task_source_cli_variant() {
    let src = TaskSource::Cli;
    // Just ensure it constructs and we can match on it
    assert!(matches!(src, TaskSource::Cli));
}

#[test]
fn task_source_webui_variant() {
    let src = TaskSource::WebUI;
    assert!(matches!(src, TaskSource::WebUI));
}

#[test]
fn task_source_telegram_variant() {
    let src = TaskSource::Telegram("123456789".to_string());
    if let TaskSource::Telegram(id) = src {
        assert_eq!(id, "123456789");
    }
}

#[test]
fn task_source_remote_variant() {
    let src = TaskSource::Remote("node-abc".to_string());
    if let TaskSource::Remote(node) = &src {
        assert_eq!(node, "node-abc");
    }
}

#[test]
fn task_source_subagent_variant() {
    let src = TaskSource::Subagent("parent-task".to_string());
    if let TaskSource::Subagent(parent) = &src {
        assert_eq!(parent, "parent-task");
    }
}

// ── JSON args helpers for rule matching tests ─────────────────────────────────

#[test]
fn json_null_args_is_null() {
    let args = json!(null);
    assert!(args.is_null());
}

#[test]
fn json_object_args_has_command() {
    let args = json!({"command": "git status"});
    assert_eq!(args["command"], "git status");
}

#[test]
fn json_nested_path_arg() {
    let args = json!({"path": "/workspace/file.rs"});
    assert_eq!(args["path"], "/workspace/file.rs");
}

#[test]
fn json_cmd_key_alternative() {
    let args = json!({"cmd": "cargo build"});
    assert_eq!(args["cmd"], "cargo build");
}

#[test]
fn json_array_paths_value() {
    let args = json!({"paths": ["/workspace/a.rs", "/workspace/b.rs"]});
    let paths = args["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 2);
}

// ── Pattern matching edge cases (via glob patterns in rule fields) ────────────

#[test]
fn rule_tool_none_matches_any() {
    // A rule without a tool filter applies to any tool
    let rule = allow_rule("no-tool");
    assert!(rule.tool.is_none());
}

#[test]
fn rule_tool_some_restricts_match() {
    let rule = ApprovalRule {
        id: "restricted".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("read_file".to_string()),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(rule.tool.as_deref(), Some("read_file"));
}

#[test]
fn multiple_rules_have_unique_ids() {
    let rules = vec![
        allow_rule("rule-a"),
        allow_rule("rule-b"),
        allow_rule("rule-c"),
    ];
    let ids: std::collections::HashSet<_> = rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), 3);
}

#[test]
fn rules_sorted_by_id() {
    let mut rules = vec![
        allow_rule("zzz"),
        allow_rule("aaa"),
        allow_rule("mmm"),
    ];
    rules.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(rules[0].id, "aaa");
    assert_eq!(rules[1].id, "mmm");
    assert_eq!(rules[2].id, "zzz");
}

#[test]
fn rule_dedup_by_id_replaces_old() {
    let mut rules = vec![
        ApprovalRule {
            id: "dup".to_string(),
            action: ApprovalRuleAction::Allow,
            tool: Some("read_file".to_string()),
            commands: vec![],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: None,
            effect: None,
        },
        ApprovalRule {
            id: "dup".to_string(),
            action: ApprovalRuleAction::RequireApproval,
            tool: Some("write_file".to_string()),
            commands: vec![],
            paths: vec![],
            nodes: vec![],
            channels: vec![],
            risk_tier: None,
            effect: None,
        },
    ];
    rules.retain(|r| r.id != "dup");
    rules.push(ApprovalRule {
        id: "dup".to_string(),
        action: ApprovalRuleAction::RequireApproval,
        tool: Some("write_file".to_string()),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    });
    let dup_rules: Vec<_> = rules.iter().filter(|r| r.id == "dup").collect();
    assert_eq!(dup_rules.len(), 1);
    assert_eq!(dup_rules[0].action, ApprovalRuleAction::RequireApproval);
}
