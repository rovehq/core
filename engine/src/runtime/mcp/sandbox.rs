//! Gate 5 — MCP sandbox.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use sdk::errors::EngineError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxProfile {
    pub allow_network: bool,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub allow_tmp: bool,
}

impl SandboxProfile {
    pub fn with_network(mut self) -> Self {
        self.allow_network = true;
        self
    }

    pub fn with_read_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.read_paths.push(path.into());
        self
    }

    pub fn with_write_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.write_paths.push(path.into());
        self
    }

    pub fn with_tmp(mut self) -> Self {
        self.allow_tmp = true;
        self
    }
}

pub struct McpSandbox;

impl McpSandbox {
    pub fn wrap_command(
        cmd: &str,
        args: &[String],
        profile: &SandboxProfile,
    ) -> Result<Command, EngineError> {
        #[cfg(target_os = "linux")]
        return Self::wrap_linux_bubblewrap(cmd, args, profile);

        #[cfg(target_os = "macos")]
        return Self::wrap_macos_seatbelt(cmd, args, profile);

        #[cfg(target_os = "windows")]
        return Self::wrap_windows_job(cmd, args, profile);

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            warn!("MCP sandbox not supported on this platform, running without sandbox");
            let mut command = Command::new(cmd);
            command.args(args);
            Ok(command)
        }
    }

    pub fn check_availability() -> Result<(), EngineError> {
        #[cfg(target_os = "linux")]
        {
            match Command::new("which").arg("bwrap").output() {
                Ok(output) if output.status.success() => {
                    debug!("Bubblewrap (bwrap) found");
                    Ok(())
                }
                _ => Err(EngineError::Plugin(
                    "Bubblewrap (bwrap) not found. Install: apt install bubblewrap".into(),
                )),
            }
        }

        #[cfg(target_os = "macos")]
        {
            debug!("Seatbelt (sandbox-exec) available on macOS");
            Ok(())
        }

        #[cfg(target_os = "windows")]
        {
            debug!("Job Objects available on Windows");
            Ok(())
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            warn!("MCP sandbox not supported on this platform");
            Ok(())
        }
    }
}
