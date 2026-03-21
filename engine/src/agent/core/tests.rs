use std::sync::Arc;
use std::{collections::VecDeque, sync::Mutex};

use async_trait::async_trait;
use std::time::Instant;
use tempfile::TempDir;

use super::orchestration::{decide_execution_strategy, ExecutionStrategy, OrchestrationHistory};
use super::prompt::TaskContext;
use super::{AgentCore, TaskResult};
use crate::builtin_tools::{FilesystemTool, TerminalTool, ToolRegistry};
use crate::config::LLMConfig;
use crate::db::tasks::TaskRepository;
use crate::db::tasks::TaskStatus;
use crate::db::Database;
use crate::gateway::{Task, WorkspaceLocks};
use crate::llm::router::LLMRouter;
use crate::llm::{FinalAnswer, LLMProvider, LLMResponse, Message};
use crate::rate_limiter::RateLimiter;
use crate::risk_assessor::RiskAssessor;
use crate::risk_assessor::RiskTier;
use sdk::{Complexity, RemoteExecutionPlan, Route, TaskDomain};

async fn setup_test_agent() -> (TempDir, AgentCore) {
    setup_test_agent_with_providers(vec![], false).await
}

async fn setup_test_agent_with_providers(
    providers: Vec<Box<dyn LLMProvider>>,
    include_core_tools: bool,
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

    let mut tools = ToolRegistry::empty();
    if include_core_tools {
        tools
            .register_builtin_filesystem(
                FilesystemTool::new(temp_dir.path().to_path_buf()).unwrap(),
            )
            .await;
        tools
            .register_builtin_terminal(TerminalTool::new(
                temp_dir.path().to_string_lossy().to_string(),
            ))
            .await;
    }

    let agent = AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        Arc::new(tools),
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
        self.responses.lock().unwrap().pop_front().ok_or_else(|| {
            crate::llm::LLMError::ProviderUnavailable("No mock response".to_string())
        })
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
async fn test_planned_task_executes_direct_tool_without_provider() {
    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![], true).await;
    let file_path = temp_dir.path().join("note.txt");
    std::fs::write(&file_path, "executor payload").expect("write fixture");

    let task = Task::build_from_cli("read note.txt on the remote executor");
    let result = agent
        .process_planned_task(
            task,
            RemoteExecutionPlan::direct(
                "read fixture note",
                "read_file",
                serde_json::json!({ "path": file_path }),
                Some("general".to_string()),
            ),
        )
        .await
        .expect("planned task should succeed");

    assert!(result.answer.contains("executor payload"));
    assert_eq!(result.provider_used, "executor-plan");
    assert_eq!(result.iterations, 1);
}

#[tokio::test]
async fn test_planned_task_executes_bundled_steps_without_provider() {
    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![], true).await;
    let file_path = temp_dir.path().join("note.txt");
    std::fs::write(&file_path, "executor payload").expect("write fixture");

    let mut plan = RemoteExecutionPlan::direct(
        "verify note exists",
        "file_exists",
        serde_json::json!({ "path": file_path }),
        Some("general".to_string()),
    );
    plan.append_step(
        "read fixture note",
        "read_file",
        serde_json::json!({ "path": file_path }),
    );

    let task = Task::build_from_cli("read note.txt on the remote executor");
    let result = agent
        .process_planned_task(task, plan)
        .await
        .expect("planned bundled task should succeed");

    assert!(result.answer.contains("executor payload"));
    assert_eq!(result.provider_used, "executor-plan");
    assert_eq!(result.iterations, 2);
}

#[test]
fn simple_general_task_stays_linear() {
    let task = Task::build_from_cli("what does this error mean?");
    let context = TaskContext {
        domain_str: "general".to_string(),
        domain: TaskDomain::General,
        complexity: Complexity::Simple,
        route: Route::Cloud,
        sensitive: false,
    };

    let decision = decide_execution_strategy(&task, &context, &[], None);

    assert_eq!(decision.strategy, ExecutionStrategy::Linear);
    assert_eq!(decision.estimated_steps, 1);
}

