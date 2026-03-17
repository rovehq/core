use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use crate::agent::AgentCore;
use crate::api::gateway::WorkspaceLocks;
use crate::config::Config;
use crate::llm::router::LLMRouter;
use crate::memory::conductor::MemorySystem;
use crate::security::rate_limiter::RateLimiter;
use crate::security::risk_assessor::RiskAssessor;
use crate::steering::loader::SteeringEngine;
use crate::storage::{Database, TaskRepository};

use super::plugins;
use super::providers;

pub async fn init_agent_with_db(database: Arc<Database>) -> Result<Arc<RwLock<AgentCore>>> {
    let config = Config::load_or_create()?;
    let db_pool = database.pool().clone();

    let (providers, local_brain) = providers::build(&config).await?;
    let router = Arc::new(LLMRouter::with_local_brain(
        providers,
        Arc::new(config.llm.clone()),
        local_brain,
    ));
    let rate_limiter = Arc::new(RateLimiter::new(db_pool.clone()));
    let risk_assessor = RiskAssessor::new();
    let task_repo = Arc::new(TaskRepository::new(db_pool.clone()));
    let memory_system = Arc::new(MemorySystem::new(db_pool, router.clone()));

    {
        let memory_system = memory_system.clone();
        tokio::spawn(async move {
            memory_system
                .start_consolidation_loop(std::time::Duration::from_secs(30 * 60))
                .await;
        });
    }

    let tools = plugins::build(&database, &config).await?;
    let steering = load_steering().await?;
    let workspace_locks = Arc::new(WorkspaceLocks::new());

    let mut agent = AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        tools,
        Some(steering),
        Arc::new(config),
        workspace_locks,
    )?;
    agent.set_memory_system(memory_system);

    Ok(Arc::new(RwLock::new(agent)))
}

async fn load_steering() -> Result<SteeringEngine> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let steering_dir = home_dir.join(".rove").join("steering");
    let mut steering = SteeringEngine::new(&steering_dir).await?;
    steering.load_all_skills().await?;
    Ok(steering)
}
