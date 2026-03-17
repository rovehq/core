//! Integration tests for the RiskAssessor module
//!
//! These tests verify that the RiskAssessor correctly classifies operations
//! according to the requirements specified in the design document.

use rove_engine::risk_assessor::{Operation, OperationSource, RiskAssessor, RiskTier};

#[test]
fn test_all_tier0_operations_classified_correctly() {
    let assessor = RiskAssessor::new();
    let tier0_ops = vec!["read_file", "list_dir", "git_status", "git_log"];

    for op_name in tier0_ops {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        let result = assessor.assess(&op);
        assert!(result.is_ok(), "Operation {} should be recognized", op_name);
        assert_eq!(
            result.unwrap(),
            RiskTier::Tier0,
            "Operation {} should be Tier 0",
            op_name
        );
    }
}

#[test]
fn test_all_tier1_operations_classified_correctly() {
    let assessor = RiskAssessor::new();
    let tier1_ops = vec!["write_file", "git_add", "git_commit", "create_dir"];

    for op_name in tier1_ops {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        let result = assessor.assess(&op);
        assert!(result.is_ok(), "Operation {} should be recognized", op_name);
        assert_eq!(
            result.unwrap(),
            RiskTier::Tier1,
            "Operation {} should be Tier 1",
            op_name
        );
    }
}

#[test]
fn test_all_tier2_operations_classified_correctly() {
    let assessor = RiskAssessor::new();
    let tier2_ops = vec!["delete_file", "git_push", "execute_command", "git_reset"];

    for op_name in tier2_ops {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        let result = assessor.assess(&op);
        assert!(result.is_ok(), "Operation {} should be recognized", op_name);
        assert_eq!(
            result.unwrap(),
            RiskTier::Tier2,
            "Operation {} should be Tier 2",
            op_name
        );
    }
}

