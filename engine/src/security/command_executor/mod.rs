use std::collections::HashSet;
use std::process::{Command, Output, Stdio};
use thiserror::Error;

/// CommandExecutor provides secure command execution with allowlist validation
/// and shell injection prevention.
///
/// # Security Features
/// - Allowlist-based command validation with absolute path pinning
/// - Shell pattern rejection (sh -c, bash -c)
/// - Shell metacharacter detection
/// - Dangerous pipe pattern detection
/// - execve-style execution (no shell)
/// - stdin set to null, stdout/stderr piped
#[derive(Debug, Clone)]
pub struct CommandExecutor {
    /// Maps command name -> absolute path (e.g. "git" -> "/usr/bin/git")
    allowlist: HashSet<String>,
    resolved: std::collections::HashMap<String, String>,
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("Command not allowed: {0}")]
    CommandNotAllowed(String),

    #[error("Shell invocation attempt detected")]
    ShellInjectionAttempt,

    #[error("Shell metacharacters detected in argument: {0}")]
    ShellMetacharactersDetected(String),

    #[error("Dangerous pipe pattern detected")]
    DangerousPipeDetected,

    #[error("Command execution failed: {0}")]
    ExecutionFailed(#[from] std::io::Error),
}

/// Resolve a command name to its absolute path using `which`
fn resolve_path(cmd: &str) -> Option<String> {
    Command::new("which")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}

impl CommandExecutor {
    /// Creates a new CommandExecutor with a hardened allowlist.
    ///
    /// Commands are resolved to absolute paths at construction time
    /// to prevent PATH hijacking. Only explicitly allowed commands can execute.
    ///
    /// Allowed commands (per spec):
    /// - git, cargo, npm, npx, node
    /// - python3, python, pip3, pip
    /// - make, cmake, rustc, rustfmt
    /// - rg, fd, bat
    pub fn new() -> Self {
        let safe_commands = [
            // Version control
            "git", // Rust toolchain
            "cargo", "rustc", "rustfmt", // Node.js ecosystem
            "npm", "npx", "node", // Python ecosystem
            "python3", "python", "pip3", "pip", // Build tools
            "make", "cmake", // Modern CLI tools
            "rg", "fd", "bat",
        ];

        let mut allowlist = HashSet::new();
        let mut resolved = std::collections::HashMap::new();

        for cmd in &safe_commands {
            allowlist.insert(cmd.to_string());
            if let Some(abs_path) = resolve_path(cmd) {
                resolved.insert(cmd.to_string(), abs_path);
            }
        }

        Self {
            allowlist,
            resolved,
        }
    }

    /// Creates a CommandExecutor with a custom allowlist.
    pub fn with_allowlist(commands: Vec<String>) -> Self {
        let mut resolved = std::collections::HashMap::new();
        for cmd in &commands {
            if let Some(abs_path) = resolve_path(cmd) {
                resolved.insert(cmd.clone(), abs_path);
            }
        }
        Self {
            allowlist: commands.into_iter().collect(),
            resolved,
        }
    }

    /// Adds a command to the allowlist.
    pub fn allow_command(&mut self, command: String) {
        if let Some(abs_path) = resolve_path(&command) {
            self.resolved.insert(command.clone(), abs_path);
        }
        self.allowlist.insert(command);
    }

    /// Removes a command from the allowlist.
    pub fn disallow_command(&mut self, command: &str) {
        self.allowlist.remove(command);
        self.resolved.remove(command);
    }

    /// Get the absolute path for a command, falling back to the bare name
    fn abs_path(&self, command: &str) -> String {
        self.resolved
            .get(command)
            .cloned()
            .unwrap_or_else(|| command.to_string())
    }

