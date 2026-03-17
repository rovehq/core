//! Risk assessment module
//!
//! This module provides risk tier classification for operations.
//! All operations are classified into three risk tiers:
//!
//! - **Tier 0 (Read-only)**: Auto-execute without confirmation
//!   - read_file, list_dir, git_status, git_log
//!
//! - **Tier 1 (Write/Reversible)**: Display operation with 10-second countdown
//!   - write_file, git_commit, create_dir
//!
//! - **Tier 2 (Destructive/Irreversible)**: Require explicit confirmation
//!   - delete_file, git_push, execute_command, git_reset
//!
//! # Security Features
//!
//! - Dangerous flags (--force, -rf, --delete, --hard) automatically escalate to Tier 2
//! - Remote operations are escalated one tier up
//! - All classifications are logged for audit

use sdk::errors::EngineError;
use serde::{Deserialize, Serialize};

/// Risk tier classification for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskTier {
    /// Tier 0: Read-only operations (auto-execute)
    Tier0 = 0,
    /// Tier 1: Write/reversible operations (countdown confirmation)
    Tier1 = 1,
    /// Tier 2: Destructive/irreversible operations (explicit confirmation)
    Tier2 = 2,
}

impl RiskTier {
    /// Escalate the risk tier by one level
    ///
    /// Used when operations are delegated from remote sources.
    /// Tier 0 → Tier 1, Tier 1 → Tier 2, Tier 2 → Tier 2 (max)
    pub fn escalate(self) -> Self {
        match self {
            RiskTier::Tier0 => RiskTier::Tier1,
            RiskTier::Tier1 => RiskTier::Tier2,
            RiskTier::Tier2 => RiskTier::Tier2,
        }
    }
}

/// Source of an operation request
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationSource {
    /// Local CLI request
    Local,
    /// Remote request (Telegram, API, etc.)
    Remote,
}

impl OperationSource {
    /// Check if the source is remote
    pub fn is_remote(&self) -> bool {
        matches!(self, OperationSource::Remote)
    }
}

/// Operation request for risk assessment
#[derive(Debug, Clone)]
pub struct Operation {
    /// The operation name (e.g., "read_file", "delete_file")
    pub name: String,
    /// Arguments passed to the operation
    pub args: Vec<String>,
    /// Source of the operation request
    pub source: OperationSource,
}

impl Operation {
    /// Create a new operation
    pub fn new(name: impl Into<String>, args: Vec<String>, source: OperationSource) -> Self {
        Self {
            name: name.into(),
            args,
            source,
        }
    }
}

/// Risk assessor for operation classification
///
/// The RiskAssessor classifies operations into risk tiers based on:
/// - The operation type (read/write/destructive)
/// - Presence of dangerous flags
/// - Whether the operation is from a remote source
///
/// # Examples
///
/// ```
/// use rove_engine::risk_assessor::{RiskAssessor, Operation, OperationSource, RiskTier};
///
/// let assessor = RiskAssessor::new();
///
/// // Read operation is Tier 0
/// let op = Operation::new("read_file", vec!["test.txt".to_string()], OperationSource::Local);
/// assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier0);
///
/// // Delete operation is Tier 2
/// let op = Operation::new("delete_file", vec!["test.txt".to_string()], OperationSource::Local);
/// assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
///
/// // Operation with --force flag is Tier 2
/// let op = Operation::new("git_push", vec!["--force".to_string()], OperationSource::Local);
/// assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
///
/// // Remote operation is escalated
/// let op = Operation::new("write_file", vec!["test.txt".to_string()], OperationSource::Remote);
/// assert_eq!(assessor.assess(&op).unwrap(), RiskTier::Tier2);
/// ```
pub struct RiskAssessor {
    // Future: Add configuration for custom risk tier mappings
}

impl RiskAssessor {
    /// Create a new RiskAssessor
    pub fn new() -> Self {
        Self {}
    }

