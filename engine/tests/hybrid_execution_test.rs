//! Integration tests for Hybrid Execution Mode
//!
//! Tests cloud planning + local execution with DAG concurrency

use rove_engine::conductor::types::{ConductorPlan, PlanStep, StepRole, StepType};
use rove_engine::conductor::HybridExecutor;
use rove_engine::config::LLMConfig;
use rove_engine::llm::router::LLMRouter;
use std::sync::Arc;

/// Test DAG execution with parallel steps
#[test]
fn test_dag_parallel_execution() {
    let config = Arc::new(LLMConfig {
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

    let router = Arc::new(LLMRouter::new(vec![], config));
    let executor = HybridExecutor::new(router, None);

    // Create a plan with parallel steps
    let plan = ConductorPlan {
        id: "test_plan".to_string(),
        original_goal: "Test parallel execution".to_string(),
        mode: Default::default(),
        stages: vec![],
        steps: vec![
            PlanStep {
                id: "step_1".to_string(),
                order: 0,
                description: "Independent step 1".to_string(),
                step_type: StepType::Execute,
                role: StepRole::Executor,
                parallel_safe: false,
                dependencies: vec![],
                expected_outcome: "Done".to_string(),
            },
            PlanStep {
                id: "step_2".to_string(),
                order: 1,
                description: "Independent step 2".to_string(),
                step_type: StepType::Execute,
                role: StepRole::Executor,
                parallel_safe: false,
                dependencies: vec![],
                expected_outcome: "Done".to_string(),
            },
            PlanStep {
                id: "step_3".to_string(),
                order: 2,
                description: "Depends on both".to_string(),
                step_type: StepType::Execute,
                role: StepRole::Executor,
                parallel_safe: false,
                dependencies: vec!["step_1".to_string(), "step_2".to_string()],
                expected_outcome: "Done".to_string(),
            },
        ],
        created_at: 0,
    };

    // Build DAG
    let dag = executor.build_dag(&plan.steps).unwrap();

    // Verify structure
    assert_eq!(dag.len(), 3);
    assert_eq!(dag.get("step_1").unwrap().len(), 0); // No dependencies
    assert_eq!(dag.get("step_2").unwrap().len(), 0); // No dependencies
    assert_eq!(dag.get("step_3").unwrap().len(), 2); // Depends on both
}

/// Test cycle detection in DAG
#[test]
fn test_dag_cycle_detection() {
    let config = Arc::new(LLMConfig {
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

    let router = Arc::new(LLMRouter::new(vec![], config));
    let executor = HybridExecutor::new(router, None);

    // Create a plan with a cycle
    let plan = ConductorPlan {
        id: "test_plan".to_string(),
        original_goal: "Test cycle detection".to_string(),
        mode: Default::default(),
        stages: vec![],
        steps: vec![
            PlanStep {
                id: "step_1".to_string(),
                order: 0,
                description: "Step 1".to_string(),
                step_type: StepType::Execute,
                role: StepRole::Executor,
                parallel_safe: false,
                dependencies: vec!["step_2".to_string()], // Cycle!
                expected_outcome: "Done".to_string(),
            },
            PlanStep {
                id: "step_2".to_string(),
                order: 1,
                description: "Step 2".to_string(),
                step_type: StepType::Execute,
                role: StepRole::Executor,
                parallel_safe: false,
                dependencies: vec!["step_1".to_string()], // Cycle!
                expected_outcome: "Done".to_string(),
            },
        ],
        created_at: 0,
    };

    // Should detect cycle
    let result = executor.build_dag(&plan.steps);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Cycle"));
}

/// Test plan parsing from JSON
#[test]
fn test_plan_parsing() {
    let config = Arc::new(LLMConfig {
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

    let router = Arc::new(LLMRouter::new(vec![], config));
    let executor = HybridExecutor::new(router, None);

    let json = r#"[
        {
            "id": "step_1",
            "description": "Read the file",
            "dependencies": [],
            "expected_outcome": "File contents loaded"
        },
        {
            "id": "step_2",
            "description": "Process the data",
            "dependencies": ["step_1"],
            "expected_outcome": "Data processed"
        }
    ]"#;

    let plan = executor.parse_plan(json, "Test goal").unwrap();

    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[0].id, "step_1");
    assert_eq!(plan.steps[0].role, StepRole::Executor);
    assert!(!plan.steps[0].parallel_safe);
    assert_eq!(plan.steps[1].dependencies.len(), 1);
    assert_eq!(plan.steps[1].dependencies[0], "step_1");
}
