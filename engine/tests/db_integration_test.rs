/// Integration tests for database module
///
/// Tests the complete database lifecycle including:
/// - Database creation and initialization
/// - WAL mode configuration
/// - Schema creation via migrations
/// - Graceful shutdown with WAL flush
///
/// Requirements: 12.1, 12.3, 12.7, 12.8, 12.9
use rove_engine::db::Database;
use tempfile::TempDir;

#[tokio::test]
async fn test_database_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    // Create database
    let db = Database::new(&db_path).await.unwrap();

    // Verify database file exists
    assert!(db_path.exists());

    // Verify WAL file exists (created when WAL mode is enabled)
    let wal_path = temp_dir.path().join("rove.db-wal");
    assert!(wal_path.exists());

    // Verify we can query the database
    let result = sqlx::query("SELECT COUNT(*) as count FROM tasks")
        .fetch_one(db.pool())
        .await;

    assert!(result.is_ok());

    // Close database (flushes WAL)
    db.close().await.unwrap();
}

#[tokio::test]
async fn test_database_schema_complete() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();

    // Verify all required tables exist
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .fetch_all(db.pool())
            .await
            .unwrap();

    assert!(tables.contains(&"tasks".to_string()), "tasks table missing");
    assert!(
        tables.contains(&"task_steps".to_string()),
        "task_steps table missing"
    );
    assert!(
        tables.contains(&"plugins".to_string()),
        "plugins table missing"
    );
    assert!(
        tables.contains(&"secrets_cache".to_string()),
        "secrets_cache table missing"
    );
    assert!(
        tables.contains(&"rate_limits".to_string()),
        "rate_limits table missing"
    );

    // Verify all required indexes exist
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();

    assert!(indexes.contains(&"idx_tasks_created_at".to_string()));
    assert!(indexes.contains(&"idx_tasks_status".to_string()));
    assert!(indexes.contains(&"idx_task_steps_task_id".to_string()));
    assert!(indexes.contains(&"idx_plugins_enabled".to_string()));
    assert!(indexes.contains(&"idx_secrets_cache_expires_at".to_string()));
    assert!(indexes.contains(&"idx_rate_limits_timestamp".to_string()));
    assert!(indexes.contains(&"idx_rate_limits_source_tier".to_string()));

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_tasks_table_constraints() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();

    // Test valid status values
    let result =
        sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
            .bind("test-1")
            .bind("test input")
            .bind("pending")
            .bind(1234567890i64)
            .execute(db.pool())
            .await;

    assert!(result.is_ok());

    // Test invalid status value (should fail CHECK constraint)
    let result =
        sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
            .bind("test-2")
            .bind("test input")
            .bind("invalid_status")
            .bind(1234567890i64)
            .execute(db.pool())
            .await;

    assert!(result.is_err());

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_foreign_key_constraint() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();

    // Try to insert a task_step without a corresponding task (should fail)
    let result = sqlx::query(
        "INSERT INTO task_steps (task_id, step_order, step_type, content, created_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind("nonexistent-task")
    .bind(1)
    .bind("user_message")
    .bind("test content")
    .bind(1234567890i64)
    .execute(db.pool())
    .await;

    assert!(result.is_err());

    // Create a task first
    sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
        .bind("test-task")
        .bind("test input")
        .bind("pending")
        .bind(1234567890i64)
        .execute(db.pool())
        .await
        .unwrap();

    // Now insert a task_step (should succeed)
    let result = sqlx::query(
        "INSERT INTO task_steps (task_id, step_order, step_type, content, created_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind("test-task")
    .bind(1)
    .bind("user_message")
    .bind("test content")
    .bind(1234567890i64)
    .execute(db.pool())
    .await;

    assert!(result.is_ok());

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_cascade_delete() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();

    // Create a task
    sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
        .bind("test-task")
        .bind("test input")
        .bind("pending")
        .bind(1234567890i64)
        .execute(db.pool())
        .await
        .unwrap();

    // Create task steps
    for i in 1..=3 {
        sqlx::query(
            "INSERT INTO task_steps (task_id, step_order, step_type, content, created_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind("test-task")
        .bind(i)
        .bind("user_message")
        .bind(format!("step {}", i))
        .bind(1234567890i64)
        .execute(db.pool())
        .await
        .unwrap();
    }

    // Verify steps exist
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_steps WHERE task_id = ?")
        .bind("test-task")
        .fetch_one(db.pool())
        .await
        .unwrap();

    assert_eq!(count, 3);

    // Delete the task
    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind("test-task")
        .execute(db.pool())
        .await
        .unwrap();

    // Verify steps were cascade deleted
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_steps WHERE task_id = ?")
        .bind("test-task")
        .fetch_one(db.pool())
        .await
        .unwrap();

    assert_eq!(count, 0);

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_wal_flush_on_close() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");
    let wal_path = temp_dir.path().join("rove.db-wal");

    // Create database and insert data
    {
        let db = Database::new(&db_path).await.unwrap();

        sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
            .bind("test-task")
            .bind("test input")
            .bind("pending")
            .bind(1234567890i64)
            .execute(db.pool())
            .await
            .unwrap();

        // WAL file should exist
        assert!(wal_path.exists());

        // Close database (should flush WAL)
        db.close().await.unwrap();
    }

    // After close, WAL should be flushed and truncated
    // Note: WAL file may still exist but should be empty or very small
    if wal_path.exists() {
        let metadata = std::fs::metadata(&wal_path).unwrap();
        // WAL file should be empty or minimal after checkpoint
        assert!(metadata.len() < 1024, "WAL file not properly flushed");
    }

    // Verify data persisted by reopening database
    let db = Database::new(&db_path).await.unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id = ?")
        .bind("test-task")
        .fetch_one(db.pool())
        .await
        .unwrap();

    assert_eq!(count, 1);

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_wal_recovery_after_unclean_shutdown() {
    // This test verifies Requirement 12.9: Recover from WAL on startup after unclean shutdown
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");
    let wal_path = temp_dir.path().join("rove.db-wal");

    // Simulate unclean shutdown by NOT calling close()
    {
        let db = Database::new(&db_path).await.unwrap();

        // Insert data that will be in the WAL
        sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
            .bind("task-1")
            .bind("test input 1")
            .bind("pending")
            .bind(1234567890i64)
            .execute(db.pool())
            .await
            .unwrap();

        sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
            .bind("task-2")
            .bind("test input 2")
            .bind("running")
            .bind(1234567891i64)
            .execute(db.pool())
            .await
            .unwrap();

        // Verify WAL file exists with data
        assert!(wal_path.exists());
        let wal_size = std::fs::metadata(&wal_path).unwrap().len();
        assert!(wal_size > 0, "WAL should contain uncommitted data");

        // Drop database WITHOUT calling close() to simulate unclean shutdown
        // This leaves the WAL file with uncommitted transactions
        drop(db);
    }

    // WAL file should still exist after unclean shutdown
    assert!(
        wal_path.exists(),
        "WAL file should exist after unclean shutdown"
    );

    // Reopen database - SQLite should automatically recover from WAL
    let db = Database::new(&db_path).await.unwrap();

    // Verify data was recovered from WAL
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(db.pool())
        .await
        .unwrap();

    assert_eq!(count, 2, "Both tasks should be recovered from WAL");

    // Verify specific tasks
    let task1 = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, input, status FROM tasks WHERE id = ?",
    )
    .bind("task-1")
    .fetch_one(db.pool())
    .await
    .unwrap();

    assert_eq!(task1.0, "task-1");
    assert_eq!(task1.1, "test input 1");
    assert_eq!(task1.2, "pending");

    let task2 = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, input, status FROM tasks WHERE id = ?",
    )
    .bind("task-2")
    .fetch_one(db.pool())
    .await
    .unwrap();

    assert_eq!(task2.0, "task-2");
    assert_eq!(task2.1, "test input 2");
    assert_eq!(task2.2, "running");

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_wal_recovery_with_multiple_transactions() {
    // Test WAL recovery with multiple uncommitted transactions
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    // Simulate unclean shutdown with multiple transactions
    {
        let db = Database::new(&db_path).await.unwrap();

        // Transaction 1: Create task
        sqlx::query("INSERT INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)")
            .bind("task-1")
            .bind("test input")
            .bind("pending")
            .bind(1234567890i64)
            .execute(db.pool())
            .await
            .unwrap();

        // Transaction 2: Add steps
        for i in 1..=3 {
            sqlx::query(
                "INSERT INTO task_steps (task_id, step_order, step_type, content, created_at) VALUES (?, ?, ?, ?, ?)"
            )
            .bind("task-1")
            .bind(i)
            .bind("user_message")
            .bind(format!("step {}", i))
            .bind(1234567890i64)
            .execute(db.pool())
            .await
            .unwrap();
        }

        // Transaction 3: Update task status
        sqlx::query("UPDATE tasks SET status = ? WHERE id = ?")
            .bind("running")
            .bind("task-1")
            .execute(db.pool())
            .await
            .unwrap();

        // Drop without close() - unclean shutdown
        drop(db);
    }

    // Reopen and verify all transactions recovered
    let db = Database::new(&db_path).await.unwrap();

    // Verify task exists with updated status
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = ?")
        .bind("task-1")
        .fetch_one(db.pool())
        .await
        .unwrap();

    assert_eq!(status, "running");

    // Verify all steps recovered
    let step_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_steps WHERE task_id = ?")
        .bind("task-1")
        .fetch_one(db.pool())
        .await
        .unwrap();

    assert_eq!(step_count, 3);

    db.close().await.unwrap();
}

// ============================================================================
// Task Repository Tests
// ============================================================================

#[tokio::test]
async fn test_create_and_get_task() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    let task_id = uuid::Uuid::new_v4();

    // Create a task
    let task = repo.create_task(&task_id, "test input").await.unwrap();

    assert_eq!(task.id, task_id);
    assert_eq!(task.input, "test input");
    assert_eq!(task.status, rove_engine::db::TaskStatus::Pending);
    assert!(task.provider_used.is_none());
    assert!(task.duration_ms.is_none());
    assert!(task.completed_at.is_none());

    // Retrieve the task
    let retrieved = repo.get_task(&task_id).await.unwrap().unwrap();

    assert_eq!(retrieved.id, task.id);
    assert_eq!(retrieved.input, task.input);
    assert_eq!(retrieved.status, task.status);

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_update_task_status() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    let task_id = uuid::Uuid::new_v4();

    // Create a task
    repo.create_task(&task_id, "test input").await.unwrap();

    // Update status to running
    repo.update_task_status(&task_id, rove_engine::db::TaskStatus::Running)
        .await
        .unwrap();

    // Verify status changed
    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.status, rove_engine::db::TaskStatus::Running);

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_complete_task() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    let task_id = uuid::Uuid::new_v4();

    // Create a task
    repo.create_task(&task_id, "test input").await.unwrap();

    // Complete the task
    repo.complete_task(&task_id, "ollama", 1500).await.unwrap();

    // Verify task completed
    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.status, rove_engine::db::TaskStatus::Completed);
    assert_eq!(task.provider_used, Some("ollama".to_string()));
    assert_eq!(task.duration_ms, Some(1500));
    assert!(task.completed_at.is_some());

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_fail_task() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    let task_id = uuid::Uuid::new_v4();

    // Create a task
    repo.create_task(&task_id, "test input").await.unwrap();

    // Fail the task
    repo.fail_task(&task_id).await.unwrap();

    // Verify task failed
    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.status, rove_engine::db::TaskStatus::Failed);
    assert!(task.completed_at.is_some());

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_get_recent_tasks() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    // Create multiple tasks
    let mut task_ids = Vec::new();
    for i in 1..=5 {
        let task_id = uuid::Uuid::new_v4();
        task_ids.push(task_id);
        repo.create_task(&task_id, &format!("input {}", i))
            .await
            .unwrap();
    }

    // Get recent tasks (limit 3)
    let tasks = repo.get_recent_tasks(3).await.unwrap();

    // Should return 3 tasks
    assert_eq!(tasks.len(), 3);

    // All returned tasks should be from our created set
    for task in &tasks {
        assert!(task_ids.contains(&task.id));
    }

    // Get all tasks to verify total count
    let all_tasks = repo.get_recent_tasks(10).await.unwrap();
    assert_eq!(all_tasks.len(), 5);

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_add_and_get_task_steps() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    let task_id = uuid::Uuid::new_v4();

    // Create a task
    repo.create_task(&task_id, "test input").await.unwrap();

    // Add steps
    let step1 = repo
        .add_task_step(
            &task_id,
            1,
            rove_engine::db::StepType::UserMessage,
            "user message",
        )
        .await
        .unwrap();

    let step2 = repo
        .add_task_step(
            &task_id,
            2,
            rove_engine::db::StepType::AssistantMessage,
            "assistant response",
        )
        .await
        .unwrap();

    let step3 = repo
        .add_task_step(
            &task_id,
            3,
            rove_engine::db::StepType::ToolCall,
            "tool call data",
        )
        .await
        .unwrap();

    assert!(step1.id.is_some());
    assert!(step2.id.is_some());
    assert!(step3.id.is_some());

    // Retrieve steps
    let steps = repo.get_task_steps(&task_id).await.unwrap();

    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].step_order, 1);
    assert_eq!(steps[0].step_type, rove_engine::db::StepType::UserMessage);
    assert_eq!(steps[0].content, "user message");

    assert_eq!(steps[1].step_order, 2);
    assert_eq!(
        steps[1].step_type,
        rove_engine::db::StepType::AssistantMessage
    );
    assert_eq!(steps[1].content, "assistant response");

    assert_eq!(steps[2].step_order, 3);
    assert_eq!(steps[2].step_type, rove_engine::db::StepType::ToolCall);
    assert_eq!(steps[2].content, "tool call data");

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_delete_old_tasks() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    let task_id_1 = uuid::Uuid::new_v4();
    let task_id_2 = uuid::Uuid::new_v4();

    // Create tasks
    repo.create_task(&task_id_1, "input 1").await.unwrap();
    repo.create_task(&task_id_2, "input 2").await.unwrap();

    // Wait a moment to ensure tasks are in the past
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Delete tasks older than -1 days (i.e., all tasks created before tomorrow, which is all of them)
    let deleted = repo.delete_old_tasks(-1).await.unwrap();

    assert_eq!(deleted, 2);

    // Verify tasks deleted
    let tasks = repo.get_recent_tasks(10).await.unwrap();
    assert_eq!(tasks.len(), 0);

    db.close().await.unwrap();
}

