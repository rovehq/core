use std::time::Duration;

use anyhow::Result;
use sdk::core_tool::{CoreContext, CoreTool};
use sdk::errors::EngineError;
use sdk::tool_io::{ToolInput, ToolOutput};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct TerminalTool {
    work_dir: String,
    timeout: Duration,
    executor: CommandExecutor,
}

impl TerminalTool {
    pub fn new(work_dir: String) -> Self {
        Self {
            work_dir,
            timeout: Duration::from_secs(60),
            executor: CommandExecutor::new(),
        }
    }

    pub async fn execute(&self, command: &str) -> Result<String> {
        info!("Executing terminal command: {}", command);
        let Some(parts) = shlex::split(command) else {
            return Err(anyhow::anyhow!("Invalid shell-style quoting in command"));
        };
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty command"));
        }

        let program = parts[0].clone();
        let args: Vec<String> = parts[1..].to_vec();
        let executor = self.executor.clone();
        let program_owned = program;
        let work_dir = self.work_dir.clone();
        let timeout = self.timeout;

        let result = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                use std::process::{Command, Stdio};

                executor
                    .validate(&program_owned, &args)
                    .map_err(|e| anyhow::anyhow!("Command rejected: {}", e))?;

                let output = Command::new(executor.abs_path(&program_owned))
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

impl CoreTool for TerminalTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError> {
        if let Some(workspace) = ctx.config.get("core.workspace").and_then(|v| v.as_str().map(ToOwned::to_owned)) {
            self.work_dir = workspace;
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        Ok(())
    }

    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError> {
        let command = match input.method.as_str() {
            "run_command" => input.param_str("command").map_err(tool_input_error)?,
            other => {
                return Err(EngineError::ToolError(format!(
                    "Unknown terminal method '{}'",
                    other
                )))
            }
        };
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|error| EngineError::ToolError(error.to_string()))?;
        let output = runtime
            .block_on(self.execute(&command))
            .map_err(|error| EngineError::ToolError(error.to_string()))?;
        Ok(ToolOutput::json(serde_json::json!(output)))
    }
}

#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn create_tool() -> *mut dyn CoreTool {
    let work_dir = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Box::into_raw(Box::new(TerminalTool::new(work_dir)))
}

fn tool_input_error(error: sdk::tool_io::ToolError) -> EngineError {
    EngineError::ToolError(error.to_string())
}

#[derive(Debug, Clone)]
struct CommandExecutor {
    allowlist: std::collections::HashSet<String>,
    resolved: std::collections::HashMap<String, String>,
}

impl CommandExecutor {
    fn new() -> Self {
        let safe_commands = [
            "git", "cargo", "rustc", "rustfmt", "npm", "npx", "node", "python3", "python",
            "pip3", "pip", "make", "cmake", "rg", "fd", "bat",
        ];

        let mut allowlist = std::collections::HashSet::new();
        let mut resolved = std::collections::HashMap::new();
        for cmd in &safe_commands {
            allowlist.insert((*cmd).to_string());
            if let Some(path) = resolve_path(cmd) {
                resolved.insert((*cmd).to_string(), path);
            }
        }
        Self { allowlist, resolved }
    }

    fn validate(&self, command: &str, args: &[String]) -> Result<(), anyhow::Error> {
        const BLOCKED_SHELLS: &[&str] = &[
            "bash", "sh", "zsh", "fish", "dash", "cmd", "powershell", "pwsh", "eval", "exec",
        ];
        let command_lower = command.to_lowercase();
        if BLOCKED_SHELLS.contains(&command_lower.as_str()) {
            anyhow::bail!("Shell invocation attempt detected");
        }
        if !self.allowlist.contains(&command_lower) {
            anyhow::bail!("Command not allowed: {}", command);
        }
        if args.iter().any(|arg| has_shell_metacharacters(arg)) {
            anyhow::bail!("Shell metacharacters detected in arguments");
        }
        if has_dangerous_pipe(&format!("{} {}", command, args.join(" "))) {
            anyhow::bail!("Dangerous pipe pattern detected");
        }
        Ok(())
    }

    fn abs_path(&self, command: &str) -> String {
        self.resolved
            .get(command)
            .cloned()
            .unwrap_or_else(|| command.to_string())
    }
}

fn resolve_path(cmd: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}

fn has_shell_metacharacters(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(
            c,
            ';' | '|' | '&' | '$' | '<' | '>' | '`' | '\'' | '"' | '\n' | '\r' | '!'
        )
    })
}

fn has_dangerous_pipe(s: &str) -> bool {
    s.contains("| sh") || s.contains("| bash") || s.contains("| zsh") || s.contains("| python")
}
