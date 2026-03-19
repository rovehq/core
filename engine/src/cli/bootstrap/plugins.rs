use std::sync::Arc;

use anyhow::Result;

use crate::config::Config;
use crate::runtime::{RuntimeManager, ToolRegistry};
use crate::storage::Database;

pub async fn build(database: &Database, config: &Config) -> Result<Arc<ToolRegistry>> {
    let runtime = RuntimeManager::build(database, config).await?;
    Ok(runtime.registry)
}