    /// Validates a command through all security gates without executing it.
    ///
    /// This is used by `TerminalTool` to validate commands before executing
    /// them with a custom working directory.
    pub fn validate(&self, command: &str, args: &[String]) -> Result<(), CommandError> {
        // Gate 1: Block shell interpreters explicitly
        const BLOCKED_SHELLS: &[&str] = &[
            "bash",
            "sh",
            "zsh",
            "fish",
            "dash",
            "cmd",
            "powershell",
            "pwsh",
            "eval",
            "exec",
        ];

        let command_lower = command.to_lowercase();
        if BLOCKED_SHELLS.contains(&command_lower.as_str()) {
            return Err(CommandError::ShellInjectionAttempt);
        }

        // Gate 2: Validate command is in allowlist (case-insensitive)
        if !self.allowlist.contains(&command_lower) {
            return Err(CommandError::CommandNotAllowed(command.to_string()));
        }

        // Gate 3: Check for shell metacharacters in arguments
        for arg in args {
            if self.has_shell_metacharacters(arg) {
                return Err(CommandError::ShellMetacharactersDetected(arg.clone()));
            }
        }

        // Gate 4: Reject dangerous piping patterns
        let full_command = format!("{} {}", command, args.join(" "));
        if self.has_dangerous_pipe(&full_command) {
            return Err(CommandError::DangerousPipeDetected);
        }

        Ok(())
    }

    /// Executes a command with security validation.
    ///
    /// # Security Gates
    /// 1. Block shell interpreters explicitly (bash, sh, zsh, fish, dash, cmd, powershell, pwsh, eval, exec)
    /// 2. Validate command is in allowlist
    /// 3. Check for shell metacharacters in arguments
    /// 4. Detect dangerous piping patterns
    ///
    /// # Execution
    /// - Uses execve-style execution (no shell)
    /// - stdin set to null
    /// - stdout and stderr piped
    ///
    /// # Requirements
    /// - Requirement 8.1: Uses execve-style command execution
    /// - Requirement 8.4: Validates commands against allowlist
    /// - Requirement 8.5: Sets stdin to null, stdout/stderr to piped
    pub fn execute(&self, command: &str, args: &[String]) -> Result<Output, CommandError> {
        // Gate 1: Block shell interpreters explicitly
        const BLOCKED_SHELLS: &[&str] = &[
            "bash",
            "sh",
            "zsh",
            "fish",
            "dash",
            "cmd",
            "powershell",
            "pwsh",
            "eval",
            "exec",
        ];

        let command_lower = command.to_lowercase();
        if BLOCKED_SHELLS.contains(&command_lower.as_str()) {
            return Err(CommandError::ShellInjectionAttempt);
        }

        // Gate 2: Validate command is in allowlist (case-insensitive)
        if !self.allowlist.contains(&command_lower) {
            return Err(CommandError::CommandNotAllowed(command.to_string()));
        }

        // Gate 3: Check for shell metacharacters in arguments
        for arg in args {
            if self.has_shell_metacharacters(arg) {
                return Err(CommandError::ShellMetacharactersDetected(arg.clone()));
            }
        }

        // Gate 4: Reject dangerous piping patterns
        let full_command = format!("{} {}", command, args.join(" "));
        if self.has_dangerous_pipe(&full_command) {
            return Err(CommandError::DangerousPipeDetected);
        }

        // Execute with execve-style (no shell)
        // Uses absolute path to prevent PATH hijacking
        let abs = self.abs_path(command);
        let output = Command::new(&abs)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;

        Ok(output)
    }

    /// Checks if a string contains shell metacharacters.
    ///
    /// Detects: ; | & $ < > ` ' " \n \r !
    ///
    /// # Requirement
    /// - Requirement 8.3: Rejects shell metacharacters in user input
    fn has_shell_metacharacters(&self, s: &str) -> bool {
        s.chars().any(|c| {
            matches!(
                c,
                ';' | '|' | '&' | '$' | '<' | '>' | '`' | '\'' | '"' | '\n' | '\r' | '!'
            )
        })
    }

