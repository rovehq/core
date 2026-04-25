use anyhow::Result;
use rove_engine::conductor::memory::{HitType, MemorySystem};
use rove_engine::conductor::types::TaskDomain;
use rove_engine::config::{MemoryConfig, MemoryMode};
use rove_engine::llm::ollama::OllamaProvider;
use rove_engine::llm::router::LLMRouter;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn memory_system_for_test(pool: SqlitePool, router: Arc<LLMRouter>) -> MemorySystem {
    MemorySystem::new_with_config(
        pool,
        router,
        MemoryConfig {
            mode: MemoryMode::AlwaysOn,
            ..MemoryConfig::default()
        },
    )
}

#[tokio::test]
async fn test_memory_pipeline_mock_llm() -> Result<()> {
    // 1. Setup Mock Server
    let mock_server = MockServer::start().await;

    // Mock the health check endpoint
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "models": [
                { "name": "mock-model" }
            ]
        })))
        .mount(&mock_server)
        .await;

    // Mock the LLM completion endpoint for ingest()
    // Ingest expects a JSON blob with summary, entities, topics, importance
    let ingest_response = serde_json::json!({
        "summary": "User prefers python3",
        "entities": ["python3", "scripting"],
        "topics": ["preferences", "language"],
        "importance": 0.8
    });

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "model": "mock-model",
            "message": {
                "role": "assistant",
                "content": ingest_response.to_string()
            },
            "done": true
        })))
        .mount(&mock_server)
        .await;

    // 2. Configure mock LLM Router
    let provider = Box::new(OllamaProvider::new(mock_server.uri(), "mock-model").unwrap());
    let router = Arc::new(LLMRouter::new(
        vec![provider],
        Arc::new(rove_engine::config::Config::default().llm),
    ));

    // 3. Setup ephemeral SQLite FTS5 database with full Phase 2 schema
    let pool = setup_test_db().await?;

    // 4. Initialize MemorySystem
    let memory_system = memory_system_for_test(pool.clone(), router.clone());

    // 5. Test Ingest
    let result = memory_system
        .ingest(
            "Write a script to do X",
            "Okay, using python3",
            "task_123",
            &TaskDomain::Code,
            false,
        )
        .await?;

    assert_eq!(result.summary, "User prefers python3");
    assert_eq!(result.topics.len(), 2);

    // Verify database directly
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM episodic_memory")
        .fetch_one(&pool)
        .await?;
    assert_eq!(row.0, 1, "Should have 1 episodic memory");

    // We must flush FTS triggers and force a second ingest to test consolidation threshold
    // Insert dummy records to hit the >= 3 consolidation threshold
    let now = chrono::Utc::now().timestamp();
    sqlx::query("INSERT INTO episodic_memory (id, task_id, summary, entities, topics, importance, consolidated, created_at, domain) VALUES ('id1', 'task2', 'summary1', '[]', '[]', 0.5, 0, ?, 'code')").bind(now).execute(&pool).await?;
    sqlx::query("INSERT INTO episodic_memory (id, task_id, summary, entities, topics, importance, consolidated, created_at, domain) VALUES ('id2', 'task3', 'summary2', '[]', '[]', 0.5, 0, ?, 'code')").bind(now).execute(&pool).await?;
    sqlx::query("INSERT INTO episodic_memory (id, task_id, summary, entities, topics, importance, consolidated, created_at, domain) VALUES ('id3', 'task4', 'summary3', '[]', '[]', 0.5, 0, ?, 'code')").bind(now).execute(&pool).await?;

    // Reset Mock for Consolidation response
    mock_server.reset().await;

    // Remount LLM completion endpoint for consolidate()
    let insight_response = serde_json::json!([
        {
            "insight": "General pattern insight",
            "source_ids": ["id1", "id2", "id3"]
        }
    ]);

    // Remount Health check for router's provider check
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "models": [{ "name": "mock-model" }]
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "model": "mock-model",
            "message": {
                "role": "assistant",
                "content": insight_response.to_string()
            },
            "done": true
        })))
        .mount(&mock_server)
        .await;

    // 6. Test Consolidation
    let cons_result = memory_system.consolidate().await?;
    match cons_result {
        rove_engine::conductor::memory::ConsolidationResult::Completed {
            insights_generated,
            ..
        } => {
            assert_eq!(insights_generated, 1, "Should have generated 1 insight");
        }
        _ => panic!("Expected Completed, got {:?}", cons_result),
    }

    // Verify insights DB
    let insights_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM consolidation_insights")
        .fetch_one(&pool)
        .await?;
    assert_eq!(insights_count.0, 1, "Should have 1 insight row");

    // Verify records marked as consolidated
    let unconsolidated_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM episodic_memory WHERE consolidated = FALSE")
            .fetch_one(&pool)
            .await?;
    assert_eq!(
        unconsolidated_count.0, 0,
        "All rows should now be consolidated"
    );

    // 7. Test Query
    let hits = memory_system
        .query("python3 scripting", &TaskDomain::Code, None)
        .await?;

    // Should get at least 1 result back from the FTS table based on the ingest we triggered earlier
    assert!(
        !hits.is_empty(),
        "Memory query should return hits for 'python3'"
    );

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// Task 14.1: Integration tests for complete memory workflow
// ─────────────────────────────────────────────────────────────────────