// ============================================================================
// Plugin Repository Tests
// ============================================================================

#[tokio::test]
async fn test_register_and_get_plugin() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register a plugin
    let plugin = repo
        .register_plugin(
            "plugin-1",
            "fs-editor",
            "0.1.0",
            "/path/to/fs_editor.wasm",
            "abc123hash",
            r#"{"name":"fs-editor"}"#,
        )
        .await
        .unwrap();

    assert_eq!(plugin.id, "plugin-1");
    assert_eq!(plugin.name, "fs-editor");
    assert_eq!(plugin.version, "0.1.0");
    assert_eq!(plugin.wasm_path, "/path/to/fs_editor.wasm");
    assert_eq!(plugin.wasm_hash, "abc123hash");
    assert!(plugin.enabled);

    // Retrieve the plugin
    let retrieved = repo.get_plugin("plugin-1").await.unwrap().unwrap();

    assert_eq!(retrieved.id, plugin.id);
    assert_eq!(retrieved.name, plugin.name);
    assert_eq!(retrieved.version, plugin.version);

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_get_plugin_by_name() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register a plugin
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "abc123hash",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    // Retrieve by name
    let plugin = repo.get_plugin_by_name("fs-editor").await.unwrap().unwrap();

    assert_eq!(plugin.id, "plugin-1");
    assert_eq!(plugin.name, "fs-editor");

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_update_plugin() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register a plugin
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "abc123hash",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    // Update the plugin
    repo.update_plugin(
        "plugin-1",
        "0.2.0",
        "/path/to/fs_editor_v2.wasm",
        "def456hash",
        r#"{"name":"fs-editor","version":"0.2.0"}"#,
    )
    .await
    .unwrap();

    // Verify update
    let plugin = repo.get_plugin("plugin-1").await.unwrap().unwrap();

    assert_eq!(plugin.version, "0.2.0");
    assert_eq!(plugin.wasm_path, "/path/to/fs_editor_v2.wasm");
    assert_eq!(plugin.wasm_hash, "def456hash");

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_set_plugin_enabled() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register a plugin
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "abc123hash",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    // Disable the plugin
    repo.set_plugin_enabled("plugin-1", false).await.unwrap();

    // Verify disabled
    let plugin = repo.get_plugin("plugin-1").await.unwrap().unwrap();
    assert!(!plugin.enabled);

    // Re-enable the plugin
    repo.set_plugin_enabled("plugin-1", true).await.unwrap();

    // Verify enabled
    let plugin = repo.get_plugin("plugin-1").await.unwrap().unwrap();
    assert!(plugin.enabled);

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_get_all_plugins() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register multiple plugins
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "hash1",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    repo.register_plugin(
        "plugin-2",
        "terminal",
        "0.1.0",
        "/path/to/terminal.wasm",
        "hash2",
        r#"{"name":"terminal"}"#,
    )
    .await
    .unwrap();

    repo.register_plugin(
        "plugin-3",
        "git",
        "0.1.0",
        "/path/to/git.wasm",
        "hash3",
        r#"{"name":"git"}"#,
    )
    .await
    .unwrap();

    // Get all plugins
    let plugins = repo.get_all_plugins().await.unwrap();

    assert_eq!(plugins.len(), 3);
    // Should be sorted by name
    assert_eq!(plugins[0].name, "fs-editor");
    assert_eq!(plugins[1].name, "git");
    assert_eq!(plugins[2].name, "terminal");

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_get_enabled_plugins() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register multiple plugins
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "hash1",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    repo.register_plugin(
        "plugin-2",
        "terminal",
        "0.1.0",
        "/path/to/terminal.wasm",
        "hash2",
        r#"{"name":"terminal"}"#,
    )
    .await
    .unwrap();

    // Disable one plugin
    repo.set_plugin_enabled("plugin-2", false).await.unwrap();

    // Get enabled plugins
    let plugins = repo.get_enabled_plugins().await.unwrap();

    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "fs-editor");

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_delete_plugin() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register a plugin
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "abc123hash",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    // Verify plugin exists
    assert!(repo.get_plugin("plugin-1").await.unwrap().is_some());

    // Delete the plugin
    repo.delete_plugin("plugin-1").await.unwrap();

    // Verify plugin deleted
    assert!(repo.get_plugin("plugin-1").await.unwrap().is_none());

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_plugin_exists() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Check non-existent plugin
    assert!(!repo.plugin_exists("fs-editor").await.unwrap());

    // Register a plugin
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "abc123hash",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    // Check existing plugin
    assert!(repo.plugin_exists("fs-editor").await.unwrap());

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_plugin_name_unique_constraint() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Register a plugin
    repo.register_plugin(
        "plugin-1",
        "fs-editor",
        "0.1.0",
        "/path/to/fs_editor.wasm",
        "abc123hash",
        r#"{"name":"fs-editor"}"#,
    )
    .await
    .unwrap();

    // Try to register another plugin with the same name (should fail)
    let result = repo
        .register_plugin(
            "plugin-2",
            "fs-editor",
            "0.2.0",
            "/path/to/fs_editor_v2.wasm",
            "def456hash",
            r#"{"name":"fs-editor"}"#,
        )
        .await;

    assert!(result.is_err());

    db.close().await.unwrap();
}

