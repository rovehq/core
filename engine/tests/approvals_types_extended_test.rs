//! Extended tests for security::approvals — types, enums, rules

use rove_engine::config::ApprovalMode;
use rove_engine::security::approvals::{
    ApprovalDecision, ApprovalRequest, ApprovalRule, ApprovalRuleAction, ApprovalRulesFile,
};
use sdk::TaskSource;

// ── ApprovalMode (from config) ────────────────────────────────────────────────

#[test]
fn approval_mode_open_eq() {
    assert_eq!(ApprovalMode::Open, ApprovalMode::Open);
}

#[test]
fn approval_mode_default_eq() {
    assert_eq!(ApprovalMode::Default, ApprovalMode::Default);
}

#[test]
fn approval_mode_allowlist_eq() {
    assert_eq!(ApprovalMode::Allowlist, ApprovalMode::Allowlist);
}

#[test]
fn approval_mode_open_ne_default() {
    assert_ne!(ApprovalMode::Open, ApprovalMode::Default);
}

#[test]
fn approval_mode_default_ne_allowlist() {
    assert_ne!(ApprovalMode::Default, ApprovalMode::Allowlist);
}

#[test]
fn approval_mode_open_ne_allowlist() {
    assert_ne!(ApprovalMode::Open, ApprovalMode::Allowlist);
}

#[test]
fn approval_mode_debug_open() {
    let s = format!("{:?}", ApprovalMode::Open);
    assert!(s.contains("Open"));
}

#[test]
fn approval_mode_debug_default() {
    let s = format!("{:?}", ApprovalMode::Default);
    assert!(s.contains("Default"));
}

#[test]
fn approval_mode_debug_allowlist() {
    let s = format!("{:?}", ApprovalMode::Allowlist);
    assert!(s.contains("Allowlist"));
}

#[test]
fn approval_mode_clone() {
    let m = ApprovalMode::Allowlist;
    assert_eq!(m, m.clone());
}

#[test]
fn approval_mode_copy() {
    let m = ApprovalMode::Open;
    let m2 = m;
    assert_eq!(m, m2);
}

// ── ApprovalRuleAction ────────────────────────────────────────────────────────

#[test]
fn rule_action_allow_eq() {
    assert_eq!(ApprovalRuleAction::Allow, ApprovalRuleAction::Allow);
}

#[test]
fn rule_action_require_eq() {
    assert_eq!(ApprovalRuleAction::RequireApproval, ApprovalRuleAction::RequireApproval);
}

#[test]
fn rule_action_allow_ne_require() {
    assert_ne!(ApprovalRuleAction::Allow, ApprovalRuleAction::RequireApproval);
}

#[test]
fn rule_action_debug_allow() {
    let s = format!("{:?}", ApprovalRuleAction::Allow);
    assert!(s.contains("Allow"));
}

#[test]
fn rule_action_debug_require() {
    let s = format!("{:?}", ApprovalRuleAction::RequireApproval);
    assert!(s.contains("Require"));
}

#[test]
fn rule_action_clone() {
    let a = ApprovalRuleAction::Allow;
    assert_eq!(a, a.clone());
}

#[test]
fn rule_action_serializes_allow() {
    let j = serde_json::to_string(&ApprovalRuleAction::Allow).unwrap();
    assert!(!j.is_empty());
    assert!(j.contains("allow") || j.contains("Allow"));
}

#[test]
fn rule_action_serializes_require() {
    let j = serde_json::to_string(&ApprovalRuleAction::RequireApproval).unwrap();
    assert!(!j.is_empty());
}

#[test]
fn rule_action_roundtrip_allow() {
    let j = serde_json::to_string(&ApprovalRuleAction::Allow).unwrap();
    let back: ApprovalRuleAction = serde_json::from_str(&j).unwrap();
    assert_eq!(back, ApprovalRuleAction::Allow);
}

#[test]
fn rule_action_roundtrip_require() {
    let j = serde_json::to_string(&ApprovalRuleAction::RequireApproval).unwrap();
    let back: ApprovalRuleAction = serde_json::from_str(&j).unwrap();
    assert_eq!(back, ApprovalRuleAction::RequireApproval);
}

// ── ApprovalDecision ──────────────────────────────────────────────────────────

#[test]
fn decision_auto_allow_matches() {
    let d = ApprovalDecision::AutoAllow { reason: "test".to_string() };
    assert!(matches!(d, ApprovalDecision::AutoAllow { .. }));
}

#[test]
fn decision_require_approval_matches() {
    let d = ApprovalDecision::RequireApproval { reason: None };
    assert!(matches!(d, ApprovalDecision::RequireApproval { .. }));
}

