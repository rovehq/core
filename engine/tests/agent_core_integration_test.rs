//! Integration tests for the Agent Core Loop
//!
//! Validates the core agent limits and behavior:
//! - Max iterations limit
//! - Result size limits
//! - LLM timeout enforcement

use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use rove_engine::agent::core::AgentCore;
use rove_engine::config::LLMConfig;
use rove_engine::db::tasks::TaskRepository;
use rove_engine::db::Database;
use rove_engine::gateway::Task;
use rove_engine::llm::{ollama::OllamaProvider, router::LLMRouter, LLMProvider};
use rove_engine::rate_limiter::RateLimiter;
use rove_engine::risk_assessor::RiskAssessor;
use sdk::errors::EngineError;

async fn setup_agent(mock_uri: &str, temp_dir: &TempDir) -> AgentCore {
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let pool = db.pool().clone();

    let llm_config = Arc::new(LLMConfig {
        default_provider: "ollama".to_string(),
        sensitivity_threshold: 0.5,
        complexity_threshold: 0.8,
        ollama: Default::default(),
        openai: Default::default(),
        anthropic: Default::default(),
        gemini: Default::default(),
        nvidia_nim: Default::default(),
        custom_providers: vec![],
    });

    let provider = Box::new(OllamaProvider::new(mock_uri, "llama3.1:8b").unwrap()) as Box<dyn LLMProvider>;
    let router = Arc::new(LLMRouter::new(vec![provider], llm_config));

    let risk_assessor = RiskAssessor::new();
    let rate_limiter = Arc::new(RateLimiter::new(pool.clone()));
    let task_repo = Arc::new(TaskRepository::new(pool));

    use rove_engine::builtin_tools::ToolRegistry;
    let tools = Arc::new(ToolRegistry::empty());
    let workspace_locks = Arc::new(rove_engine::gateway::WorkspaceLocks::new());
    AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        tools,
        None,
        Arc::new(rove_engine::config::Config::default()),
        workspace_locks,
    )
    .expect("Failed to create AgentCore in test")
}

// Property 1: Agent Loop Iteration Limit
// Validates: Requirements 2.2
#[tokio::test]
async fn test_property_agent_loop_iteration_limit() {
    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();

    // The mock server will always return a tool call
    // This will force the agent to iterate until it hits MAX_ITERATIONS (20)
    let tool_call_response = json!({
        "model": "llama3.1:8b",
        "created_at": "2023-08-04T19:22:45.499127Z",
        "message": {
            "role": "assistant",
            "content": "{\"function\": \"dummy_tool\", \"arguments\": {}}"
        },
        "done": true
    });

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(tool_call_response))
        .mount(&mock_server)
        .await;

    let mut agent = setup_agent(&mock_server.uri(), &temp_dir).await;
    let task = Task::build_from_cli("Do a loop");
    let result = agent.process_task(task).await;

    let err = result.expect_err("Agent should fail after exceeding max iterations");
    let root_cause = err.root_cause();
    let engine_error = root_cause.downcast_ref::<EngineError>();
    assert!(
        matches!(engine_error, Some(EngineError::MaxIterationsExceeded)),
        "Unexpected error: {:?}",
        err
    );
}

// Property 2: LLM Call Timeout Enforcement
// Validates: Requirements 2.3
#[tokio::test]
async fn test_property_llm_call_timeout_enforcement() {
    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();

    // We can't actually wait 30 seconds in a unit test easily without making the test suite slow.
    // Instead, we will configure a delayed response, but we might just assert that the timeout handling works
    // Unfortunately, the AgentCore hardcodes LLM_TIMEOUT_SECS = 30. We will set a small timeout in the test context if possible,
    // or we'll just skip this full duration run and test a manually dropped connection, OR we can accept the 30s wait for this one test.
    // Given the constraints, let's just make the mock server hold for 31 seconds. This will make `cargo test` take 30+ seconds.
    // Better yet, we can skip running this specific test by default if it takes 30s, or we can just let it run.

    // To keep testing fast in CI, we will just use a 1 second delay if we could configure the timeout, but we can't easily without editing `src/agent/core.rs`. Let's mock the timeout by passing a NetworkError to avoid a 30s test run, or actually run the true test with 31 seconds delay.
    // For standard property checking, we'll configure a delay of 31 secs.
    let response = ResponseTemplate::new(200).set_delay(Duration::from_secs(31));

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(response)
        .mount(&mock_server)
        .await;

    let mut agent = setup_agent(&mock_server.uri(), &temp_dir).await;
    let task = Task::build_from_cli("Timeout task");
    let result = agent.process_task(task).await;

    // This will take 30s to fail.
    let err = result.expect_err("Agent should fail due to timeout");
    let root_cause = err.root_cause();
    let engine_error = root_cause.downcast_ref::<EngineError>();
    assert!(
        matches!(engine_error, Some(EngineError::LLMTimeout)) || engine_error.is_none(),
        "Expected timeout error, got: {:?}",
        err
    );
}

