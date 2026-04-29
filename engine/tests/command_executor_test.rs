//! Tests for security::command_executor — CommandExecutor, CommandError, validate(), execute()

use rove_engine::security::command_executor::{CommandError, CommandExecutor};

// ── Construction ──────────────────────────────────────────────────────────────

#[test]
fn executor_new_constructs() {
    let _ = CommandExecutor::new();
}

#[test]
fn executor_default_constructs() {
    let _ = CommandExecutor::default();
}

#[test]
fn executor_with_allowlist_constructs() {
    let _ = CommandExecutor::with_allowlist(vec!["git".to_string()]);
}

#[test]
fn executor_with_empty_allowlist() {
    let _ = CommandExecutor::with_allowlist(vec![]);
}

// ── allow_command / disallow_command ─────────────────────────────────────────

#[test]
fn allow_command_then_validate_passes() {
    let mut executor = CommandExecutor::with_allowlist(vec![]);
    executor.allow_command("git".to_string());
    let result = executor.validate("git", &["--version".to_string()]);
    assert!(result.is_ok(), "Expected allowed after allow_command");
}

#[test]
fn disallow_command_then_validate_fails() {
    let mut executor = CommandExecutor::with_allowlist(vec!["cargo".to_string()]);
    executor.disallow_command("cargo");
    let result = executor.validate("cargo", &["--version".to_string()]);
    assert!(result.is_err());
}

#[test]
fn disallow_nonexistent_is_noop() {
    let mut executor = CommandExecutor::new();
    // Should not panic
    executor.disallow_command("nonexistent_xyz");
}

// ── validate: shell interpreter blocking (Gate 1) ─────────────────────────────

#[test]
fn validate_bash_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("bash", &["-c".to_string(), "echo hi".to_string()]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_sh_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("sh", &["-c".to_string(), "ls".to_string()]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_zsh_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("zsh", &[]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_fish_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("fish", &[]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_dash_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("dash", &[]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_eval_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("eval", &["echo hi".to_string()]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_exec_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("exec", &["ls".to_string()]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_powershell_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("powershell", &[]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

#[test]
fn validate_pwsh_is_blocked() {
    let executor = CommandExecutor::new();
    let result = executor.validate("pwsh", &[]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::ShellInjectionAttempt
    ));
}

// ── validate: command not allowed (Gate 2) ────────────────────────────────────

#[test]
fn validate_rm_not_in_allowlist() {
    let executor = CommandExecutor::new();
    let result = executor.validate("rm", &["-rf".to_string()]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::CommandNotAllowed(_)
    ));
}

#[test]
fn validate_unknown_command_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("unknown_xyz", &[]);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        CommandError::CommandNotAllowed(_)
    ));
}

#[test]
fn validate_curl_not_in_default_allowlist() {
    let executor = CommandExecutor::new();
    let result = executor.validate("curl", &["http://example.com".to_string()]);
    assert!(result.is_err());
}

#[test]
fn validate_sudo_not_in_allowlist() {
    let executor = CommandExecutor::new();
    let result = executor.validate("sudo", &["ls".to_string()]);
    assert!(result.is_err());
}

#[test]
fn validate_empty_command_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("", &[]);
    assert!(result.is_err());
}

// ── validate: allowed commands pass ──────────────────────────────────────────

#[test]
fn validate_git_passes_gate2() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["--version".to_string()]);
    // May fail on metachar check but not on allowlist
    match result {
        Ok(_) => {}
        Err(CommandError::ShellMetacharactersDetected(_)) => {}
        Err(e) => panic!("Unexpected error: {e}"),
    }
}

#[test]
fn validate_cargo_passes_gate2() {
    let executor = CommandExecutor::new();
    let result = executor.validate("cargo", &["--version".to_string()]);
    match result {
        Ok(_) => {}
        Err(CommandError::ShellMetacharactersDetected(_)) => {}
        Err(e) => panic!("Unexpected error: {e}"),
    }
}

#[test]
fn validate_rg_passes_gate2() {
    let executor = CommandExecutor::new();
    let result = executor.validate("rg", &["fn".to_string(), "src/".to_string()]);
    match result {
        Ok(_) => {}
        Err(CommandError::ShellMetacharactersDetected(_)) => {}
        Err(e) => panic!("Unexpected error: {e}"),
    }
}

// ── validate: shell metacharacters in args (Gate 3) ──────────────────────────