/// Helper function to setup test database with full schema including migration 004
async fn setup_test_db() -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;

    sqlx::raw_sql(include_str!("../schemas/base.sql"))
        .execute(&pool)
        .await?;

    Ok(pool)
}

/// Helper function to setup mock LLM server
async fn setup_mock_llm(
    ingest_response: serde_json::Value,
    _consolidate_response: serde_json::Value,
) -> (MockServer, Arc<LLMRouter>) {
    let mock_server = MockServer::start().await;

    // Mock health check
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "models": [{ "name": "mock-model" }]
        })))
        .mount(&mock_server)
        .await;

    // Mock ingest response
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "model": "mock-model",
            "message": {
                "role": "assistant",
                "content": ingest_response.to_string()
            },
            "done": true
        })))
        .mount(&mock_server)
        .await;

    let provider = Box::new(OllamaProvider::new(mock_server.uri(), "mock-model").unwrap());
    let router = Arc::new(LLMRouter::new(
        vec![provider],
        Arc::new(rove_engine::config::Config::default().llm),
    ));

    (mock_server, router)
}

#[tokio::test]
async fn test_roundtrip_summary_preserved() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Fixed authentication bug in login module",
        "entities": ["auth", "login"],
        "topics": ["security", "bugfix"],
        "importance": 0.9
    });

    let (_mock_server, router) =
        setup_mock_llm(ingest_response.clone(), serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest
    let result = memory_system
        .ingest(
            "Fix the auth bug",
            "Fixed by updating token validation",
            "task_001",
            &TaskDomain::Code,
            false,
        )
        .await?;

    assert_eq!(result.summary, "Fixed authentication bug in login module");

    // Query
    let hits = memory_system
        .query("authentication", &TaskDomain::Code, None)
        .await?;

    assert!(!hits.is_empty(), "Should find memory by summary");
    assert_eq!(hits[0].content, "Fixed authentication bug in login module");

    Ok(())
}

#[tokio::test]
async fn test_roundtrip_entities_preserved() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Updated database schema",
        "entities": ["PostgreSQL", "SQLAlchemy", "migrations"],
        "topics": ["database"],
        "importance": 0.7
    });

    let (_mock_server, router) =
        setup_mock_llm(ingest_response.clone(), serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest
    let result = memory_system
        .ingest(
            "Update DB schema",
            "Added new tables",
            "task_002",
            &TaskDomain::Code,
            false,
        )
        .await?;

    assert_eq!(
        result.entities,
        vec!["PostgreSQL", "SQLAlchemy", "migrations"]
    );

    // Verify in database
    let row = sqlx::query("SELECT entities FROM episodic_memory WHERE task_id = ?")
        .bind("task_002")
        .fetch_one(&pool)
        .await?;

    let entities_json: String = row.get("entities");
    let entities: Vec<String> = serde_json::from_str(&entities_json)?;
    assert_eq!(entities, vec!["PostgreSQL", "SQLAlchemy", "migrations"]);

    Ok(())
}

#[tokio::test]
async fn test_roundtrip_topics_preserved() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Implemented caching layer",
        "entities": ["Redis", "cache"],
        "topics": ["performance", "optimization", "infrastructure"],
        "importance": 0.8
    });

    let (_mock_server, router) =
        setup_mock_llm(ingest_response.clone(), serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest
    let result = memory_system
        .ingest(
            "Add caching",
            "Implemented Redis cache",
            "task_003",
            &TaskDomain::Code,
            false,
        )
        .await?;

    assert_eq!(
        result.topics,
        vec!["performance", "optimization", "infrastructure"]
    );

    // Verify in database
    let row = sqlx::query("SELECT topics FROM episodic_memory WHERE task_id = ?")
        .bind("task_003")
        .fetch_one(&pool)
        .await?;

    let topics_json: String = row.get("topics");
    let topics: Vec<String> = serde_json::from_str(&topics_json)?;
    assert_eq!(
        topics,
        vec!["performance", "optimization", "infrastructure"]
    );

    Ok(())
}