    /// Assess the risk tier of an operation
    ///
    /// # Arguments
    ///
    /// * `operation` - The operation to assess
    ///
    /// # Returns
    ///
    /// The risk tier for the operation, or an error if the operation is unknown
    ///
    /// # Security Rules
    ///
    /// - Remote source → ALWAYS Tier2 (regardless of action type)
    /// - Dangerous flags → ALWAYS Tier2
    /// - Otherwise classified by operation type
    ///
    /// # Examples
    ///
    /// ```
    /// use rove_engine::risk_assessor::{RiskAssessor, Operation, OperationSource, RiskTier};
    ///
    /// let assessor = RiskAssessor::new();
    /// let op = Operation::new("read_file", vec![], OperationSource::Local);
    /// let tier = assessor.assess(&op).unwrap();
    /// assert_eq!(tier, RiskTier::Tier0);
    /// ```
    pub fn assess(&self, operation: &Operation) -> Result<RiskTier, EngineError> {
        // Rule 1: Remote source ALWAYS Tier2 (no exceptions)
        if operation.source.is_remote() {
            return Ok(RiskTier::Tier2);
        }

        // Rule 2: Dangerous flags ALWAYS Tier2
        if self.has_dangerous_flags(&operation.args) {
            return Ok(RiskTier::Tier2);
        }

        // Rule 3: Classify by operation type
        self.classify_operation(&operation.name)
    }

    /// Classify an operation by its name
    ///
    /// # Arguments
    ///
    /// * `operation_name` - The name of the operation
    ///
    /// # Returns
    ///
    /// The base risk tier for the operation
    fn classify_operation(&self, operation_name: &str) -> Result<RiskTier, EngineError> {
        match operation_name {
            // Tier 0: Read-only operations and core agent tasks
            "read_file" | "list_dir" | "git_status" | "git_log" | "execute_task" => {
                Ok(RiskTier::Tier0)
            }

            // Tier 1: Write/reversible operations
            "write_file" | "git_add" | "git_commit" | "create_dir" => Ok(RiskTier::Tier1),

            // Tier 2: Destructive/irreversible operations
            "delete_file" | "git_push" | "execute_command" | "git_reset" => Ok(RiskTier::Tier2),

            // Unknown operation
            _ => Err(EngineError::UnknownOperation(operation_name.to_string())),
        }
    }

    /// Check if arguments contain dangerous flags
    ///
    /// Dangerous flags include:
    /// - --force
    /// - -rf
    /// - --delete
    /// - --hard
    ///
    /// # Arguments
    ///
    /// * `args` - The arguments to check
    ///
    /// # Returns
    ///
    /// `true` if any dangerous flags are present, `false` otherwise
    fn has_dangerous_flags(&self, args: &[String]) -> bool {
        const DANGEROUS_FLAGS: &[&str] = &["--force", "-rf", "--delete", "--hard"];

        args.iter()
            .any(|arg| DANGEROUS_FLAGS.iter().any(|flag| arg.contains(flag)))
    }
}

