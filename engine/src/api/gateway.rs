//! Gateway — durable inbox coordination.

mod locks;
mod poller;
mod submission;
mod task;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::agent::AgentCore;
use crate::db::Database;
use crate::injection_detector::InjectionDetector;
use brain::dispatch::DispatchBrain;

pub use locks::WorkspaceLocks;
pub use task::Task;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub poll_interval_ms: u64,
    pub poll_limit: i64,
    pub cli_password: Option<String>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 200,
            poll_limit: 10,
            cli_password: None,
        }
    }
}

impl GatewayConfig {
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            poll_interval_ms: config.gateway.poll_interval_ms.unwrap_or(200),
            poll_limit: config.gateway.poll_limit.unwrap_or(10),
            cli_password: config.gateway.cli_password.clone(),
        }
    }
}

pub struct Gateway {
    db: Arc<Database>,
    config: GatewayConfig,
    injection_detector: InjectionDetector,
    dispatch_brain: DispatchBrain,
}

impl Gateway {
    pub fn new(db: Arc<Database>, config: GatewayConfig) -> anyhow::Result<Self> {
        let injection_detector = InjectionDetector::new().map_err(|error| {
            anyhow::anyhow!("Failed to initialize injection detector: {}", error)
        })?;
        let dispatch_brain = DispatchBrain::init()
            .map_err(|error| anyhow::anyhow!("Failed to initialize dispatch brain: {}", error))?;

        Ok(Self {
            db,
            config,
            injection_detector,
            dispatch_brain,
        })
    }

    pub fn start(self: Arc<Self>, agent: Arc<RwLock<AgentCore>>) {
        tokio::spawn(async move {
            self.run(agent).await;
        });
        info!("Gateway poll loop started");
    }
}

pub async fn recover_crashed_tasks(db: &Database) -> anyhow::Result<usize> {
    let repo = db.pending_tasks();
    let recovered = repo.recover_crashed_tasks().await?;

    if recovered > 0 {
        info!(
            "Recovered {} crashed task(s) — marked as pending",
            recovered
        );
    }

    Ok(recovered)
}
