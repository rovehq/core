use std::sync::Arc;

use anyhow::Result;
use sdk::NodeExecutionRole;
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
    let memory_config = config.memory.clone();
    let execution_role =
        crate::remote::local_execution_role_for_config(&config).unwrap_or(NodeExecutionRole::Full);
    let agent = build_task_agent_with_role(database.clone(), None, execution_role).await?;

    if let Some(memory_system) = agent_memory_system(&agent) {
        tokio::spawn(async move {
            memory_system
                .start_consolidation_loop(std::time::Duration::from_secs(
                    memory_config.consolidation_interval_mins * 60,
                ))
                .await;
        });
    }

    Ok(Arc::new(RwLock::new(agent)))
}

pub async fn build_task_agent(
    database: Arc<Database>,
    workspace_override: Option<std::path::PathBuf>,
) -> Result<AgentCore> {
    let config = Config::load_or_create()?;
    let execution_role =
        crate::remote::local_execution_role_for_config(&config).unwrap_or(NodeExecutionRole::Full);
    build_task_agent_with_role(database, workspace_override, execution_role).await
}

async fn build_task_agent_with_role(
    database: Arc<Database>,
    workspace_override: Option<std::path::PathBuf>,
    execution_role: NodeExecutionRole,
) -> Result<AgentCore> {
    let config = Config::load_or_create()?;
    let mut config = config;
    if let Some(workspace) = workspace_override {
        config.core.workspace = workspace;
    }

    let db_pool = database.pool().clone();

    let (providers, local_brain) = providers::build_for_execution_role(&config, execution_role).await?;
    let router = Arc::new(LLMRouter::with_local_brain(
        providers,
        Arc::new(config.llm.clone()),
        local_brain,
    ));
    let rate_limiter = Arc::new(RateLimiter::new(db_pool.clone()));
    let risk_assessor = RiskAssessor::new();
    let task_repo = Arc::new(TaskRepository::new(db_pool.clone()));
    let memory_system = Arc::new(MemorySystem::new_with_config(
        db_pool,
        router.clone(),
        config.memory.clone(),
    ));

    let tools = plugins::build(&database, &config).await?;
    let steering = load_steering(&config).await?;
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

    Ok(agent)
}

async fn load_steering(config: &Config) -> Result<SteeringEngine> {
    let steering_dir = config.steering.skill_dir.clone();
    let workspace_dir = config.core.workspace.join(".rove").join("steering");
    let mut steering =
        SteeringEngine::new_with_workspace(&steering_dir, Some(&workspace_dir)).await?;
    steering.load_all_skills().await?;
    Ok(steering)
}

fn agent_memory_system(agent: &AgentCore) -> Option<Arc<MemorySystem>> {
    agent.memory_system().cloned()
}
