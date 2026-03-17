use std::fs;
use std::path::{Path, PathBuf};

use super::{DaemonManager, Result};
use crate::config::Config;
use sdk::errors::EngineError;

impl DaemonManager {
    pub(super) fn is_daemon_running(&self) -> Result<bool> {
        if !self.pid_file.exists() {
            return Ok(false);
        }

        let pid = Self::read_pid_file(&self.pid_file)?;
        if Self::is_process_running(pid) {
            Ok(true)
        } else {
            fs::remove_file(&self.pid_file).map_err(EngineError::Io)?;
            Ok(false)
        }
    }

    pub(super) fn write_pid_file(&self) -> Result<()> {
        let pid = std::process::id();
        if let Some(parent) = self.pid_file.parent() {
            fs::create_dir_all(parent).map_err(EngineError::Io)?;
        }
        fs::write(&self.pid_file, pid.to_string()).map_err(EngineError::Io)?;
        tracing::info!("Wrote PID {} to {:?}", pid, self.pid_file);
        Ok(())
    }

    pub(super) fn read_pid_file(pid_file: &Path) -> Result<u32> {
        fs::read_to_string(pid_file)
            .map_err(EngineError::Io)?
            .trim()
            .parse::<u32>()
            .map_err(|error| EngineError::Config(format!("Invalid PID in file: {}", error)))
    }

    pub(super) fn is_process_running(pid: u32) -> bool {
        #[cfg(unix)]
        {
            use nix::sys::signal::kill;
            use nix::unistd::Pid;

            kill(Pid::from_raw(pid as i32), None).is_ok()
        }

        #[cfg(windows)]
        {
            let _ = pid;
            false
        }
    }

    pub(super) fn get_pid_file_path(config: &Config) -> Result<PathBuf> {
        let mut data_dir = config.core.data_dir.clone();
        if let Some(home) = dirs::home_dir() {
            if data_dir.starts_with("~") {
                if let Ok(stripped) = data_dir.strip_prefix("~") {
                    data_dir = home.join(stripped);
                }
            }
        }
        Ok(data_dir.join("rove.pid"))
    }
}

impl Drop for DaemonManager {
    fn drop(&mut self) {
        if self.pid_file.exists() {
            if let Err(error) = fs::remove_file(&self.pid_file) {
                tracing::warn!("Failed to remove PID file on drop: {}", error);
            }
        }
    }
}
