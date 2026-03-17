use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::task::JoinHandle;

use super::DaemonManager;

impl DaemonManager {
    #[cfg(unix)]
    pub fn setup_signal_handler(shutdown_flag: Arc<AtomicBool>) -> JoinHandle<()> {
        use std::sync::atomic::Ordering;
        use tokio::signal::unix::{signal, SignalKind};

        tokio::spawn(async move {
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(handler) => handler,
                Err(error) => {
                    tracing::error!("Failed to create SIGTERM handler: {}", error);
                    return;
                }
            };

            sigterm.recv().await;
            tracing::info!("Received SIGTERM signal");
            shutdown_flag.store(true, Ordering::Relaxed);
        })
    }

    #[cfg(windows)]
    pub fn setup_signal_handler(_shutdown_flag: Arc<AtomicBool>) -> JoinHandle<()> {
        use std::time::Duration;

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(u64::MAX)).await;
        })
    }
}