#[test]
fn multi_stage_medium_task_fans_out() {
    let task = Task::build_from_cli(
        "First inspect the release notes, then update the rollout summary, and finally verify the checklist.",
    );
    let context = TaskContext {
        domain_str: "general".to_string(),
        domain: TaskDomain::General,
        complexity: Complexity::Medium,
        route: Route::Cloud,
        sensitive: false,
    };

    let decision = decide_execution_strategy(
        &task,
        &context,
        &["cargo test".to_string()],
        Some(&OrchestrationHistory {
            sampled_tasks: 3,
            dag_tasks: 2,
            linear_tasks: 1,
            failed_tasks: 1,
            average_dag_steps: 3,
        }),
    );

    assert_eq!(decision.strategy, ExecutionStrategy::Dag);
    assert!(decision.estimated_steps >= 3);
    assert!(decision
        .reasons
        .iter()
        .any(|reason| reason == "post-write verification"));
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

    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![provider], false).await;
    let task = Task::build_from_cli("plan a complex rust refactor");
    let result = agent
        .process_task(task)
        .await
        .expect("complex task should succeed");

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

    assert!(events
        .iter()
        .any(|event| event.event_type == "dag_wave_started"));
    assert!(events
        .iter()
        .any(|event| event.event_type == "dag_step_succeeded"));
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

    let (_temp_dir, mut agent) = setup_test_agent_with_providers(vec![provider], false).await;
    let task = Task::build_from_cli("build the project and then run tests");
    let result = agent
        .process_task(task)
        .await
        .expect("medium task should succeed");

    assert_eq!(result.answer, "tests passed");
    assert_eq!(result.provider_used, "dag[cloud]");
    assert_eq!(result.iterations, 2);
}

#[tokio::test]
async fn test_dag_write_steps_run_policy_after_write_commands() {
    let plan = r#"[
        {"id":"step_1","description":"Update the note file","role":"Executor","parallel_safe":false,"dependencies":[],"expected_outcome":"File updated"}
    ]"#;
    let responses = vec![
        LLMResponse::FinalAnswer(FinalAnswer::new(plan)),
        LLMResponse::ToolCall(crate::llm::ToolCall::new(
            "tool-1".to_string(),
            "write_file",
            serde_json::json!({"path":"notes.txt","content":"hello"}).to_string(),
        )),
        LLMResponse::FinalAnswer(FinalAnswer::new("file updated and checked")),
    ];
    let provider: Box<dyn LLMProvider> =
        Box::new(MockSequenceProvider::new("mock-cloud", responses));

    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![provider], true).await;
    let task = Task::build_from_cli("update the note file and then verify the directory");
    let context = agent
        .initialize_task_context(&task, RiskTier::Tier0)
        .await
        .expect("task context");
    agent.policy_after_write_commands = vec!["rg --files".to_string()];
    agent
        .task_repo
        .create_task(&task.id, &task.input)
        .await
        .expect("create task");
    agent
        .task_repo
        .update_task_status(&task.id, TaskStatus::Running)
        .await
        .expect("mark running");

    let orchestration = agent.select_execution_strategy(&task, &context).await;
    let result = agent
        .execute_dag_task(&task.id, &task, &context, &orchestration, Instant::now())
        .await
        .expect("DAG write task should succeed");
    assert_eq!(result.answer, "file updated and checked");

    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let events = db
        .tasks()
        .get_agent_events_by_parent(&result.task_id)
        .await
        .unwrap();

    assert!(events.iter().any(|event| {
        event.event_type == "tool_call" && event.payload.contains("\"tool_name\":\"write_file\"")
    }));
    assert!(events.iter().any(|event| {
        event.event_type == "tool_call" && event.payload.contains("\"tool_name\":\"run_command\"")
    }));
    assert!(events
        .iter()
        .any(|event| { event.event_type == "observation" && event.payload.contains("notes.txt") }));
}
