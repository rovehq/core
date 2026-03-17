use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::timeout;

use super::{DaemonManager, Result};
use crate::config::Config;
use sdk::errors::EngineError;

impl DaemonManager {
    pub async fn wait_for_shutdown(&self, timeout_duration: Duration) -> Result<()> {
        let result = timeout(timeout_duration, async {
            while !self.shutdown_flag.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(_) => Err(EngineError::Config("Shutdown timeout exceeded".to_string())),
        }
    }

    pub fn signal_shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
    }

    pub async fn graceful_shutdown(&mut self, _config: &Config) -> Result<()> {
        tracing::info!("Starting graceful shutdown");
        self.signal_shutdown();
        tracing::info!("Shutdown flag set - refusing new tasks");

        match self.wait_for_shutdown(Duration::from_secs(30)).await {
            Ok(_) => tracing::info!("All in-progress tasks completed"),
            Err(_) => tracing::warn!("Timeout waiting for tasks - proceeding with shutdown"),
        }

        if let Some(native_runtime) = &self.native_runtime {
            tracing::info!("Stopping all core tools");
            native_runtime.lock().await.unload_all();
            tracing::info!("All core tools stopped");
        }

        if let Some(wasm_runtime) = &self.wasm_runtime {
            tracing::info!("Closing all plugins");
            wasm_runtime.lock().await.unload_all();
            tracing::info!("All plugins closed");
        }

        if let Some(database) = &self.database {
            tracing::info!("Flushing SQLite WAL");
            match database.flush_wal().await {
                Ok(_) => tracing::info!("SQLite WAL flushed successfully"),
                Err(error) => tracing::error!("Failed to flush SQLite WAL: {}", error),
            }
        }

        if self.pid_file.exists() {
            tracing::info!("Removing PID file");
            match std::fs::remove_file(&self.pid_file) {
                Ok(_) => tracing::info!("PID file removed successfully"),
                Err(error) => tracing::error!("Failed to remove PID file: {}", error),
            }
        }

        tracing::info!("Graceful shutdown completed");
        Ok(())
    }
}