#[tokio::test]
async fn test_roundtrip_importance_preserved() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Critical security patch",
        "entities": ["security"],
        "topics": ["security"],
        "importance": 0.95
    });

    let (_mock_server, router) =
        setup_mock_llm(ingest_response.clone(), serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest
    let result = memory_system
        .ingest(
            "Apply security patch",
            "Patched CVE-2024-1234",
            "task_004",
            &TaskDomain::Code,
            false,
        )
        .await?;

    assert_eq!(result.importance, 0.95);

    // Query and verify importance
    let hits = memory_system
        .query("security", &TaskDomain::Code, None)
        .await?;

    assert!(!hits.is_empty());
    assert!((hits[0].importance - 0.95).abs() < 0.01);

    Ok(())
}

#[tokio::test]
async fn test_roundtrip_domain_preserved() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Committed changes to feature branch",
        "entities": ["git", "branch"],
        "topics": ["version-control"],
        "importance": 0.5
    });

    let (_mock_server, router) =
        setup_mock_llm(ingest_response.clone(), serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with Git domain
    memory_system
        .ingest(
            "Commit changes",
            "Committed to feature-x",
            "task_005",
            &TaskDomain::Git,
            false,
        )
        .await?;

    // Verify domain in database
    let row = sqlx::query("SELECT domain FROM episodic_memory WHERE task_id = ?")
        .bind("task_005")
        .fetch_one(&pool)
        .await?;

    let domain: String = row.get("domain");
    assert_eq!(domain, "git");

    // Query with Git domain should find it
    let hits = memory_system
        .query("branch", &TaskDomain::Git, None)
        .await?;
    assert!(!hits.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_roundtrip_sensitive_flag_preserved() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Updated API keys in config",
        "entities": ["API", "credentials"],
        "topics": ["configuration"],
        "importance": 0.8
    });

    let (_mock_server, router) =
        setup_mock_llm(ingest_response.clone(), serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with sensitive=true
    memory_system
        .ingest(
            "Update API keys",
            "Rotated production keys",
            "task_006",
            &TaskDomain::Code,
            true, // sensitive
        )
        .await?;

    // Verify sensitive flag in database
    let row = sqlx::query("SELECT sensitive FROM episodic_memory WHERE task_id = ?")
        .bind("task_006")
        .fetch_one(&pool)
        .await?;

    let sensitive: i32 = row.get("sensitive");
    assert_eq!(sensitive, 1);

    // Query should NOT return sensitive memories
    let hits = memory_system
        .query("API keys", &TaskDomain::Code, None)
        .await?;
    assert!(hits.is_empty(), "Sensitive memories should be filtered out");

    Ok(())
}

#[tokio::test]
async fn test_end_to_end_workflow_ingest_consolidate_query_decay() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Test memory",
        "entities": ["test"],
        "topics": ["testing"],
        "importance": 0.6
    });

    let consolidate_response = serde_json::json!([
        {
            "insight": "User frequently works with testing frameworks",
            "source_ids": ["mem1", "mem2", "mem3"]
        }
    ]);

    let (mock_server, router) = setup_mock_llm(ingest_response, consolidate_response).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Step 1: Ingest multiple memories
    for i in 1..=4 {
        memory_system
            .ingest(
                &format!("Test task {}", i),
                &format!("Test result {}", i),
                &format!("task_{:03}", i),
                &TaskDomain::Code,
                false,
            )
            .await?;
    }

    // Verify ingest
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM episodic_memory")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count.0, 4);

    // Step 2: Consolidate
    // Reset mock for consolidation
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "models": [{ "name": "mock-model" }]
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "model": "mock-model",
            "message": {
                "role": "assistant",
                "content": serde_json::json!([
                    {
                        "insight": "User frequently works with testing frameworks",
                        "source_ids": []
                    }
                ]).to_string()
            },
            "done": true
        })))
        .mount(&mock_server)
        .await;

    let cons_result = memory_system.consolidate().await?;
    match cons_result {
        rove_engine::conductor::memory::ConsolidationResult::Completed {
            insights_generated,
            ..
        } => {
            assert!(insights_generated > 0);
        }
        _ => panic!("Expected consolidation to complete"),
    }

    // Step 3: Query
    let hits = memory_system
        .query("testing", &TaskDomain::Code, None)
        .await?;
    assert!(!hits.is_empty());

    // Find an episodic hit to verify access tracking
    let episodic_hit = hits
        .iter()
        .find(|h| matches!(h.hit_type, HitType::Episodic));

    if let Some(hit) = episodic_hit {
        // Verify access tracking was updated
        let row =
            sqlx::query("SELECT access_count, last_accessed FROM episodic_memory WHERE id = ?")
                .bind(&hit.id)
                .fetch_one(&pool)
                .await?;

        let access_count: i32 = row.get("access_count");
        let last_accessed: Option<i64> = row.get("last_accessed");

        assert_eq!(access_count, 1);
        assert!(last_accessed.is_some());
    }

    // Step 4: Decay (already called by consolidate, but test explicitly)
    memory_system.decay_importance(true).await?;

    Ok(())
}

