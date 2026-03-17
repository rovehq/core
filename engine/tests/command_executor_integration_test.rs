#![cfg(unix)]

use rove_engine::command_executor::{CommandError, CommandExecutor};

#[test]
fn test_safe_command_execution() {
    let executor = CommandExecutor::new();

    // Execute a safe command
    let result = executor.execute("git", &["--version".to_string()]);
    assert!(result.is_ok());

    let output = result.unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty());
}

#[test]
fn test_git_command_execution() {
    let executor = CommandExecutor::new();

    // Execute git status (safe read-only command)
    let result = executor.execute("git", &["--version".to_string()]);
    assert!(result.is_ok());

    let output = result.unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("git version"));
}

#[test]
fn test_command_injection_prevention() {
    let executor = CommandExecutor::new();

    // Attempt command injection via semicolon
    let result = executor.execute("git", &["; rm -rf /".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));

    // Attempt command injection via pipe
    let result = executor.execute("git", &["file.txt | rm -rf /".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));

    // Attempt command injection via backticks
    let result = executor.execute("git", &["`whoami`".to_string()]);
    assert!(matches!(
        result,
        Err(CommandError::ShellMetacharactersDetected(_))
    ));
}

#[test]
fn test_dangerous_command_blocked() {
    let executor = CommandExecutor::new();

    // Attempt to execute rm (not in allowlist)
    let result = executor.execute("rm", &["-rf".to_string(), "/tmp/test".to_string()]);
    assert!(matches!(result, Err(CommandError::CommandNotAllowed(_))));

    // Attempt to execute sudo (not in allowlist)
    let result = executor.execute("sudo", &["ls".to_string()]);
    assert!(matches!(result, Err(CommandError::CommandNotAllowed(_))));
}

#[test]
fn test_stdin_null_configuration() {
    let executor = CommandExecutor::with_allowlist(vec!["cat".to_string()]);

    // Execute a command that would normally read from stdin
    // Since stdin is null, it should not hang
    let result = executor.execute("cat", &[]);

    // cat with no args and null stdin should complete immediately
    assert!(result.is_ok());
    let output = result.unwrap();

    // stdout should be empty since there's no input
    assert!(output.stdout.is_empty());
}

#[test]
fn test_custom_allowlist_workflow() {
    // Create executor with only specific commands
    let mut executor =
        CommandExecutor::with_allowlist(vec!["git".to_string(), "cargo".to_string()]);

    // git should work
    let result = executor.execute("git", &["--version".to_string()]);
    assert!(result.is_ok());

    // cargo should work
    let result = executor.execute("cargo", &["--version".to_string()]);
    assert!(result.is_ok());

    // ls should not work
    let result = executor.execute("ls", &[]);
    assert!(matches!(result, Err(CommandError::CommandNotAllowed(_))));

    // Add ls dynamically
    executor.allow_command("ls".to_string());
    let result = executor.execute("ls", &[]);
    assert!(result.is_ok());

    // Remove cargo
    executor.disallow_command("cargo");
    let result = executor.execute("cargo", &["--version".to_string()]);
    assert!(matches!(result, Err(CommandError::CommandNotAllowed(_))));
}
