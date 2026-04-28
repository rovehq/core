mod helpers;

use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;

use rove_engine::agent::core::{decide_execution_strategy, ExecutionStrategy, OrchestrationHistory, TaskContext};
use rove_engine::agent::{AgentCore, TaskResult};
use rove_engine::builtin_tools::{FilesystemTool, TerminalTool, ToolRegistry};
use rove_engine::config::LLMConfig;
use rove_engine::db::tasks::TaskRepository;
use rove_engine::db::tasks::TaskStatus;
use rove_engine::db::Database;
use rove_engine::gateway::{Task, WorkspaceLocks};
use rove_engine::llm::router::LLMRouter;
use rove_engine::llm::{FinalAnswer, LLMProvider, LLMResponse};
use rove_engine::rate_limiter::RateLimiter;
use rove_engine::risk_assessor::RiskAssessor;
use rove_engine::risk_assessor::RiskTier;
use sdk::{
    Complexity, OutcomeContract, RemoteExecutionPlan, Route, TaskDomain, TaskExecutionProfile,
    TaskSource,
};

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

    let test_config = Arc::new(rove_engine::config::Config {
        security: rove_engine::config::SecurityConfig {
            confirm_tier1: false,
            require_explicit_tier2: false,
            ..Default::default()
        },
        ..Default::default()
    });

    let mut tools = ToolRegistry::empty_with_config(Arc::clone(&test_config));
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
        test_config,
        Arc::new(WorkspaceLocks::new()),
    )
    .expect("Failed to create AgentCore in test");

    (temp_dir, agent)
}

async fn setup_test_agent_for_hook_workspace(
    workspace: &std::path::Path,
    providers: Vec<Box<dyn LLMProvider>>,
) -> AgentCore {
    let db_path = workspace.join("test.db");
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

    let config = Arc::new(rove_engine::config::Config {
        core: rove_engine::config::CoreConfig {
            workspace: workspace.to_path_buf(),
            ..Default::default()
        },
        llm: llm_config.as_ref().clone(),
        ..Default::default()
    });

    let router = Arc::new(LLMRouter::new(providers, llm_config));
    let risk_assessor = RiskAssessor::new();
    let rate_limiter = Arc::new(RateLimiter::new(pool.clone()));
    let task_repo = Arc::new(TaskRepository::new(pool));
    let tools = Arc::new(ToolRegistry::new(config.clone(), None, None, None));

    AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        tools,
        None,
        config,
        Arc::new(WorkspaceLocks::new()),
    )
    .expect("Failed to create AgentCore in test")
}

use helpers::mock_llm::MockSequenceProvider;

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
async fn initialize_task_context_injects_recent_thread_history() {
    let (_temp_dir, mut agent) = setup_test_agent_with_providers(vec![], true).await;
    let thread_id = "workflow:release:thread:research".to_string();
    let profile = TaskExecutionProfile {
        agent_id: Some("agent.release".to_string()),
        agent_name: Some("Release Agent".to_string()),
        thread_id: Some(thread_id.clone()),
        worker_preset_id: None,
        worker_preset_name: None,
        purpose: Some("Investigate release state".to_string()),
        instructions: String::new(),
        allowed_tools: Vec::new(),
        callable_agents: Vec::new(),
        output_contract: None,
        outcome_contract: None,
        max_iterations: None,
    };

    let prior_task_id = uuid::Uuid::new_v4();
    agent
        .task_repo
        .create_task_with_metadata(
            &prior_task_id,
            "inspect the last rollout",
            Some(&TaskSource::Cli),
            Some(&profile),
        )
        .await
        .unwrap();
    agent
        .task_repo
        .update_task_status(&prior_task_id, TaskStatus::Completed)
        .await
        .unwrap();
    agent
        .task_repo
        .insert_agent_event(
            &prior_task_id,
            "answer",
            r#"{"answer":"Found a regression in the checklist"}"#,
            1,
            Some("general"),
        )
        .await
        .unwrap();

    let task = Task::build_from_cli("prepare a release summary").with_execution_profile(profile);
    let _context = agent
        .initialize_task_context(&task, RiskTier::Tier0)
        .await
        .expect("task context");

    let system_prompt = &agent.memory.messages()[0].content;
    assert!(system_prompt.contains("Persistent execution thread context"));
    assert!(system_prompt.contains(&thread_id));
    assert!(system_prompt.contains("inspect the last rollout"));
    assert!(system_prompt.contains("Found a regression in the checklist"));
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

#[tokio::test]
async fn test_planned_task_enforces_agent_allowed_tools() {
    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![], true).await;
    let file_path = temp_dir.path().join("note.txt");
    std::fs::write(&file_path, "restricted payload").expect("write fixture");

    let task = Task::build_from_cli("read note.txt").with_execution_profile(TaskExecutionProfile {
        agent_id: Some("restricted-reader".to_string()),
        agent_name: Some("Restricted Reader".to_string()),
        thread_id: None,
        worker_preset_id: None,
        worker_preset_name: None,
        purpose: Some("read fixture".to_string()),
        instructions: "Only use the allowed tools.".to_string(),
        allowed_tools: vec!["write_file".to_string()],
        callable_agents: Vec::new(),
        output_contract: None,
        outcome_contract: None,
        max_iterations: None,
    });

    let error = agent
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
        .expect_err("restricted agent should reject disallowed tool");

    assert!(error
        .to_string()
        .contains("tool 'read_file' is not allowed for 'Restricted Reader'"));
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
            apex_tasks: 2,
            linear_tasks: 1,
            failed_tasks: 1,
            average_apex_steps: 3,
        }),
    );

    assert_eq!(decision.strategy, ExecutionStrategy::Apex);
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
    assert_eq!(result.provider_used, "apex[cloud]");
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
    assert_eq!(result.provider_used, "apex[cloud]");
    assert_eq!(result.iterations, 2);
}

