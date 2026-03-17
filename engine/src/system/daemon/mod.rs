//! Daemon lifecycle management.

mod manifest;
mod pid;
mod providers;
mod shutdown;
mod signals;
#[cfg(test)]
mod tests;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::config::Config;
use crate::db::Database;
use crate::runtime::native::NativeRuntime;
use crate::runtime::wasm::WasmRuntime;
use sdk::errors::EngineError;

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Debug, Clone)]
pub struct DaemonStatus {
    pub is_running: bool,
    pub pid: Option<u32>,
    pub pid_file: PathBuf,
    pub providers: ProviderAvailability,
}

#[derive(Debug, Clone)]
pub struct ProviderAvailability {
    pub ollama: bool,
    pub openai: bool,
    pub anthropic: bool,
    pub gemini: bool,
    pub nvidia_nim: bool,
}

pub struct DaemonManager {
    pid_file: PathBuf,
    shutdown_flag: Arc<AtomicBool>,
    #[allow(dead_code)]
    task_handles: Vec<JoinHandle<()>>,
    native_runtime: Option<Arc<tokio::sync::Mutex<NativeRuntime>>>,
    wasm_runtime: Option<Arc<tokio::sync::Mutex<WasmRuntime>>>,
    database: Option<Arc<Database>>,
}

impl DaemonManager {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            pid_file: Self::get_pid_file_path(config)?,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            task_handles: Vec::new(),
            native_runtime: None,
            wasm_runtime: None,
            database: None,
        })
    }

    pub async fn start(&self) -> Result<()> {
        if self.is_daemon_running()? {
            return Err(EngineError::DaemonAlreadyRunning);
        }

        self.write_pid_file()?;
        let _signal_handle = Self::setup_signal_handler(Arc::clone(&self.shutdown_flag));
        tracing::info!("SIGTERM signal handler installed");

        if let Err(error) = Self::verify_manifest_at_startup() {
            tracing::warn!("Manifest verification skipped or failed: {}", error);
            #[cfg(feature = "production")]
            return Err(EngineError::Config(format!(
                "Manifest verification failed: {}",
                error
            )));
        }

        Ok(())
    }

    pub async fn stop(config: &Config) -> Result<()> {
        let pid_file = Self::get_pid_file_path(config)?;
        let pid = Self::read_pid_file(&pid_file)?;

        #[cfg(unix)]
        {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;

            tracing::info!("Sending SIGTERM to daemon process {}", pid);
            kill(Pid::from_raw(pid as i32), Signal::SIGTERM).map_err(|error| {
                EngineError::Io(std::io::Error::other(format!(
                    "Failed to send SIGTERM: {}",
                    error
                )))
            })?;

            tracing::info!("Waiting for daemon to shut down gracefully");
            let wait_result = timeout(Duration::from_secs(35), async {
                loop {
                    if !Self::is_process_running(pid) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
            .await;

            if wait_result.is_err() {
                tracing::warn!("Daemon did not stop within 35 seconds");
            } else {
                tracing::info!("Daemon stopped successfully");
            }

            if pid_file.exists() {
                fs::remove_file(&pid_file).map_err(EngineError::Io)?;
            }

            Ok(())
        }

        #[cfg(windows)]
        {
            let _ = pid_file;
            Err(EngineError::Config(
                "Daemon stop not yet implemented for Windows".to_string(),
            ))
        }
    }

    pub fn status(config: &Config) -> Result<DaemonStatus> {
        let pid_file = Self::get_pid_file_path(config)?;
        let (is_running, pid) = match Self::read_pid_file(&pid_file) {
            Ok(pid) if Self::is_process_running(pid) => (true, Some(pid)),
            _ => (false, None),
        };

        let config_clone = config.clone();
        let providers = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .map_err(|error| format!("Failed to create runtime: {}", error))
                .map(|rt| rt.block_on(DaemonManager::check_provider_availability(&config_clone)))
        })
        .join()
        .map_err(|_| EngineError::Config("Provider check thread panicked".to_string()))?
        .map_err(|error| EngineError::Config(format!("Provider check failed: {}", error)))?;

        Ok(DaemonStatus {
            is_running,
            pid,
            pid_file,
            providers,
        })
    }

    pub fn set_native_runtime(&mut self, runtime: Arc<tokio::sync::Mutex<NativeRuntime>>) {
        self.native_runtime = Some(runtime);
    }

    pub fn set_wasm_runtime(&mut self, runtime: Arc<tokio::sync::Mutex<WasmRuntime>>) {
        self.wasm_runtime = Some(runtime);
    }

    pub fn set_database(&mut self, database: Arc<Database>) {
        self.database = Some(database);
    }

    pub fn is_shutdown_signaled(&self) -> bool {
        self.shutdown_flag
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn pid_file_path(&self) -> &PathBuf {
        &self.pid_file
    }

    pub fn write_pid_file_test(&self) -> Result<()> {
        self.write_pid_file()
    }
}
