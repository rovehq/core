//! Tests for security::risk_assessor — RiskTier, OperationSource, Operation, RiskAssessor,
//! classify_terminal_command

use rove_engine::risk_assessor::{
    classify_terminal_command, Operation, OperationSource, RiskAssessor, RiskTier,
};

// ── RiskTier escalate ─────────────────────────────────────────────────────────

#[test]
fn tier0_escalates_to_tier1() {
    assert_eq!(RiskTier::Tier0.escalate(), RiskTier::Tier1);
}

#[test]
fn tier1_escalates_to_tier2() {
    assert_eq!(RiskTier::Tier1.escalate(), RiskTier::Tier2);
}

#[test]
fn tier2_stays_tier2() {
    assert_eq!(RiskTier::Tier2.escalate(), RiskTier::Tier2);
}

#[test]
fn tier0_double_escalate_is_tier2() {
    assert_eq!(RiskTier::Tier0.escalate().escalate(), RiskTier::Tier2);
}

// ── RiskTier equality ─────────────────────────────────────────────────────────

#[test]
fn tier_equality_tier0() {
    assert_eq!(RiskTier::Tier0, RiskTier::Tier0);
}

#[test]
fn tier_equality_tier1() {
    assert_eq!(RiskTier::Tier1, RiskTier::Tier1);
}

#[test]
fn tier_equality_tier2() {
    assert_eq!(RiskTier::Tier2, RiskTier::Tier2);
}

#[test]
fn tier_inequality_0_1() {
    assert_ne!(RiskTier::Tier0, RiskTier::Tier1);
}

#[test]
fn tier_inequality_1_2() {
    assert_ne!(RiskTier::Tier1, RiskTier::Tier2);
}

#[test]
fn tier_inequality_0_2() {
    assert_ne!(RiskTier::Tier0, RiskTier::Tier2);
}

// ── RiskTier copy/clone ───────────────────────────────────────────────────────

#[test]
fn tier_copy() {
    let t = RiskTier::Tier1;
    let t2 = t;
    assert_eq!(t, t2);
}

#[test]
fn tier_debug() {
    let s = format!("{:?}", RiskTier::Tier0);
    assert!(s.contains("Tier0"));
}

#[test]
fn tier_serializes() {
    let j = serde_json::to_string(&RiskTier::Tier1).unwrap();
    assert!(!j.is_empty());
}

// ── OperationSource ───────────────────────────────────────────────────────────

#[test]
fn local_source_not_remote() {
    assert!(!OperationSource::Local.is_remote());
}

#[test]
fn remote_source_is_remote() {
    assert!(OperationSource::Remote.is_remote());
}

#[test]
fn source_equality_local() {
    assert_eq!(OperationSource::Local, OperationSource::Local);
}

#[test]
fn source_equality_remote() {
    assert_eq!(OperationSource::Remote, OperationSource::Remote);
}

#[test]
fn source_inequality() {
    assert_ne!(OperationSource::Local, OperationSource::Remote);
}

#[test]
fn source_clone_local() {
    let s = OperationSource::Local;
    let s2 = s.clone();
    assert_eq!(s, s2);
}

#[test]
fn source_clone_remote() {
    let s = OperationSource::Remote;
    let s2 = s.clone();
    assert_eq!(s, s2);
}

#[test]
fn source_debug_local() {
    let s = format!("{:?}", OperationSource::Local);
    assert!(s.contains("Local"));
}

#[test]
fn source_debug_remote() {
    let s = format!("{:?}", OperationSource::Remote);
    assert!(s.contains("Remote"));
}

// ── Operation::new ────────────────────────────────────────────────────────────

#[test]
fn operation_new_stores_name() {
    let op = Operation::new("read_file", vec![], OperationSource::Local);
    assert_eq!(op.name, "read_file");
}

#[test]
fn operation_new_stores_args() {
    let op = Operation::new(
        "write_file",
        vec!["a.txt".to_string()],
        OperationSource::Local,
    );
    assert_eq!(op.args.len(), 1);
    assert_eq!(op.args[0], "a.txt");
}