#[tokio::test]
async fn final_answer_retries_when_outcome_evaluator_requests_revision() {
    let responses = vec![
        LLMResponse::FinalAnswer(FinalAnswer::new("deployment is okay")),
        LLMResponse::FinalAnswer(FinalAnswer::new(
            r#"{"decision":"retry","reason":"The answer does not say whether verification passed.","retry_guidance":"State the deployment status and verification result explicitly."}"#,
        )),
        LLMResponse::FinalAnswer(FinalAnswer::new(
            "deployment succeeded and verification passed",
        )),
        LLMResponse::FinalAnswer(FinalAnswer::new(
            r#"{"decision":"pass","reason":"The answer now satisfies the success criteria."}"#,
        )),
    ];
    let provider: Box<dyn LLMProvider> =
        Box::new(MockSequenceProvider::new("mock-cloud", responses));

    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![provider], false).await;
    let task = Task::build_from_cli("report the deployment outcome").with_execution_profile(
        TaskExecutionProfile {
            agent_id: Some("agent.release".to_string()),
            agent_name: Some("Release Agent".to_string()),
            thread_id: None,
            worker_preset_id: None,
            worker_preset_name: None,
            purpose: Some("Summarize release state".to_string()),
            instructions: "Answer directly.".to_string(),
            allowed_tools: Vec::new(),
            callable_agents: Vec::new(),
            output_contract: None,
            outcome_contract: Some(OutcomeContract {
                success_criteria: "State the deployment status and whether verification passed."
                    .to_string(),
                max_self_evals: 1,
                evaluator_policy: "self_check".to_string(),
            }),
            max_iterations: None,
        },
    );

    let result = agent.process_task(task).await.expect("task should succeed");
    assert_eq!(
        result.answer,
        "deployment succeeded and verification passed"
    );

    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let events = db.tasks().get_agent_events(&result.task_id).await.unwrap();

    assert!(events
        .iter()
        .any(|event| event.event_type == "evaluation_retry"));
    assert!(
        events
            .iter()
            .filter(|event| event.event_type == "answer")
            .count()
            >= 2
    );
}

