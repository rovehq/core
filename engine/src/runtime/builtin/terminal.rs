//! Terminal Core Tool
//!
//! Native execution of shell commands. Unlike WASM plugins, this runs directly
//! on the host OS with the same privileges as the Rove daemon. Execution
//! is routed through `CommandExecutor` for allowlist validation and shell
//! injection prevention.

use anyhow::Result;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::command_executor::CommandExecutor;

#[derive(Debug)]
pub struct TerminalTool {
    work_dir: String,
    timeout: Duration,
    executor: CommandExecutor,
}

impl TerminalTool {
    pub fn new(work_dir: String) -> Self {
        Self {
            work_dir,
            timeout: Duration::from_secs(60), // Default 60s timeout
            executor: CommandExecutor::new(),
        }
    }

    /// Execute a command through the secure CommandExecutor
    ///
    /// The command string is parsed into program + arguments and routed through
    /// `CommandExecutor::execute()` which enforces:
    /// - Allowlist validation
    /// - Shell invocation rejection
    /// - Shell metacharacter detection
    /// - Dangerous pipe pattern detection
    /// - execve-style execution (no shell)
    pub async fn execute(&self, command: &str) -> Result<String> {
        info!("Executing terminal command: {}", command);

        // Parse command into program and arguments
        let Some(parts) = shlex::split(command) else {
            return Err(anyhow::anyhow!("Invalid shell-style quoting in command"));
        };
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty command"));
        }

        let program = parts[0].clone();
        let args: Vec<String> = parts[1..].to_vec();

        // Route through CommandExecutor for security validation
        let executor = self.executor.clone();
        let program_owned = program;
        let work_dir = self.work_dir.clone();
        let timeout = self.timeout;

        let result = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                // Execute with security gates via CommandExecutor
                // We need to set working directory, so we use a modified approach
                use std::process::{Command, Stdio};

                // First validate through CommandExecutor's security gates
                // (allowlist, shell rejection, metachar, pipe detection)
                match executor.validate(&program_owned, &args) {
                    Ok(()) => {}
                    Err(e) => {
                        return Err(anyhow::anyhow!("Command rejected: {}", e));
                    }
                }

                // Execute with working directory set (CommandExecutor doesn't support cwd)
                let output = Command::new(&program_owned)
                    .args(&args)
                    .current_dir(&work_dir)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .map_err(|e| anyhow::anyhow!("Failed to start command: {}", e))?;

                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    if stdout.is_empty() && !stderr.is_empty() {
                        Ok(stderr)
                    } else {
                        Ok(stdout)
                    }
                } else {
                    Err(anyhow::anyhow!(
                        "Command failed with status: {}\nStdout: {}\nStderr: {}",
                        output.status,
                        stdout,
                        stderr
                    ))
                }
            }),
        )
        .await;

        match result {
            Ok(Ok(Ok(output))) => {
                debug!("Command succeeded");
                Ok(output)
            }
            Ok(Ok(Err(e))) => {
                warn!("Command failed: {}", e);
                Err(e)
            }
            Ok(Err(e)) => {
                warn!("Command task panicked: {}", e);
                Err(anyhow::anyhow!("Command execution panicked: {}", e))
            }
            Err(_) => {
                let err_msg = format!("Command timed out after {} seconds", self.timeout.as_secs());
                warn!("{}", err_msg);
                Err(anyhow::anyhow!(err_msg))
            }
        }
    }
}