#[tokio::test]
async fn test_domain_gated_context_assembly_code_domain() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Code domain memory",
        "entities": ["rust"],
        "topics": ["programming"],
        "importance": 0.7
    });

    let (_mock_server, router) = setup_mock_llm(ingest_response, serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with Code domain
    memory_system
        .ingest(
            "Write Rust code",
            "Implemented feature",
            "task_code",
            &TaskDomain::Code,
            false,
        )
        .await?;

    // Query with Code domain - should find it (episodic layer active)
    let hits = memory_system.query("rust", &TaskDomain::Code, None).await?;
    assert!(!hits.is_empty());
    assert!(matches!(hits[0].hit_type, HitType::Episodic));

    Ok(())
}

#[tokio::test]
async fn test_domain_gated_context_assembly_shell_domain() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Shell command executed",
        "entities": ["bash"],
        "topics": ["shell"],
        "importance": 0.5
    });

    let (_mock_server, router) = setup_mock_llm(ingest_response, serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with Shell domain
    memory_system
        .ingest(
            "Run shell command",
            "Executed ls -la",
            "task_shell",
            &TaskDomain::Shell,
            false,
        )
        .await?;

    // Query with Shell domain - episodic layer is DISABLED for Shell
    // So we should get no results even though the memory exists
    let hits = memory_system
        .query("bash", &TaskDomain::Shell, None)
        .await?;

    // Shell domain has episodic=false, so no episodic memories should be returned
    let episodic_hits: Vec<_> = hits
        .iter()
        .filter(|h| matches!(h.hit_type, HitType::Episodic))
        .collect();
    assert!(
        episodic_hits.is_empty(),
        "Shell domain should not return episodic memories"
    );

    Ok(())
}

#[tokio::test]
async fn test_domain_gated_context_assembly_git_domain() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Git operation performed",
        "entities": ["git", "commit"],
        "topics": ["version-control"],
        "importance": 0.6
    });

    let (_mock_server, router) = setup_mock_llm(ingest_response, serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with Git domain
    memory_system
        .ingest(
            "Git commit",
            "Committed changes",
            "task_git",
            &TaskDomain::Git,
            false,
        )
        .await?;

    // Query with Git domain - should find it (episodic and insights layers active)
    let hits = memory_system
        .query("git commit", &TaskDomain::Git, None)
        .await?;
    assert!(!hits.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_domain_gated_context_assembly_general_domain() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "General task completed",
        "entities": ["general"],
        "topics": ["misc"],
        "importance": 0.5
    });

    let (_mock_server, router) = setup_mock_llm(ingest_response, serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with General domain
    memory_system
        .ingest(
            "General task",
            "Completed",
            "task_general",
            &TaskDomain::General,
            false,
        )
        .await?;

    // Query with General domain - should find it (episodic and insights active, task_trace inactive)
    let hits = memory_system
        .query("general", &TaskDomain::General, None)
        .await?;
    assert!(!hits.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_domain_gated_context_assembly_browser_domain() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Browser task completed",
        "entities": ["web", "browser"],
        "topics": ["web"],
        "importance": 0.5
    });

    let (_mock_server, router) = setup_mock_llm(ingest_response, serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with Browser domain
    memory_system
        .ingest(
            "Browser task",
            "Navigated to page",
            "task_browser",
            &TaskDomain::Browser,
            false,
        )
        .await?;

    // Query with Browser domain - should find it (episodic active, insights inactive)
    let hits = memory_system
        .query("browser", &TaskDomain::Browser, None)
        .await?;
    assert!(!hits.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_domain_gated_context_assembly_data_domain() -> Result<()> {
    let pool = setup_test_db().await?;

    let ingest_response = serde_json::json!({
        "summary": "Data processing task",
        "entities": ["data", "processing"],
        "topics": ["data"],
        "importance": 0.6
    });

    let (_mock_server, router) = setup_mock_llm(ingest_response, serde_json::json!([])).await;
    let memory_system = memory_system_for_test(pool.clone(), router);

    // Ingest with Data domain
    memory_system
        .ingest(
            "Process data",
            "Processed dataset",
            "task_data",
            &TaskDomain::Data,
            false,
        )
        .await?;

    // Query with Data domain - should find it (episodic active, insights inactive)
    let hits = memory_system
        .query("data processing", &TaskDomain::Data, None)
        .await?;
    assert!(!hits.is_empty());

    Ok(())
}