impl Default for RiskAssessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_tier_escalate() {
        assert_eq!(RiskTier::Tier0.escalate(), RiskTier::Tier1);
        assert_eq!(RiskTier::Tier1.escalate(), RiskTier::Tier2);
        assert_eq!(RiskTier::Tier2.escalate(), RiskTier::Tier2);
    }

    #[test]
    fn test_operation_source_is_remote() {
        assert!(!OperationSource::Local.is_remote());
        assert!(OperationSource::Remote.is_remote());
    }

    #[test]
    fn test_classify_tier0_operations() {
        let assessor = RiskAssessor::new();

        let operations = vec!["read_file", "list_dir", "git_status", "git_log"];

        for op_name in operations {
            let op = Operation::new(op_name, vec![], OperationSource::Local);
            let tier = assessor.assess(&op).unwrap();
            assert_eq!(
                tier,
                RiskTier::Tier0,
                "Operation {} should be Tier 0",
                op_name
            );
        }
    }

    #[test]
    fn test_classify_tier1_operations() {
        let assessor = RiskAssessor::new();

        let operations = vec!["write_file", "git_add", "git_commit", "create_dir"];

        for op_name in operations {
            let op = Operation::new(op_name, vec![], OperationSource::Local);
            let tier = assessor.assess(&op).unwrap();
            assert_eq!(
                tier,
                RiskTier::Tier1,
                "Operation {} should be Tier 1",
                op_name
            );
        }
    }

    #[test]
    fn test_classify_tier2_operations() {
        let assessor = RiskAssessor::new();

        let operations = vec!["delete_file", "git_push", "execute_command", "git_reset"];

        for op_name in operations {
            let op = Operation::new(op_name, vec![], OperationSource::Local);
            let tier = assessor.assess(&op).unwrap();
            assert_eq!(
                tier,
                RiskTier::Tier2,
                "Operation {} should be Tier 2",
                op_name
            );
        }
    }

    #[test]
    fn test_unknown_operation() {
        let assessor = RiskAssessor::new();
        let op = Operation::new("unknown_operation", vec![], OperationSource::Local);
        let result = assessor.assess(&op);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::UnknownOperation(_)
        ));
    }

    #[test]
    fn test_dangerous_flag_force() {
        let assessor = RiskAssessor::new();
        let op = Operation::new(
            "git_push",
            vec!["--force".to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_dangerous_flag_rf() {
        let assessor = RiskAssessor::new();
        let op = Operation::new(
            "execute_command",
            vec!["rm".to_string(), "-rf".to_string(), "/tmp".to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_dangerous_flag_delete() {
        let assessor = RiskAssessor::new();
        let op = Operation::new(
            "git_push",
            vec!["--delete".to_string(), "branch".to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_dangerous_flag_hard() {
        let assessor = RiskAssessor::new();
        let op = Operation::new(
            "git_reset",
            vec!["--hard".to_string(), "HEAD~1".to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_dangerous_flag_escalates_tier0() {
        let assessor = RiskAssessor::new();
        // Even a read operation with dangerous flags becomes Tier 2
        let op = Operation::new(
            "read_file",
            vec!["--force".to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_dangerous_flag_escalates_tier1() {
        let assessor = RiskAssessor::new();
        // Write operation with dangerous flags becomes Tier 2
        let op = Operation::new(
            "write_file",
            vec!["--force".to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_remote_source_always_tier2() {
        let assessor = RiskAssessor::new();

        // Remote source ALWAYS Tier2, regardless of operation type
        // Spec: "Remote source always → Tier 2 regardless of action type"

        // Read operation from remote → Tier2 (not Tier0)
        let op = Operation::new("read_file", vec![], OperationSource::Remote);
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);

        // Write operation from remote → Tier2 (not Tier1)
        let op = Operation::new("write_file", vec![], OperationSource::Remote);
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);

        // Delete operation from remote → Tier2 (already Tier2)
        let op = Operation::new("delete_file", vec![], OperationSource::Remote);
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_remote_with_dangerous_flag() {
        let assessor = RiskAssessor::new();
        // Remote + dangerous flag = Tier 2
        let op = Operation::new(
            "write_file",
            vec!["--force".to_string()],
            OperationSource::Remote,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_multiple_args_no_dangerous_flags() {
        let assessor = RiskAssessor::new();
        let op = Operation::new(
            "write_file",
            vec![
                "file.txt".to_string(),
                "--verbose".to_string(),
                "--output".to_string(),
            ],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier1);
    }

    #[test]
    fn test_dangerous_flag_in_middle_of_arg() {
        let assessor = RiskAssessor::new();
        // Flag contained within another string should still be detected
        let op = Operation::new(
            "git_push",
            vec!["origin".to_string(), "main--force".to_string()],
            OperationSource::Local,
        );
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier2);
    }

    #[test]
    fn test_empty_args() {
        let assessor = RiskAssessor::new();
        let op = Operation::new("read_file", vec![], OperationSource::Local);
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier0);
    }

    #[test]
    fn test_operation_new() {
        let op = Operation::new(
            "test_op",
            vec!["arg1".to_string(), "arg2".to_string()],
            OperationSource::Local,
        );
        assert_eq!(op.name, "test_op");
        assert_eq!(op.args.len(), 2);
        assert_eq!(op.source, OperationSource::Local);
    }

    #[test]
    fn test_risk_assessor_default() {
        let assessor = RiskAssessor::default();
        let op = Operation::new("read_file", vec![], OperationSource::Local);
        let tier = assessor.assess(&op).unwrap();
        assert_eq!(tier, RiskTier::Tier0);
    }
}
