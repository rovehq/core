use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use crate::agent::AgentCore;
use crate::api::gateway::{recover_crashed_tasks, Gateway, GatewayConfig};
use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::Database;

use super::agent::init_agent_with_db;

pub async fn init_daemon() -> Result<(Arc<RwLock<AgentCore>>, Arc<Database>, Arc<Gateway>)> {
    let config = Config::load_or_create()?;
    let db_path = database_path(&config);

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let database = Arc::new(Database::new(&db_path).await?);
    let recovered = recover_crashed_tasks(&database).await?;
    if recovered > 0 {
        tracing::info!("Recovered {} crashed task(s)", recovered);
    }

    let gateway_config = GatewayConfig::from_config(&config);
    let gateway = Arc::new(Gateway::new(database.clone(), gateway_config)?);
    let agent = init_agent_with_db(database.clone()).await?;

    Ok((agent, database, gateway))
}
