use std::sync::Arc;

use sdk::{TaskExecutionProfile, TaskSource};
use tempfile::TempDir;

use super::{recover_crashed_tasks, Gateway, GatewayConfig, WorkspaceLocks};
use crate::db::{Database, PendingTaskStatus};

#[tokio::test]
async fn test_submit_cli_task() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(&db_path).await.unwrap());

    let gateway = Gateway::new(Arc::clone(&db), GatewayConfig::default()).unwrap();
    let task_id = gateway.submit_cli("list files", None, None).await.unwrap();

    let repo = db.pending_tasks();
    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.input, "list files");
    assert_eq!(task.source, TaskSource::Cli);
    assert_eq!(task.status, PendingTaskStatus::Pending);
}

#[tokio::test]
async fn test_submit_cli_task_preserves_execution_profile() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(&db_path).await.unwrap());

    let gateway = Gateway::new(Arc::clone(&db), GatewayConfig::default()).unwrap();
    let profile = TaskExecutionProfile {
        agent_id: Some("agent.ops".to_string()),
        agent_name: Some("Ops Agent".to_string()),
        worker_preset_id: None,
        worker_preset_name: None,
        purpose: Some("Run ops tasks".to_string()),
        instructions: "Follow the saved agent profile".to_string(),
        allowed_tools: vec!["read_file".to_string()],
        output_contract: None,
        max_iterations: Some(4),
    };
    let task_id = gateway
        .submit_cli("inspect service health", None, Some(&profile))
        .await
        .unwrap();

    let repo = db.pending_tasks();
    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.execution_profile, Some(profile));
}

#[tokio::test]
async fn test_recover_crashed_tasks() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(&db_path).await.unwrap());

    let repo = db.pending_tasks();
    repo.create_task("task-1", "first", TaskSource::Cli, None, None, None, None)
        .await
        .unwrap();
    repo.create_task("task-2", "second", TaskSource::Cli, None, None, None, None)
        .await
        .unwrap();
    repo.mark_running("task-1").await.unwrap();
    repo.mark_running("task-2").await.unwrap();

    let recovered = recover_crashed_tasks(&db).await.unwrap();
    assert_eq!(recovered, 2);

    let pending = repo.get_pending_tasks(10).await.unwrap();
    assert_eq!(pending.len(), 2);
}

#[tokio::test]
async fn test_concurrent_tasks_independent() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(&db_path).await.unwrap());
    let gateway = Arc::new(Gateway::new(Arc::clone(&db), GatewayConfig::default()).unwrap());

    let gateway1 = gateway.clone();
    let gateway2 = gateway.clone();
    let submit1 =
        tokio::spawn(async move { gateway1.submit_cli("task one", None, None).await.unwrap() });
    let submit2 =
        tokio::spawn(async move { gateway2.submit_cli("task two", None, None).await.unwrap() });

    let (task_id_1, task_id_2) = tokio::join!(submit1, submit2);
    let task_id_1 = task_id_1.unwrap();
    let task_id_2 = task_id_2.unwrap();

    let repo = db.pending_tasks();
    let task1 = repo.get_task(&task_id_1).await.unwrap().unwrap();
    let task2 = repo.get_task(&task_id_2).await.unwrap().unwrap();

    assert_eq!(task1.status, PendingTaskStatus::Pending);
    assert_eq!(task2.status, PendingTaskStatus::Pending);
    assert_ne!(task_id_1, task_id_2);
    assert_eq!(task1.input, "task one");
    assert_eq!(task2.input, "task two");

    let _ = repo.mark_done(&task_id_1).await;
    let _ = repo.mark_done(&task_id_2).await;
}

#[tokio::test]
async fn test_gateway_crash_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Arc::new(Database::new(&db_path).await.unwrap());
    let gateway = Gateway::new(Arc::clone(&db), GatewayConfig::default()).unwrap();

    let task_id = gateway
        .submit_cli("crash test task", None, None)
        .await
        .unwrap();

    let repo = db.pending_tasks();
    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.status, PendingTaskStatus::Pending);

    repo.mark_running(&task_id).await.unwrap();
    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.status, PendingTaskStatus::Running);

    let recovered = recover_crashed_tasks(&db).await.unwrap();
    assert_eq!(recovered, 1);

    let task = repo.get_task(&task_id).await.unwrap().unwrap();
    assert_eq!(task.status, PendingTaskStatus::Pending);
    let pending = repo.get_pending_tasks(10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, task_id);
}

#[tokio::test]
async fn test_workspace_lock_serializes_writes() {
    use std::time::Duration;

    let temp_dir = TempDir::new().unwrap();
    let workspace_locks = Arc::new(WorkspaceLocks::new());
    let workspace = temp_dir.path().join("test_workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let lock_order = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let lock_order_1 = lock_order.clone();
    let lock_order_2 = lock_order.clone();
    let lock = workspace_locks.get_lock(&workspace);

    let lock_1 = lock.clone();
    let task1 = tokio::spawn(async move {
        let _guard = lock_1.lock().await;
        lock_order_1.lock().await.push(1);
        tokio::time::sleep(Duration::from_millis(50)).await;
        1
    });

    let lock_2 = lock.clone();
    let task2 = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let _guard = lock_2.lock().await;
        lock_order_2.lock().await.push(2);
        tokio::time::sleep(Duration::from_millis(50)).await;
        2
    });

    let (result1, result2) = tokio::join!(task1, task2);
    assert_eq!(result1.unwrap(), 1);
    assert_eq!(result2.unwrap(), 2);

    let order = lock_order.lock().await.clone();
    assert_eq!(
        order,
        vec![1, 2],
        "Locks should be serialized, not concurrent"
    );
}
