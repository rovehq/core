use std::process::Command;
use tracing::debug;

use sdk::errors::EngineError;

use super::{McpSandbox, SandboxProfile};

impl McpSandbox {
    pub(super) fn wrap_macos_seatbelt(
        cmd: &str,
        args: &[String],
        profile: &SandboxProfile,
    ) -> Result<Command, EngineError> {
        debug!("Wrapping MCP command with Seatbelt sandbox");

        let mut sb_profile = String::from("(version 1)\n(deny default)\n");
        sb_profile.push_str(&format!("(allow process-exec (literal \"{}\"))\n", cmd));
        sb_profile.push_str("(allow file-read* (subpath \"/usr\"))\n");
        sb_profile.push_str("(allow file-read* (subpath \"/System\"))\n");
        sb_profile.push_str("(allow file-read* (subpath \"/Library\"))\n");

        if profile.allow_network {
            sb_profile.push_str("(allow network*)\n");
        }

        for path in &profile.read_paths {
            if path.exists() {
                sb_profile.push_str(&format!(
                    "(allow file-read* (subpath \"{}\"))\n",
                    path.display()
                ));
            }
        }

        for path in &profile.write_paths {
            if path.exists() {
                sb_profile.push_str(&format!(
                    "(allow file-write* (subpath \"{}\"))\n",
                    path.display()
                ));
            }
        }

        if profile.allow_tmp {
            sb_profile.push_str("(allow file-read* (subpath \"/tmp\"))\n");
            sb_profile.push_str("(allow file-write* (subpath \"/tmp\"))\n");
        }

        let profile_path =
            std::env::temp_dir().join(format!("rove_mcp_{}.sb", std::process::id()));
        std::fs::write(&profile_path, sb_profile).map_err(EngineError::Io)?;

        let mut command = Command::new("sandbox-exec");
        command.arg("-f");
        command.arg(&profile_path);
        command.arg(cmd);
        command.args(args);
        Ok(command)
    }
}
