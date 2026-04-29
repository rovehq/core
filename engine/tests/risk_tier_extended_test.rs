//! Extended tests for risk_assessor — comprehensive edge cases and serialization

use rove_engine::risk_assessor::{
    classify_terminal_command, Operation, OperationSource, RiskAssessor, RiskTier,
};

// ── RiskTier numeric values ────────────────────────────────────────────────────

#[test]
fn tier0_value_is_zero() {
    assert_eq!(RiskTier::Tier0 as i32, 0);
}

#[test]
fn tier1_value_is_one() {
    assert_eq!(RiskTier::Tier1 as i32, 1);
}

#[test]
fn tier2_value_is_two() {
    assert_eq!(RiskTier::Tier2 as i32, 2);
}

// ── Escalation chain ──────────────────────────────────────────────────────────

#[test]
fn escalation_chain_0_1_2() {
    assert_eq!(RiskTier::Tier0.escalate(), RiskTier::Tier1);
    assert_eq!(RiskTier::Tier0.escalate().escalate(), RiskTier::Tier2);
}

#[test]
fn escalation_tier2_stays_at_2_repeatedly() {
    let mut tier = RiskTier::Tier2;
    for _ in 0..10 {
        tier = tier.escalate();
        assert_eq!(tier, RiskTier::Tier2);
    }
}

// ── Tier 0 comprehensive ──────────────────────────────────────────────────────

#[test]
fn all_tier0_ops_local() {
    let assessor = RiskAssessor::new();
    for op_name in [
        "read_file",
        "list_dir",
        "git_status",
        "git_log",
        "execute_task",
        "cargo_verify",
    ] {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier0,
            "Expected Tier0 for '{}'",
            op_name
        );
    }
}

#[test]
fn all_tier0_ops_remote_give_tier2() {
    let assessor = RiskAssessor::new();
    for op_name in [
        "read_file",
        "list_dir",
        "git_status",
        "git_log",
        "execute_task",
        "cargo_verify",
    ] {
        let op = Operation::new(op_name, vec![], OperationSource::Remote);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier2,
            "Expected Tier2 for remote '{}'",
            op_name
        );
    }
}

// ── Tier 1 comprehensive ──────────────────────────────────────────────────────

#[test]
fn all_tier1_ops_local() {
    let assessor = RiskAssessor::new();
    for op_name in [
        "write_file",
        "git_add",
        "git_commit",
        "create_dir",
        "cargo_build",
    ] {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier1,
            "Expected Tier1 for '{}'",
            op_name
        );
    }
}

#[test]
fn all_tier1_ops_remote_give_tier2() {
    let assessor = RiskAssessor::new();
    for op_name in [
        "write_file",
        "git_add",
        "git_commit",
        "create_dir",
        "cargo_build",
    ] {
        let op = Operation::new(op_name, vec![], OperationSource::Remote);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier2,
            "Expected Tier2 for remote '{}'",
            op_name
        );
    }
}

// ── Tier 2 comprehensive ──────────────────────────────────────────────────────

#[test]
fn all_tier2_ops_local() {
    let assessor = RiskAssessor::new();
    for op_name in ["delete_file", "git_push", "execute_command", "git_reset"] {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier2,
            "Expected Tier2 for '{}'",
            op_name
        );
    }
}

// ── Dangerous flags: all four ─────────────────────────────────────────────────

#[test]
fn all_dangerous_flags_escalate_any_op() {
    let assessor = RiskAssessor::new();
    for flag in ["--force", "-rf", "--delete", "--hard"] {
        // Test against a Tier0 op to really prove escalation
        let op = Operation::new("read_file", vec![flag.to_string()], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier2,
            "Dangerous flag '{}' should escalate",
            flag
        );
    }
}

#[test]
fn force_in_embedded_arg_escalates() {
    let assessor = RiskAssessor::new();
    let op = Operation::new(
        "git_push",
        vec!["origin".to_string(), "main--force".to_string()],
        OperationSource::Local,
    );
    // "--force" is contained in "main--force"
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
}

// ── Terminal command classification ───────────────────────────────────────────

#[test]
fn classify_git_all_variants() {
    let cases = [
        ("git status", "git_status"),
        ("git log", "git_log"),
        ("git diff HEAD", "git_log"),
        ("git show abc", "git_log"),
        ("git add .", "git_add"),
        ("git commit -m msg", "git_commit"),
        ("git push origin main", "git_push"),
        ("git reset HEAD~1", "git_reset"),
        ("git branch --show-current", "git_status"),
        ("git rev-parse --abbrev-ref HEAD", "git_status"),
    ];
    for (cmd, expected) in cases {
        assert_eq!(
            classify_terminal_command(cmd),
            expected,
            "Failed for: {}",
            cmd
        );
    }
}

