use std::sync::Arc;
use std::{collections::VecDeque, sync::Mutex};

use async_trait::async_trait;
use tempfile::TempDir;

use super::{AgentCore, TaskResult};
use crate::builtin_tools::ToolRegistry;
use crate::config::LLMConfig;
use crate::db::tasks::TaskRepository;
use crate::db::Database;
use crate::gateway::{Task, WorkspaceLocks};
use crate::llm::router::LLMRouter;
use crate::llm::{FinalAnswer, LLMProvider, LLMResponse, Message};
use crate::rate_limiter::RateLimiter;
use crate::risk_assessor::RiskAssessor;

async fn setup_test_agent() -> (TempDir, AgentCore) {
    setup_test_agent_with_providers(vec![]).await
}

async fn setup_test_agent_with_providers(
    providers: Vec<Box<dyn LLMProvider>>,
) -> (TempDir, AgentCore) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let pool = db.pool().clone();

    let llm_config = Arc::new(LLMConfig {
        default_provider: "mock-cloud".to_string(),
        sensitivity_threshold: 0.7,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let router = Arc::new(LLMRouter::new(providers, llm_config));
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

struct MockSequenceProvider {
    name: String,
    responses: Mutex<VecDeque<LLMResponse>>,
}

impl MockSequenceProvider {
    fn new(name: &str, responses: Vec<LLMResponse>) -> Self {
        Self {
            name: name.to_string(),
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

#[async_trait]
impl LLMProvider for MockSequenceProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_local(&self) -> bool {
        false
    }

    fn estimated_cost(&self, _tokens: usize) -> f64 {
        1.0
    }

    async fn generate(&self, _messages: &[Message]) -> crate::llm::Result<LLMResponse> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| crate::llm::LLMError::ProviderUnavailable("No mock response".to_string()))
    }
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

#[tokio::test]
async fn test_complex_task_uses_dag_execution_path() {
    let plan = r#"[
        {"id":"step_1","description":"Inspect the code","role":"Researcher","parallel_safe":false,"dependencies":[],"expected_outcome":"Code inspected"},
        {"id":"step_2","description":"Summarize findings","role":"Verifier","parallel_safe":true,"dependencies":["step_1"],"expected_outcome":"Findings summarized"}
    ]"#;
    let responses = vec![
        LLMResponse::FinalAnswer(FinalAnswer::new(plan)),
        LLMResponse::FinalAnswer(FinalAnswer::new("inspected the code")),
        LLMResponse::FinalAnswer(FinalAnswer::new("verified summary")),
    ];
    let provider: Box<dyn LLMProvider> =
        Box::new(MockSequenceProvider::new("mock-cloud", responses));

    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![provider]).await;
    let task = Task::build_from_cli("plan a complex rust refactor");
    let result = agent.process_task(task).await.expect("complex task should succeed");

    assert_eq!(result.answer, "verified summary");
    assert_eq!(result.provider_used, "dag[cloud]");
    assert_eq!(result.iterations, 2);

    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let task_uuid = uuid::Uuid::parse_str(&result.task_id).unwrap();
    let events = db
        .tasks()
        .get_agent_events(&task_uuid.to_string())
        .await
        .unwrap();

    assert!(events.iter().any(|event| event.event_type == "dag_wave_started"));
    assert!(events.iter().any(|event| event.event_type == "dag_step_succeeded"));
    assert!(events.iter().any(|event| event.event_type == "answer"));
}

#[tokio::test]
async fn test_medium_code_task_uses_dag_execution_path() {
    let plan = r#"[
        {"id":"step_1","description":"Build the project","role":"Executor","parallel_safe":false,"dependencies":[],"expected_outcome":"Project built"},
        {"id":"step_2","description":"Run the tests","role":"Verifier","parallel_safe":true,"dependencies":["step_1"],"expected_outcome":"Tests verified"}
    ]"#;
    let responses = vec![
        LLMResponse::FinalAnswer(FinalAnswer::new(plan)),
        LLMResponse::FinalAnswer(FinalAnswer::new("build complete")),
        LLMResponse::FinalAnswer(FinalAnswer::new("tests passed")),
    ];
    let provider: Box<dyn LLMProvider> =
        Box::new(MockSequenceProvider::new("mock-cloud", responses));

    let (_temp_dir, mut agent) = setup_test_agent_with_providers(vec![provider]).await;
    let task = Task::build_from_cli("build the project and then run tests");
    let result = agent.process_task(task).await.expect("medium task should succeed");

    assert_eq!(result.answer, "tests passed");
    assert_eq!(result.provider_used, "dag[cloud]");
    assert_eq!(result.iterations, 2);
}