    /// Checks if a command contains dangerous piping patterns.
    ///
    /// Detects patterns like:
    /// - | sudo
    /// - | su
    /// - | chmod 777
    /// - curl | bash
    /// - wget | sh
    ///
    /// # Requirement
    /// - Requirement 8.7: Rejects dangerous piping patterns
    fn has_dangerous_pipe(&self, cmd: &str) -> bool {
        const DANGEROUS: &[&str] = &[
            "| sudo",
            "| su",
            "| chmod 777",
            "curl | bash",
            "wget | sh",
            "curl | sh",
            "wget | bash",
        ];
        DANGEROUS.iter().any(|d| cmd.contains(d))
    }
}

impl Default for CommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_allowed_command_executes() {
        let executor = CommandExecutor::new();
        let result = executor.execute("git", &["--version".to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_disallowed_command_rejected() {
        let executor = CommandExecutor::new();
        let result = executor.execute("rm", &["-rf".to_string(), "/".to_string()]);
        assert!(matches!(result, Err(CommandError::CommandNotAllowed(_))));
    }

    #[test]
    fn test_shell_invocation_rejected() {
        let executor = CommandExecutor::new();
        let result = executor.execute("sh", &["-c".to_string(), "echo hello".to_string()]);
        // sh is not in allowlist, so it will be rejected as CommandNotAllowed first
        // But we also check for shell invocation, so either error is acceptable
        assert!(result.is_err());
        match result {
            Err(CommandError::CommandNotAllowed(_)) | Err(CommandError::ShellInjectionAttempt) => {}
            _ => panic!("Expected CommandNotAllowed or ShellInjectionAttempt error"),
        }
    }

    #[test]
    fn test_bash_invocation_rejected() {
        let executor = CommandExecutor::new();
        let result = executor.execute("bash", &["-c".to_string(), "echo hello".to_string()]);
        // bash is not in allowlist, so it will be rejected as CommandNotAllowed first
        // But we also check for shell invocation, so either error is acceptable
        assert!(result.is_err());
        match result {
            Err(CommandError::CommandNotAllowed(_)) | Err(CommandError::ShellInjectionAttempt) => {}
            _ => panic!("Expected CommandNotAllowed or ShellInjectionAttempt error"),
        }
    }

    #[test]
    fn test_shell_metacharacters_detected() {
        let executor = CommandExecutor::new();

        // Test pipe character
        let result = executor.execute("git", &["| cat".to_string()]);
        assert!(matches!(
            result,
            Err(CommandError::ShellMetacharactersDetected(_))
        ));

        // Test semicolon
        let result = executor.execute("git", &["; rm -rf /".to_string()]);
        assert!(matches!(
            result,
            Err(CommandError::ShellMetacharactersDetected(_))
        ));

        // Test backtick
        let result = executor.execute("git", &["`whoami`".to_string()]);
        assert!(matches!(
            result,
            Err(CommandError::ShellMetacharactersDetected(_))
        ));
    }

    #[test]
    fn test_dangerous_pipe_detected() {
        let executor = CommandExecutor::new();

        // Test pipe character in arguments (should be caught by metacharacter check)
        let result = executor.execute(
            "git",
            &["/tmp".to_string(), "|".to_string(), "bash".to_string()],
        );
        // This will be caught by shell metacharacter detection
        assert!(matches!(
            result,
            Err(CommandError::ShellMetacharactersDetected(_))
        ));
    }

    #[test]
    #[cfg(unix)]
    fn test_custom_allowlist() {
        let mut executor = CommandExecutor::with_allowlist(vec!["cat".to_string()]);

        // cat should work
        let result = executor.execute("cat", &["/dev/null".to_string()]);
        assert!(result.is_ok());

        // ls should not work (not in custom allowlist)
        let result = executor.execute("git", &[]);
        assert!(matches!(result, Err(CommandError::CommandNotAllowed(_))));

        // Add git to allowlist
        executor.allow_command("git".to_string());
        let result = executor.execute("git", &["--version".to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_stdin_null_stdout_stderr_piped() {
        let executor = CommandExecutor::new();
        let result = executor.execute("git", &["--version".to_string()]);

        // Should succeed and capture output
        assert!(result.is_ok());
        let output = result.unwrap();

        // stdout should contain something
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.is_empty());
    }
}