#[test]
fn validate_pipe_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["| cat".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_semicolon_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["; rm -rf /".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_backtick_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["`ls`".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_dollar_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["$(id)".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_ampersand_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["& whoami".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_newline_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["status\nrm -rf /".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_single_quote_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["'main'".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_double_quote_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["\"main\"".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_redirect_gt_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["> /etc/passwd".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn validate_redirect_lt_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["< /etc/shadow".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

// ── validate: safe arguments pass Gate 3 ─────────────────────────────────────

#[test]
fn validate_clean_git_status_passes() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["status".to_string()]);
    assert!(result.is_ok());
}

#[test]
fn validate_clean_cargo_check_passes() {
    let executor = CommandExecutor::new();
    let result = executor.validate("cargo", &["check".to_string()]);
    assert!(result.is_ok());
}

#[test]
fn validate_clean_rg_with_path_passes() {
    let executor = CommandExecutor::new();
    let result = executor.validate("rg", &["fn main".to_string(), "src".to_string()]);
    assert!(result.is_ok());
}

#[test]
fn validate_git_log_oneline_passes() {
    let executor = CommandExecutor::new();
    let result = executor.validate("git", &["log".to_string(), "--oneline".to_string()]);
    assert!(result.is_ok());
}

// ── execute: shell blocked ────────────────────────────────────────────────────

#[test]
fn execute_bash_is_error() {
    let executor = CommandExecutor::new();
    let result = executor.execute("bash", &["-c".to_string(), "echo test".to_string()]);
    assert!(result.is_err());
}

#[test]
fn execute_sh_is_error() {
    let executor = CommandExecutor::new();
    let result = executor.execute("sh", &["-c".to_string(), "echo test".to_string()]);
    assert!(result.is_err());
}

#[test]
fn execute_rm_not_allowed() {
    let executor = CommandExecutor::new();
    let result = executor.execute("rm", &["-rf".to_string()]);
    assert!(matches!(result, Err(CommandError::CommandNotAllowed(_))));
}

#[test]
fn execute_with_pipe_in_arg_rejected() {
    let executor = CommandExecutor::new();
    let result = executor.execute("git", &["| cat /etc/passwd".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

// ── CommandError Display ──────────────────────────────────────────────────────

#[test]
fn error_not_allowed_displays_command() {
    let err = CommandError::CommandNotAllowed("rm".to_string());
    let s = err.to_string();
    assert!(s.contains("rm"));
}

#[test]
fn error_shell_injection_displays() {
    let err = CommandError::ShellInjectionAttempt;
    let s = err.to_string();
    assert!(!s.is_empty());
}

#[test]
fn error_metacharacters_displays_arg() {
    let err = CommandError::ShellMetacharactersDetected("| evil".to_string());
    let s = err.to_string();
    assert!(s.contains("evil"));
}

#[test]
fn error_dangerous_pipe_displays() {
    let err = CommandError::DangerousPipeDetected;
    let s = err.to_string();
    assert!(!s.is_empty());
}

// ── with_allowlist isolation ───────────────────────────────────────────────────

#[test]
fn custom_allowlist_blocks_default_commands() {
    let executor = CommandExecutor::with_allowlist(vec!["git".to_string()]);
    let result = executor.validate("cargo", &["check".to_string()]);
    assert!(result.is_err());
}

#[test]
fn custom_allowlist_allows_only_listed() {
    let executor = CommandExecutor::with_allowlist(vec!["rg".to_string()]);
    let result = executor.validate("rg", &["fn".to_string()]);
    assert!(result.is_ok());
}

#[test]
fn multiple_custom_commands_allowed() {
    let executor = CommandExecutor::with_allowlist(vec!["git".to_string(), "cargo".to_string()]);
    assert!(executor.validate("git", &["status".to_string()]).is_ok());
    assert!(executor.validate("cargo", &["check".to_string()]).is_ok());
    assert!(executor.validate("rg", &["fn".to_string()]).is_err());
}

// ── Clone ─────────────────────────────────────────────────────────────────────

#[test]
fn executor_can_be_cloned() {
    let e1 = CommandExecutor::new();
    let e2 = e1.clone();
    // Both should reject rm
    assert!(e2.validate("rm", &[]).is_err());
}

#[test]
fn executor_debug_format() {
    let e = CommandExecutor::new();
    let s = format!("{:?}", e);
    assert!(!s.is_empty());
}