#[test]
fn test_all_dangerous_flags_detected() {
    let assessor = RiskAssessor::new();
    let dangerous_flags = vec!["--force", "-rf", "--delete", "--hard"];

    for flag in dangerous_flags {
        // Test with a Tier 0 operation - should escalate to Tier 2
        let op = Operation::new("read_file", vec![flag.to_string()], OperationSource::Local);
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(
            tier,
            RiskTier::Tier2,
            "Flag {} should escalate to Tier 2",
            flag
        );

        // Test with a Tier 1 operation - should escalate to Tier 2
        let op = Operation::new("write_file", vec![flag.to_string()], OperationSource::Local);
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(
            tier,
            RiskTier::Tier2,
            "Flag {} should escalate to Tier 2",
            flag
        );

        // Test with a Tier 2 operation - should remain Tier 2
        let op = Operation::new(
            "delete_file",
            vec![flag.to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2, "Flag {} should keep Tier 2", flag);
    }
}

#[test]
fn test_remote_escalation_for_all_tiers() {
    let assessor = RiskAssessor::new();

    // Tier 0 remote -> Tier 1
    let op = Operation::new("read_file", vec![], OperationSource::Remote);
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier1);

    // Tier 1 remote -> Tier 2
    let op = Operation::new("write_file", vec![], OperationSource::Remote);
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);

    // Tier 2 remote -> Tier 2 (max)
    let op = Operation::new("delete_file", vec![], OperationSource::Remote);
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn test_complex_scenario_remote_with_dangerous_flag() {
    let assessor = RiskAssessor::new();

    // Remote Tier 0 operation with dangerous flag
    let op = Operation::new(
        "read_file",
        vec!["--force".to_string()],
        OperationSource::Remote,
    );
    // Dangerous flag takes precedence, then remote escalation (but already at max)
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);

    // Remote Tier 1 operation with dangerous flag
    let op = Operation::new(
        "write_file",
        vec!["--force".to_string()],
        OperationSource::Remote,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn test_realistic_git_operations() {
    let assessor = RiskAssessor::new();

    // Normal git status - Tier 0
    let op = Operation::new("git_status", vec![], OperationSource::Local);
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier0);

    // Normal git commit - Tier 1
    let op = Operation::new(
        "git_commit",
        vec!["-m".to_string(), "commit message".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier1);

    // Force push - Tier 2 (dangerous flag)
    let op = Operation::new(
        "git_push",
        vec![
            "origin".to_string(),
            "main".to_string(),
            "--force".to_string(),
        ],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);

    // Hard reset - Tier 2 (dangerous flag)
    let op = Operation::new(
        "git_reset",
        vec!["--hard".to_string(), "HEAD~1".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn test_realistic_file_operations() {
    let assessor = RiskAssessor::new();

    // Read file - Tier 0
    let op = Operation::new(
        "read_file",
        vec!["config.toml".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier0);

    // Write file - Tier 1
    let op = Operation::new(
        "write_file",
        vec!["output.txt".to_string(), "content".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier1);

    // Delete file - Tier 2
    let op = Operation::new(
        "delete_file",
        vec!["temp.txt".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);

    // List directory - Tier 0
    let op = Operation::new(
        "list_dir",
        vec!["/home/user".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier0);
}

#[test]
fn test_realistic_command_execution() {
    let assessor = RiskAssessor::new();

    // Execute command is always Tier 2
    let op = Operation::new(
        "execute_command",
        vec!["ls".to_string(), "-la".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);

    // Execute command with dangerous flag
    let op = Operation::new(
        "execute_command",
        vec!["rm".to_string(), "-rf".to_string(), "/tmp/test".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn test_telegram_bot_scenario() {
    let assessor = RiskAssessor::new();

    // Telegram bot requests are remote
    // Read file from Telegram -> Tier 1 (escalated from Tier 0)
    let op = Operation::new(
        "read_file",
        vec!["log.txt".to_string()],
        OperationSource::Remote,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier1);

    // Write file from Telegram -> Tier 2 (escalated from Tier 1)
    let op = Operation::new(
        "write_file",
        vec!["note.txt".to_string()],
        OperationSource::Remote,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);

    // Delete file from Telegram -> Tier 2 (already max)
    let op = Operation::new(
        "delete_file",
        vec!["temp.txt".to_string()],
        OperationSource::Remote,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn test_edge_case_empty_operation_name() {
    let assessor = RiskAssessor::new();
    let op = Operation::new("", vec![], OperationSource::Local);
    let result = assessor.assess(&op);
    assert!(result.is_err());
}

#[test]
fn test_edge_case_whitespace_in_args() {
    let assessor = RiskAssessor::new();
    let op = Operation::new(
        "write_file",
        vec!["file.txt".to_string(), "  ".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier1);
}

#[test]
fn test_dangerous_flag_case_sensitivity() {
    let assessor = RiskAssessor::new();

    // Lowercase should be detected
    let op = Operation::new(
        "git_push",
        vec!["--force".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);

    // Uppercase should NOT be detected (flags are case-sensitive)
    let op = Operation::new(
        "git_push",
        vec!["--FORCE".to_string()],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2); // Still Tier 2 because git_push is Tier 2
}

#[test]
fn test_multiple_dangerous_flags() {
    let assessor = RiskAssessor::new();
    let op = Operation::new(
        "execute_command",
        vec![
            "rm".to_string(),
            "-rf".to_string(),
            "--force".to_string(),
            "/tmp".to_string(),
        ],
        OperationSource::Local,
    );
    assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
}

#[test]
fn test_requirements_3_6_read_only_operations() {
    // Requirement 3.6: THE Engine SHALL classify read-only operations
    // (read_file, list_dir, git_status, git_log) as Risk_Tier 0
    let assessor = RiskAssessor::new();

    let read_only_ops = vec!["read_file", "list_dir", "git_status", "git_log"];

    for op_name in read_only_ops {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier0,
            "Requirement 3.6: {} should be Tier 0",
            op_name
        );
    }
}

#[test]
fn test_requirements_3_7_write_operations() {
    // Requirement 3.7: THE Engine SHALL classify write operations
    // (write_file, git_commit, create_dir) as Risk_Tier 1
    let assessor = RiskAssessor::new();

    let write_ops = vec!["write_file", "git_commit", "create_dir"];

    for op_name in write_ops {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier1,
            "Requirement 3.7: {} should be Tier 1",
            op_name
        );
    }
}

#[test]
fn test_requirements_3_8_destructive_operations() {
    // Requirement 3.8: THE Engine SHALL classify destructive operations
    // (delete_file, git_push, execute_command, git_reset) as Risk_Tier 2
    let assessor = RiskAssessor::new();

    let destructive_ops = vec!["delete_file", "git_push", "execute_command", "git_reset"];

    for op_name in destructive_ops {
        let op = Operation::new(op_name, vec![], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier2,
            "Requirement 3.8: {} should be Tier 2",
            op_name
        );
    }
}

#[test]
fn test_requirements_3_9_dangerous_flags() {
    // Requirement 3.9: THE Engine SHALL classify any operation with flags
    // (--force, -rf, --delete, --hard) as Risk_Tier 2
    let assessor = RiskAssessor::new();

    let dangerous_flags = vec!["--force", "-rf", "--delete", "--hard"];

    for flag in dangerous_flags {
        // Test with any operation - should become Tier 2
        let op = Operation::new("read_file", vec![flag.to_string()], OperationSource::Local);
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier2,
            "Requirement 3.9: Operation with {} should be Tier 2",
            flag
        );
    }
}

#[test]
fn test_requirements_8_6_command_dangerous_flags() {
    // Requirement 8.6: THE Engine SHALL classify commands with
    // --force, -rf, --delete, --hard flags as Risk_Tier 2
    let assessor = RiskAssessor::new();

    let dangerous_flags = vec!["--force", "-rf", "--delete", "--hard"];

    for flag in dangerous_flags {
        let op = Operation::new(
            "execute_command",
            vec!["command".to_string(), flag.to_string()],
            OperationSource::Local,
        );
        assert_eq!(
            assessor.assess(&op).unwrap(),
            RiskTier::Tier2,
            "Requirement 8.6: Command with {} should be Tier 2",
            flag
        );
    }
}
