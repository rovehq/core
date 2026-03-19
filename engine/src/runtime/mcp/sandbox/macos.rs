use std::path::{Path, PathBuf};
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
        let resolved_cmd = resolve_command_path(cmd);
        let exec_literal = seatbelt_string_literal(&resolved_cmd);

        let mut sb_profile = String::from("(version 1)\n(allow default)\n");
        sb_profile.push_str(&format!(
            "(allow process-exec (literal \"{}\"))\n",
            exec_literal
        ));

        if !profile.allow_network {
            sb_profile.push_str("(deny network*)\n");
        }

        for path in &profile.read_paths {
            if path.exists() {
                sb_profile.push_str(&format!(
                    "(allow file-read* (subpath \"{}\"))\n",
                    seatbelt_string_literal(path)
                ));
            }
        }

        for path in &profile.write_paths {
            if path.exists() {
                sb_profile.push_str(&format!(
                    "(allow file-write* (subpath \"{}\"))\n",
                    seatbelt_string_literal(path)
                ));
            }
        }

        let profile_path = std::env::temp_dir().join(format!(
            "rove_mcp_{}_{}.sb",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&profile_path, sb_profile).map_err(EngineError::Io)?;

        let mut command = Command::new("sandbox-exec");
        command.arg("-f");
        command.arg(&profile_path);
        command.arg(&resolved_cmd);
        command.args(args);
        Ok(command)
    }
}

fn seatbelt_string_literal(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn resolve_command_path(cmd: &str) -> PathBuf {
    let candidate = Path::new(cmd);
    if candidate.is_absolute() || cmd.contains(std::path::MAIN_SEPARATOR) {
        return candidate.to_path_buf();
    }

    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(cmd))
                .find(|path| path.exists())
        })
        .unwrap_or_else(|| candidate.to_path_buf())
}