#[tokio::test]
async fn evaluator_failure_persists_structured_event() {
    let responses = vec![
        LLMResponse::FinalAnswer(FinalAnswer::new("candidate answer")),
        LLMResponse::FinalAnswer(FinalAnswer::new("not json")),
    ];
    let provider: Box<dyn LLMProvider> =
        Box::new(MockSequenceProvider::new("mock-cloud", responses));

    let (temp_dir, mut agent) = setup_test_agent_with_providers(vec![provider], false).await;
    let task = Task::build_from_cli("report deployment outcome").with_execution_profile(
        TaskExecutionProfile {
            agent_id: Some("agent.release".to_string()),
            agent_name: Some("Release Agent".to_string()),
            thread_id: None,
            worker_preset_id: None,
            worker_preset_name: None,
            purpose: Some("Summarize release state".to_string()),
            instructions: "Answer directly.".to_string(),
            allowed_tools: Vec::new(),
            callable_agents: Vec::new(),
            output_contract: None,
            outcome_contract: Some(OutcomeContract {
                success_criteria: "State the deployment status and whether verification passed."
                    .to_string(),
                max_self_evals: 0,
                evaluator_policy: "self_check".to_string(),
            }),
            max_iterations: None,
        },
    );
    let task_id = task.id;

    let error = agent
        .process_task(task)
        .await
        .expect_err("evaluation parse failure should fail task");
    assert!(error
        .to_string()
        .contains("Failed to parse outcome evaluation JSON"));

    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let events = db
        .tasks()
        .get_agent_events(&task_id.to_string())
        .await
        .unwrap();

    assert!(events.iter().any(|event| {
        event.event_type == "evaluation_result"
            && event.payload.contains("Outcome evaluation failed to run")
    }));
}

#[tokio::test]
async fn lifecycle_hooks_modify_input_and_observe_output() {
    let responses = vec![LLMResponse::FinalAnswer(FinalAnswer::new("hooked answer"))];
    let provider: Box<dyn LLMProvider> =
        Box::new(MockSequenceProvider::new("mock-cloud", responses));

    let temp_dir = TempDir::new().unwrap();
    let hooks_root = temp_dir.path().join(".rove").join("hooks");
    std::fs::create_dir_all(&hooks_root).expect("hooks dir");

    let receive_dir = hooks_root.join("rewrite-message");
    std::fs::create_dir_all(&receive_dir).expect("receive dir");
    let receive_script = receive_dir.join("handler.sh");
    std::fs::write(
        receive_dir.join("HOOK.md"),
        r#"
name = "rewrite-message"
events = ["MessageReceived"]
command = "./handler.sh"
"#,
    )
    .expect("receive hook");
    std::fs::write(
        &receive_script,
        "#!/bin/sh\nprintf '{\"action\":\"modify\",\"data\":{\"input\":\"rewritten task\"}}'\n",
    )
    .expect("receive script");

    let send_dir = hooks_root.join("capture-send");
    std::fs::create_dir_all(&send_dir).expect("send dir");
    let send_script = send_dir.join("handler.sh");
    let output_path = temp_dir.path().join("message-sending.json");
    std::fs::write(
        send_dir.join("HOOK.md"),
        r#"
name = "capture-send"
events = ["MessageSending"]
command = "./handler.sh"
"#,
    )
    .expect("send hook");
    std::fs::write(
        &send_script,
        format!(
            "#!/bin/sh\ncat > \"{}\"\nprintf '{{\"action\":\"continue\"}}'\n",
            output_path.display()
        ),
    )
    .expect("send script");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&receive_script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod receive");
        std::fs::set_permissions(&send_script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod send");
    }

    let mut agent = setup_test_agent_for_hook_workspace(temp_dir.path(), vec![provider]).await;
    let task = Task::build_from_cli("original task");
    let result = agent.process_task(task).await.expect("task should succeed");
    assert_eq!(result.answer, "hooked answer");

    let sent_payload = std::fs::read_to_string(&output_path).expect("message sending payload");
    assert!(sent_payload.contains("\"event\":\"MessageSending\""));
    assert!(sent_payload.contains("\"output\":\"hooked answer\""));

    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(&db_path).await.unwrap();
    let stored_task = db
        .tasks()
        .get_task(&uuid::Uuid::parse_str(&result.task_id).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_task.input, "rewritten task");
}

#[tokio::test]
async fn test_apex_write_steps_run_policy_after_write_commands() {
    let plan = r#"[
        {"id":"step_1","description":"Update the note file","role":"Executor","parallel_safe":false,"dependencies":[],"expected_outcome":"File updated"}
    ]"#;
    let responses = vec![
        LLMResponse::FinalAnswer(FinalAnswer::new(plan)),
        LLMResponse::ToolCall(rove_engine::llm::ToolCall::new(
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
    agent.policy_after_write_commands = vec!["git --version".to_string()];
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
        .execute_apex_task(&task.id, &task, &context, &orchestration, Instant::now())
        .await
        .expect("APEX write task should succeed");
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