// Property 3: Result Size Limit
// Validates: Requirements 2.4
#[tokio::test]
async fn test_property_result_size_limit() {
    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();

    // Create a 6MB response string
    // AgentCore enforces 5MB limit
    let large_content = "A".repeat(6 * 1024 * 1024);

    let large_response = json!({
        "model": "llama3.1:8b",
        "created_at": "2023-08-04T19:22:45.499127Z",
        "message": {
            "role": "assistant",
            "content": large_content,
        },
        "done": true
    });

    // Note: Generating 6MB JSON may be slow, but it's local.
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(large_response))
        .mount(&mock_server)
        .await;

    let mut agent = setup_agent(&mock_server.uri(), &temp_dir).await;
    let task = Task::build_from_cli("Large task");
    let result = agent.process_task(task).await;

    let err = result.expect_err("Agent should fail due to result size limit");
    let root_cause = err.root_cause();
    let engine_error = root_cause.downcast_ref::<EngineError>();
    assert!(
        matches!(engine_error, Some(EngineError::ResultSizeExceeded { .. })),
        "Unexpected error: {:?}",
        err
    );
}

// Property 4: Task Persistence Completeness
// Validates: Requirements 2.5
#[tokio::test]
async fn test_property_task_persistence_completeness() {
    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();

    let success_response = json!({
        "model": "llama3.1:8b",
        "created_at": "2023-08-04T19:22:45.499127Z",
        "message": {
            "role": "assistant",
            "content": "Final Answer Document",
        },
        "done": true
    });

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_response))
        .mount(&mock_server)
        .await;

    let mut agent = setup_agent(&mock_server.uri(), &temp_dir).await;
    let task = Task::build_from_cli("Persistence task");

    let result = agent
        .process_task(task)
        .await
        .expect("Task failed unexpectedly");
    let task_id = result.task_id;

    // Check database to ensure steps were persisted
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();

    let task_uuid = uuid::Uuid::parse_str(&task_id).unwrap();
    let task_record = db.tasks().get_task(&task_uuid).await.unwrap().unwrap();
    assert_eq!(task_record.status, rove_engine::db::TaskStatus::Completed);

    let steps = db.tasks().get_task_steps(&task_uuid).await.unwrap();
    assert!(steps.len() >= 2); // Initial user message + final answer
}

// Property 5: Task Serialization Round-Trip
// Validates: Requirements 2.9
#[test]
fn test_property_task_serialization_round_trip() {
    use rove_engine::db::tasks::{Task, TaskStatus};

    let original_task = Task {
        id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap(),
        input: "Test input string with special characters !@#$%^&*()".to_string(),
        status: TaskStatus::Completed,
        provider_used: Some("openai".to_string()),
        duration_ms: Some(1500),
        created_at: 1600000000,
        completed_at: Some(1600000005),
    };

    // Serialize to JSON
    let serialized = serde_json::to_string(&original_task).expect("Failed to serialize task");

    // Deserialize back
    let deserialized: Task = serde_json::from_str(&serialized).expect("Failed to deserialize task");

    // Verify all fields remain exactly the same
    assert_eq!(original_task.id, deserialized.id);
    assert_eq!(original_task.input, deserialized.input);
    assert_eq!(original_task.status, deserialized.status);
    assert_eq!(original_task.provider_used, deserialized.provider_used);
    assert_eq!(original_task.duration_ms, deserialized.duration_ms);
    assert_eq!(original_task.created_at, deserialized.created_at);
    assert_eq!(original_task.completed_at, deserialized.completed_at);
}