#[test]
fn decision_auto_allow_reason_accessible() {
    let d = ApprovalDecision::AutoAllow { reason: "rule matched".to_string() };
    if let ApprovalDecision::AutoAllow { reason } = d {
        assert_eq!(reason, "rule matched");
    } else {
        panic!("expected AutoAllow");
    }
}

#[test]
fn decision_require_approval_reason_some() {
    let d = ApprovalDecision::RequireApproval { reason: Some("rule matched".to_string()) };
    if let ApprovalDecision::RequireApproval { reason } = d {
        assert_eq!(reason.unwrap(), "rule matched");
    }
}

#[test]
fn decision_require_approval_reason_none() {
    let d = ApprovalDecision::RequireApproval { reason: None };
    if let ApprovalDecision::RequireApproval { reason } = d {
        assert!(reason.is_none());
    }
}

#[test]
fn decision_debug_auto_allow() {
    let d = ApprovalDecision::AutoAllow { reason: "x".to_string() };
    let s = format!("{:?}", d);
    assert!(s.contains("AutoAllow"));
}

#[test]
fn decision_debug_require() {
    let d = ApprovalDecision::RequireApproval { reason: None };
    let s = format!("{:?}", d);
    assert!(s.contains("RequireApproval"));
}

// ── TaskSource ────────────────────────────────────────────────────────────────

#[test]
fn task_source_cli() {
    let src = TaskSource::Cli;
    assert!(matches!(src, TaskSource::Cli));
}

#[test]
fn task_source_webui() {
    let src = TaskSource::WebUI;
    assert!(matches!(src, TaskSource::WebUI));
}

#[test]
fn task_source_telegram_string_payload() {
    let src = TaskSource::Telegram("123456789".to_string());
    if let TaskSource::Telegram(id) = &src {
        assert_eq!(id, "123456789");
    } else {
        panic!("expected Telegram");
    }
}

#[test]
fn task_source_remote_node() {
    let src = TaskSource::Remote("node-abc".to_string());
    if let TaskSource::Remote(node) = &src {
        assert_eq!(node, "node-abc");
    }
}

#[test]
fn task_source_subagent() {
    let src = TaskSource::Subagent("parent-id".to_string());
    if let TaskSource::Subagent(id) = &src {
        assert_eq!(id, "parent-id");
    }
}

#[test]
fn task_source_channel() {
    let src = TaskSource::Channel("discord".to_string());
    if let TaskSource::Channel(kind) = &src {
        assert_eq!(kind, "discord");
    }
}

#[test]
fn task_source_cli_debug() {
    let s = format!("{:?}", TaskSource::Cli);
    assert!(s.contains("Cli"));
}

#[test]
fn task_source_telegram_clone() {
    let src = TaskSource::Telegram("123".to_string());
    let src2 = src.clone();
    assert_eq!(src, src2);
}

// ── ApprovalRequest ───────────────────────────────────────────────────────────

#[test]
fn request_id_field() {
    let r = ApprovalRequest {
        id: "req-1".to_string(),
        task_id: "task-1".to_string(),
        tool_name: "write_file".to_string(),
        risk_tier: 2,
        summary: "Write file at /tmp/test".to_string(),
        created_at: 1000,
        auto_resolve_after_secs: None,
    };
    assert_eq!(r.id, "req-1");
}

#[test]
fn request_task_id_field() {
    let r = ApprovalRequest {
        id: "req-2".to_string(),
        task_id: "task-99".to_string(),
        tool_name: "delete_file".to_string(),
        risk_tier: 2,
        summary: "Delete /tmp/foo".to_string(),
        created_at: 2000,
        auto_resolve_after_secs: None,
    };
    assert_eq!(r.task_id, "task-99");
}

#[test]
fn request_risk_tier_zero() {
    let r = ApprovalRequest {
        id: "r".to_string(),
        task_id: "t".to_string(),
        tool_name: "read_file".to_string(),
        risk_tier: 0,
        summary: "read".to_string(),
        created_at: 0,
        auto_resolve_after_secs: None,
    };
    assert_eq!(r.risk_tier, 0);
}

#[test]
fn request_risk_tier_two() {
    let r = ApprovalRequest {
        id: "r".to_string(),
        task_id: "t".to_string(),
        tool_name: "execute_command".to_string(),
        risk_tier: 2,
        summary: "exec".to_string(),
        created_at: 0,
        auto_resolve_after_secs: None,
    };
    assert_eq!(r.risk_tier, 2);
}