// ============================================================================
// SQL Injection Prevention Tests
// ============================================================================

#[tokio::test]
async fn test_sql_injection_prevention_in_task_input() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.tasks();

    let task_id = uuid::Uuid::new_v4();

    // Try to inject SQL via task input
    let malicious_input = "'; DROP TABLE tasks; --";

    // This should be safely parameterized
    let task = repo.create_task(&task_id, malicious_input).await.unwrap();

    assert_eq!(task.input, malicious_input);

    // Verify tasks table still exists
    let result = sqlx::query("SELECT COUNT(*) FROM tasks")
        .fetch_one(db.pool())
        .await;

    assert!(result.is_ok());

    db.close().await.unwrap();
}

#[tokio::test]
async fn test_sql_injection_prevention_in_plugin_name() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("rove.db");

    let db = Database::new(&db_path).await.unwrap();
    let repo = db.plugins();

    // Try to inject SQL via plugin name
    let malicious_name = "'; DROP TABLE plugins; --";

    // This should be safely parameterized
    let plugin = repo
        .register_plugin(
            "plugin-1",
            malicious_name,
            "0.1.0",
            "/path/to/plugin.wasm",
            "hash",
            r#"{"name":"test"}"#,
        )
        .await
        .unwrap();

    assert_eq!(plugin.name, malicious_name);

    // Verify plugins table still exists
    let result = sqlx::query("SELECT COUNT(*) FROM plugins")
        .fetch_one(db.pool())
        .await;

    assert!(result.is_ok());

    db.close().await.unwrap();
}
