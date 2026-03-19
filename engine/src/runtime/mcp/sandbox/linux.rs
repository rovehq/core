use std::process::Command;
use tracing::debug;

use sdk::errors::EngineError;

use super::{McpSandbox, SandboxProfile};

impl McpSandbox {
    pub(super) fn wrap_linux_bubblewrap(
        cmd: &str,
        args: &[String],
        profile: &SandboxProfile,
    ) -> Result<Command, EngineError> {
        debug!("Wrapping MCP command with Bubblewrap sandbox");

        let mut command = Command::new("bwrap");
        command.arg("--ro-bind").arg("/usr").arg("/usr");
        command.arg("--ro-bind").arg("/lib").arg("/lib");
        command.arg("--ro-bind").arg("/lib64").arg("/lib64");
        command.arg("--ro-bind").arg("/bin").arg("/bin");
        command.arg("--ro-bind").arg("/sbin").arg("/sbin");
        command.arg("--proc").arg("/proc");
        command.arg("--dev").arg("/dev");
        command.arg("--tmpfs").arg("/tmp");

        if !profile.allow_network {
            command.arg("--unshare-net");
        }
        command.arg("--unshare-pid");

        for path in &profile.read_paths {
            if path.exists() {
                command.arg("--ro-bind");
                command.arg(path);
                command.arg(path);
            }
        }

        for path in &profile.write_paths {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    return Err(EngineError::PathCanonicalization(
                        path.clone(),
                        "write path parent does not exist".to_string(),
                    ));
                }
            }
            command.arg("--bind");
            command.arg(path);
            command.arg(path);
        }

        command.arg(cmd);
        command.args(args);
        Ok(command)
    }
}