#[test]
fn operation_new_stores_source_local() {
    let op = Operation::new("list_dir", vec![], OperationSource::Local);
    assert_eq!(op.source, OperationSource::Local);
}

#[test]
fn operation_new_stores_source_remote() {
    let op = Operation::new("read_file", vec![], OperationSource::Remote);
    assert_eq!(op.source, OperationSource::Remote);
}

#[test]
fn operation_empty_args() {
    let op = Operation::new("git_status", vec![], OperationSource::Local);
    assert!(op.args.is_empty());
}

// ── RiskAssessor::new / default ───────────────────────────────────────────────

#[test]
fn assessor_new_constructs() {
    let _ = RiskAssessor::new();
}

#[test]
fn assessor_default_constructs() {
    let _ = RiskAssessor::default();
}

// ── Tier 0 operations (read-only) ─────────────────────────────────────────────

#[test]
fn read_file_is_tier0() {
    let a = RiskAssessor::new();
    let op = Operation::new("read_file", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn list_dir_is_tier0() {
    let a = RiskAssessor::new();
    let op = Operation::new("list_dir", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn git_status_is_tier0() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_status", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn git_log_is_tier0() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_log", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn execute_task_is_tier0() {
    let a = RiskAssessor::new();
    let op = Operation::new("execute_task", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn cargo_verify_is_tier0() {
    let a = RiskAssessor::new();
    let op = Operation::new("cargo_verify", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier0);
}

// ── Tier 1 operations (write/reversible) ──────────────────────────────────────

#[test]
fn write_file_is_tier1() {
    let a = RiskAssessor::new();
    let op = Operation::new("write_file", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier1);
}

#[test]
fn git_add_is_tier1() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_add", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier1);
}

#[test]
fn git_commit_is_tier1() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_commit", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier1);
}

#[test]
fn create_dir_is_tier1() {
    let a = RiskAssessor::new();
    let op = Operation::new("create_dir", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier1);
}

#[test]
fn cargo_build_is_tier1() {
    let a = RiskAssessor::new();
    let op = Operation::new("cargo_build", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier1);
}

// ── Tier 2 operations (destructive) ───────────────────────────────────────────

#[test]
fn delete_file_is_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("delete_file", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn git_push_is_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_push", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn execute_command_is_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("execute_command", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn git_reset_is_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_reset", vec![], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

// ── Unknown operation returns error ──────────────────────────────────────────

#[test]
fn unknown_operation_returns_error() {
    let a = RiskAssessor::new();
    let op = Operation::new("teleport", vec![], OperationSource::Local);
    assert!(a.assess(&op).is_err());
}

#[test]
fn empty_operation_name_returns_error() {
    let a = RiskAssessor::new();
    let op = Operation::new("", vec![], OperationSource::Local);
    assert!(a.assess(&op).is_err());
}

// ── Dangerous flags escalation ─────────────────────────────────────────────────

#[test]
fn force_flag_escalates_to_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_push", vec!["--force".to_string()], OperationSource::Local);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn rf_flag_escalates_to_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "execute_command",
        vec!["rm".to_string(), "-rf".to_string()],
        OperationSource::Local,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn delete_flag_escalates_to_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "git_push",
        vec!["--delete".to_string()],
        OperationSource::Local,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn hard_flag_escalates_to_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "git_reset",
        vec!["--hard".to_string(), "HEAD~1".to_string()],
        OperationSource::Local,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn force_flag_escalates_tier0_op() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "read_file",
        vec!["--force".to_string()],
        OperationSource::Local,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn force_flag_escalates_tier1_op() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "write_file",
        vec!["--force".to_string()],
        OperationSource::Local,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn safe_flags_do_not_escalate() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "write_file",
        vec!["--verbose".to_string(), "--output".to_string()],
        OperationSource::Local,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier1);
}

#[test]
fn multiple_safe_args_no_escalation() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "list_dir",
        vec!["/workspace".to_string(), "--all".to_string()],
        OperationSource::Local,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier0);
}

// ── Remote source always Tier2 ────────────────────────────────────────────────

#[test]
fn remote_tier0_op_becomes_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("read_file", vec![], OperationSource::Remote);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn remote_tier1_op_becomes_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("write_file", vec![], OperationSource::Remote);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn remote_tier2_op_stays_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("delete_file", vec![], OperationSource::Remote);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn remote_git_status_is_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new("git_status", vec![], OperationSource::Remote);
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn remote_with_safe_flags_is_tier2() {
    let a = RiskAssessor::new();
    let op = Operation::new(
        "write_file",
        vec!["--verbose".to_string()],
        OperationSource::Remote,
    );
    assert_eq!(a.assess(&op).unwrap(), RiskTier::Tier2);
}

// ── classify_terminal_command ─────────────────────────────────────────────────

#[test]
fn classify_git_status() {
    assert_eq!(classify_terminal_command("git status"), "git_status");
}

#[test]
fn classify_git_log() {
    assert_eq!(classify_terminal_command("git log --oneline"), "git_log");
}

#[test]
fn classify_git_diff() {
    assert_eq!(classify_terminal_command("git diff HEAD"), "git_log");
}

#[test]
fn classify_git_show() {
    assert_eq!(classify_terminal_command("git show abc1234"), "git_log");
}

#[test]
fn classify_git_add() {
    assert_eq!(classify_terminal_command("git add src/main.rs"), "git_add");
}

#[test]
fn classify_git_commit() {
    assert_eq!(
        classify_terminal_command("git commit -m 'fix'"),
        "git_commit"
    );
}

#[test]
fn classify_git_push() {
    assert_eq!(classify_terminal_command("git push origin main"), "git_push");
}

#[test]
fn classify_git_reset() {
    assert_eq!(classify_terminal_command("git reset HEAD~1"), "git_reset");
}

#[test]
fn classify_git_branch_show_current() {
    assert_eq!(
        classify_terminal_command("git branch --show-current"),
        "git_status"
    );
}

#[test]
fn classify_git_rev_parse_head() {
    assert_eq!(
        classify_terminal_command("git rev-parse --abbrev-ref HEAD"),
        "git_status"
    );
}

#[test]
fn classify_cargo_check() {
    assert_eq!(classify_terminal_command("cargo check"), "cargo_verify");
}

#[test]
fn classify_cargo_test() {
    assert_eq!(classify_terminal_command("cargo test"), "cargo_verify");
}

#[test]
fn classify_cargo_clippy() {
    assert_eq!(classify_terminal_command("cargo clippy"), "cargo_verify");
}

#[test]
fn classify_cargo_build() {
    assert_eq!(classify_terminal_command("cargo build"), "cargo_build");
}

#[test]
fn classify_cargo_unknown_subcommand() {
    assert_eq!(
        classify_terminal_command("cargo install serde"),
        "execute_command"
    );
}

#[test]
fn classify_rg_search() {
    assert_eq!(classify_terminal_command("rg TODO src/"), "read_file");
}

#[test]
fn classify_fd_search() {
    assert_eq!(
        classify_terminal_command("fd Cargo.toml ."),
        "read_file"
    );
}

#[test]
fn classify_bat_view() {
    assert_eq!(classify_terminal_command("bat src/main.rs"), "read_file");
}

#[test]
fn classify_empty_is_execute_command() {
    assert_eq!(classify_terminal_command(""), "execute_command");
}

#[test]
fn classify_unknown_command_is_execute_command() {
    assert_eq!(classify_terminal_command("curl http://example.com"), "execute_command");
}

#[test]
fn classify_ls_is_execute_command() {
    // ls is not in the safe list
    assert_eq!(classify_terminal_command("ls -la"), "execute_command");
}

#[test]
fn classify_rm_is_execute_command() {
    assert_eq!(classify_terminal_command("rm -rf /tmp/file"), "execute_command");
}

#[test]
fn classify_npm_is_execute_command() {
    assert_eq!(classify_terminal_command("npm install"), "execute_command");
}