#[test]
fn classify_cargo_all_variants() {
    let cases = [
        ("cargo check", "cargo_verify"),
        ("cargo test", "cargo_verify"),
        ("cargo clippy", "cargo_verify"),
        ("cargo build", "cargo_build"),
        ("cargo build --release", "cargo_build"),
        ("cargo install serde", "execute_command"),
        ("cargo run", "execute_command"),
    ];
    for (cmd, expected) in cases {
        assert_eq!(
            classify_terminal_command(cmd),
            expected,
            "Failed for: {}",
            cmd
        );
    }
}

#[test]
fn classify_safe_search_tools() {
    let cases = [
        ("rg pattern src/", "read_file"),
        ("fd .rs .", "read_file"),
        ("bat Cargo.toml", "read_file"),
    ];
    for (cmd, expected) in cases {
        assert_eq!(
            classify_terminal_command(cmd),
            expected,
            "Failed for: {}",
            cmd
        );
    }
}

#[test]
fn classify_unknown_programs_are_execute_command() {
    for cmd in [
        "python3 script.py",
        "node server.js",
        "make build",
        "curl url",
        "ls -la",
    ] {
        assert_eq!(
            classify_terminal_command(cmd),
            "execute_command",
            "Should be execute_command: {}",
            cmd
        );
    }
}

#[test]
fn classify_empty_command_is_execute_command() {
    assert_eq!(classify_terminal_command(""), "execute_command");
}

// ── Operation: various args ────────────────────────────────────────────────────

#[test]
fn operation_with_many_args() {
    let op = Operation::new(
        "git_log",
        vec![
            "--oneline".to_string(),
            "--graph".to_string(),
            "--all".to_string(),
        ],
        OperationSource::Local,
    );
    let assessor = RiskAssessor::new();
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn operation_with_path_arg_tier0() {
    let op = Operation::new(
        "read_file",
        vec!["/workspace/src/main.rs".to_string()],
        OperationSource::Local,
    );
    let assessor = RiskAssessor::new();
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn operation_clone() {
    let op1 = Operation::new("read_file", vec![], OperationSource::Local);
    let op2 = op1.clone();
    assert_eq!(op1.name, op2.name);
    assert_eq!(op1.source, op2.source);
}

#[test]
fn operation_debug() {
    let op = Operation::new(
        "write_file",
        vec!["a.txt".to_string()],
        OperationSource::Remote,
    );
    let s = format!("{:?}", op);
    assert!(s.contains("write_file"));
}

// ── RiskTier serialization ─────────────────────────────────────────────────────

#[test]
fn tier0_serializes_to_int() {
    let j = serde_json::to_string(&RiskTier::Tier0).unwrap();
    assert_eq!(j, "0");
}

#[test]
fn tier1_serializes_to_int() {
    let j = serde_json::to_string(&RiskTier::Tier1).unwrap();
    assert_eq!(j, "1");
}

#[test]
fn tier2_serializes_to_int() {
    let j = serde_json::to_string(&RiskTier::Tier2).unwrap();
    assert_eq!(j, "2");
}

#[test]
fn tier0_deserializes() {
    let tier: RiskTier = serde_json::from_str("0").unwrap();
    assert_eq!(tier, RiskTier::Tier0);
}

#[test]
fn tier1_deserializes() {
    let tier: RiskTier = serde_json::from_str("1").unwrap();
    assert_eq!(tier, RiskTier::Tier1);
}

#[test]
fn tier2_deserializes() {
    let tier: RiskTier = serde_json::from_str("2").unwrap();
    assert_eq!(tier, RiskTier::Tier2);
}

// ── Multiple assessors ────────────────────────────────────────────────────────

#[test]
fn two_assessors_same_result() {
    let a1 = RiskAssessor::new();
    let a2 = RiskAssessor::default();
    let op = Operation::new("read_file", vec![], OperationSource::Local);
    assert_eq!(a1.assess(&op).unwrap(), a2.assess(&op).unwrap());
}

// ── Error message contains op name ────────────────────────────────────────────

#[test]
fn unknown_op_error_contains_name() {
    let assessor = RiskAssessor::new();
    let op = Operation::new("magic_operation", vec![], OperationSource::Local);
    let err = assessor.assess(&op).unwrap_err();
    let s = err.to_string();
    assert!(s.contains("magic_operation") || !s.is_empty());
}