#[test]
fn request_auto_resolve_secs_some() {
    let r = ApprovalRequest {
        id: "r".to_string(),
        task_id: "t".to_string(),
        tool_name: "write_file".to_string(),
        risk_tier: 1,
        summary: "write".to_string(),
        created_at: 0,
        auto_resolve_after_secs: Some(300),
    };
    assert_eq!(r.auto_resolve_after_secs, Some(300));
}

#[test]
fn request_auto_resolve_secs_none() {
    let r = ApprovalRequest {
        id: "r".to_string(),
        task_id: "t".to_string(),
        tool_name: "write_file".to_string(),
        risk_tier: 1,
        summary: "write".to_string(),
        created_at: 0,
        auto_resolve_after_secs: None,
    };
    assert!(r.auto_resolve_after_secs.is_none());
}

#[test]
fn request_clone() {
    let r = ApprovalRequest {
        id: "r".to_string(),
        task_id: "t".to_string(),
        tool_name: "write_file".to_string(),
        risk_tier: 1,
        summary: "write".to_string(),
        created_at: 1234,
        auto_resolve_after_secs: None,
    };
    let r2 = r.clone();
    assert_eq!(r.id, r2.id);
    assert_eq!(r.risk_tier, r2.risk_tier);
}

#[test]
fn request_serialize_json() {
    let r = ApprovalRequest {
        id: "r".to_string(),
        task_id: "t".to_string(),
        tool_name: "tool".to_string(),
        risk_tier: 1,
        summary: "summ".to_string(),
        created_at: 99,
        auto_resolve_after_secs: Some(60),
    };
    let j = serde_json::to_string(&r).unwrap();
    assert!(j.contains("risk_tier"));
}

// ── ApprovalRule ─────────────────────────────────────────────────────────────

#[test]
fn rule_id_field() {
    let r = ApprovalRule {
        id: "my-rule".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(r.id, "my-rule");
}

#[test]
fn rule_tool_field_some() {
    let r = ApprovalRule {
        id: "r".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("read_file".to_string()),
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(r.tool.unwrap(), "read_file");
}

#[test]
fn rule_tool_field_none() {
    let r = ApprovalRule {
        id: "r".to_string(),
        action: ApprovalRuleAction::RequireApproval,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert!(r.tool.is_none());
}

#[test]
fn rule_commands_field() {
    let r = ApprovalRule {
        id: "r".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("run_command".to_string()),
        commands: vec!["git status".to_string(), "git diff".to_string()],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(r.commands.len(), 2);
}

#[test]
fn rule_paths_field() {
    let r = ApprovalRule {
        id: "r".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: Some("read_file".to_string()),
        commands: vec![],
        paths: vec!["/workspace/**".to_string()],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(r.paths.len(), 1);
    assert_eq!(r.paths[0], "/workspace/**");
}

#[test]
fn rule_risk_tier_some() {
    let r = ApprovalRule {
        id: "r".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: Some(1),
        effect: None,
    };
    assert_eq!(r.risk_tier, Some(1));
}

#[test]
fn rule_effect_some() {
    let r = ApprovalRule {
        id: "r".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec![],
        risk_tier: None,
        effect: Some("read_only".to_string()),
    };
    assert_eq!(r.effect.unwrap(), "read_only");
}

#[test]
fn rule_channels_field() {
    let r = ApprovalRule {
        id: "r".to_string(),
        action: ApprovalRuleAction::Allow,
        tool: None,
        commands: vec![],
        paths: vec![],
        nodes: vec![],
        channels: vec!["cli".to_string(), "telegram:*".to_string()],
        risk_tier: None,
        effect: None,
    };
    assert_eq!(r.channels.len(), 2);
}

// ── ApprovalRulesFile ─────────────────────────────────────────────────────────

#[test]
fn rules_file_default_empty() {
    let f = ApprovalRulesFile::default();
    assert!(f.rules.is_empty());
}

#[test]
fn rules_file_with_rules() {
    let f = ApprovalRulesFile {
        rules: vec![
            ApprovalRule {
                id: "rule-1".to_string(),
                action: ApprovalRuleAction::Allow,
                tool: None,
                commands: vec![],
                paths: vec![],
                nodes: vec![],
                channels: vec![],
                risk_tier: None,
                effect: None,
            },
        ],
    };
    assert_eq!(f.rules.len(), 1);
}

#[test]
fn rules_file_serialize() {
    let f = ApprovalRulesFile {
        rules: vec![
            ApprovalRule {
                id: "r".to_string(),
                action: ApprovalRuleAction::Allow,
                tool: Some("read_file".to_string()),
                commands: vec![],
                paths: vec![],
                nodes: vec![],
                channels: vec![],
                risk_tier: Some(0),
                effect: None,
            },
        ],
    };
    let toml = toml::to_string(&f);
    assert!(toml.is_ok());
}
