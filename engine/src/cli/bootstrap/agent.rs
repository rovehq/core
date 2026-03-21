use std::sync::Arc;

use anyhow::Result;
use sdk::NodeExecutionRole;
use tokio::sync::RwLock;

use crate::agent::AgentCore;
use crate::api::gateway::WorkspaceLocks;
use crate::config::Config;
use crate::llm::router::LLMRouter;
use crate::memory::conductor::MemorySystem;
use crate::policy::{active_workspace_policy_dir, legacy_policy_workspace_dir, policy_workspace_dir};
use crate::security::rate_limiter::RateLimiter;
use crate::security::risk_assessor::RiskAssessor;
use crate::policy::PolicyEngine;
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
    let policy_engine = load_policy_engine(&config).await?;
    let workspace_locks = Arc::new(WorkspaceLocks::new());

    let mut agent = AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        tools,
        Some(policy_engine),
        Arc::new(config),
        workspace_locks,
    )?;
    agent.set_memory_system(memory_system);

    Ok(agent)
}

async fn load_policy_engine(config: &Config) -> Result<PolicyEngine> {
    let policy_dir = config.policy.policy_dir().clone();
    let primary_workspace_dir = policy_workspace_dir(&config.core.workspace);
    let legacy_workspace_dir = legacy_policy_workspace_dir(&config.core.workspace);
    let workspace_dir = active_workspace_policy_dir(&primary_workspace_dir, &legacy_workspace_dir);
    let mut policy_engine =
        PolicyEngine::new_with_workspace(&policy_dir, Some(&workspace_dir)).await?;
    policy_engine.load_all_policies().await?;
    Ok(policy_engine)
}

fn agent_memory_system(agent: &AgentCore) -> Option<Arc<MemorySystem>> {
    agent.memory_system().cloned()
}
