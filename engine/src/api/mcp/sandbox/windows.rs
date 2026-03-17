use std::process::Command;
use tracing::{debug, warn};

use sdk::errors::EngineError;

use super::{McpSandbox, SandboxProfile};

impl McpSandbox {
    pub(super) fn wrap_windows_job(
        cmd: &str,
        args: &[String],
        _profile: &SandboxProfile,
    ) -> Result<Command, EngineError> {
        debug!("Wrapping MCP command with Windows Job Object restrictions");

        let mut command = Command::new(cmd);
        command.args(args);

        warn!(
            "Windows Job Object restrictions not yet implemented - MCP server running without sandbox"
        );
        Ok(command)
    }
}
