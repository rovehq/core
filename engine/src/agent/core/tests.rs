use std::sync::Arc;

use tempfile::TempDir;

use super::{AgentCore, TaskResult};
use crate::builtin_tools::ToolRegistry;
use crate::config::LLMConfig;
use crate::db::tasks::TaskRepository;
use crate::db::Database;
use crate::gateway::{Task, WorkspaceLocks};
use crate::llm::router::LLMRouter;
use crate::rate_limiter::RateLimiter;
use crate::risk_assessor::RiskAssessor;

async fn setup_test_agent() -> (TempDir, AgentCore) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let pool = db.pool().clone();

    let llm_config = Arc::new(LLMConfig {
        default_provider: "ollama".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let router = Arc::new(LLMRouter::new(vec![], llm_config));
    let risk_assessor = RiskAssessor::new();
    let rate_limiter = Arc::new(RateLimiter::new(pool.clone()));
    let task_repo = Arc::new(TaskRepository::new(pool));

    let agent = AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        Arc::new(ToolRegistry::empty()),
        None,
        Arc::new(crate::config::Config::default()),
        Arc::new(WorkspaceLocks::new()),
    )
    .expect("Failed to create AgentCore in test");

    (temp_dir, agent)
}

#[test]
fn test_task_creation() {
    let task = Task::build_from_cli("Test task");
    assert_eq!(task.input, "Test task");
    assert_eq!(task.source, sdk::TaskSource::Cli);
}

#[test]
fn test_task_result_creation() {
    let result = TaskResult::success(
        "task-123".to_string(),
        "Answer".to_string(),
        "ollama".to_string(),
        1000,
        5,
        sdk::TaskDomain::General,
        false,
    );

    assert_eq!(result.task_id, "task-123");
    assert_eq!(result.answer, "Answer");
    assert_eq!(result.provider_used, "ollama");
    assert_eq!(result.duration_ms, 1000);
    assert_eq!(result.iterations, 5);
    assert_eq!(result.domain, sdk::TaskDomain::General);
    assert!(!result.sensitive);
}

#[tokio::test]
async fn test_agent_core_creation() {
    let (_temp_dir, agent) = setup_test_agent().await;
    assert_eq!(agent.memory.messages().len(), 0);
}
