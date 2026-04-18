use anyhow::{Context, Result};
use sqlx::Row;

use crate::storage::Database;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetricsSnapshot {
    pub tasks_total: i64,
    pub tasks_pending: i64,
    pub tasks_running: i64,
    pub tasks_completed: i64,
    pub tasks_failed: i64,
    pub tool_calls_total: i64,
    pub task_error_rate: f64,
    pub task_latency_avg_ms: f64,
    pub active_sessions: i64,
}

pub async fn collect_metrics(db: &Database) -> Result<MetricsSnapshot> {
    let task_rows = sqlx::query(
        r#"SELECT status, COUNT(*) AS count
           FROM tasks
           GROUP BY status"#,
    )
    .fetch_all(db.pool())
    .await
    .context("Failed to query task metrics")?;

    let mut snapshot = MetricsSnapshot::default();

    for row in task_rows {
        let status: String = row.get("status");
        let count: i64 = row.get("count");
        snapshot.tasks_total += count;
        match status.as_str() {
            "pending" => snapshot.tasks_pending = count,
            "running" => snapshot.tasks_running = count,
            "completed" => snapshot.tasks_completed = count,
            "failed" => snapshot.tasks_failed = count,
            _ => {}
        }
    }

    snapshot.tool_calls_total =
        sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE event_type = 'tool_call'")
            .fetch_one(db.pool())
            .await
            .context("Failed to query tool call metrics")?;

    snapshot.task_latency_avg_ms = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(duration_ms) FROM tasks WHERE status = 'completed' AND duration_ms IS NOT NULL",
    )
    .fetch_one(db.pool())
    .await
    .context("Failed to query task latency metric")?
    .unwrap_or(0.0);

    let terminal_tasks = snapshot.tasks_completed + snapshot.tasks_failed;
    snapshot.task_error_rate = if terminal_tasks > 0 {
        snapshot.tasks_failed as f64 / terminal_tasks as f64
    } else {
        0.0
    };

    let now = chrono::Utc::now().timestamp();
    snapshot.active_sessions = sqlx::query_scalar(
        r#"SELECT COUNT(*)
           FROM auth_sessions
           WHERE revoked_at IS NULL
             AND expires_at > ?
             AND absolute_expires_at > ?"#,
    )
    .bind(now)
    .bind(now)
    .fetch_one(db.pool())
    .await
    .context("Failed to query active session metric")?;

    Ok(snapshot)
}

pub fn render_prometheus(snapshot: &MetricsSnapshot) -> String {
    [
        "# HELP rove_tasks_total Total persisted tasks.",
        "# TYPE rove_tasks_total counter",
        &format!("rove_tasks_total {}", snapshot.tasks_total),
        "# HELP rove_tasks_pending Current pending tasks.",
        "# TYPE rove_tasks_pending gauge",
        &format!("rove_tasks_pending {}", snapshot.tasks_pending),
        "# HELP rove_tasks_running Current running tasks.",
        "# TYPE rove_tasks_running gauge",
        &format!("rove_tasks_running {}", snapshot.tasks_running),
        "# HELP rove_tasks_completed_total Total completed tasks.",
        "# TYPE rove_tasks_completed_total counter",
        &format!("rove_tasks_completed_total {}", snapshot.tasks_completed),
        "# HELP rove_tasks_failed_total Total failed tasks.",
        "# TYPE rove_tasks_failed_total counter",
        &format!("rove_tasks_failed_total {}", snapshot.tasks_failed),
        "# HELP rove_tool_calls_total Total persisted tool calls.",
        "# TYPE rove_tool_calls_total counter",
        &format!("rove_tool_calls_total {}", snapshot.tool_calls_total),
        "# HELP rove_task_error_rate Failed terminal tasks divided by completed plus failed tasks.",
        "# TYPE rove_task_error_rate gauge",
        &format!("rove_task_error_rate {:.6}", snapshot.task_error_rate),
        "# HELP rove_task_latency_avg_ms Average completed task latency in milliseconds.",
        "# TYPE rove_task_latency_avg_ms gauge",
        &format!(
            "rove_task_latency_avg_ms {:.3}",
            snapshot.task_latency_avg_ms
        ),
        "# HELP rove_active_sessions Current non-revoked, non-expired daemon sessions.",
        "# TYPE rove_active_sessions gauge",
        &format!("rove_active_sessions {}", snapshot.active_sessions),
        "",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Database, TaskStatus};
    use tempfile::TempDir;

    #[tokio::test]
    async fn collects_and_renders_prometheus_metrics() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("metrics.db"))
            .await
            .unwrap();
        let repo = db.tasks();

        let running_id = uuid::Uuid::new_v4();
        repo.create_task(&running_id, "running").await.unwrap();
        repo.update_task_status(&running_id, TaskStatus::Running)
            .await
            .unwrap();

        let completed_id = uuid::Uuid::new_v4();
        repo.create_task(&completed_id, "completed").await.unwrap();
        repo.complete_task(&completed_id, "localbrain", 1200)
            .await
            .unwrap();
        repo.insert_agent_event(
            &completed_id,
            "tool_call",
            r#"{"tool_name":"read_file"}"#,
            1,
            None,
        )
        .await
        .unwrap();

        let failed_id = uuid::Uuid::new_v4();
        repo.create_task(&failed_id, "failed").await.unwrap();
        repo.fail_task(&failed_id).await.unwrap();

        db.auth()
            .create_session(
                "session-1",
                60,
                120,
                Some("webui"),
                Some("local"),
                Some("ua"),
            )
            .await
            .unwrap();

        let snapshot = collect_metrics(&db).await.unwrap();
        assert_eq!(snapshot.tasks_total, 3);
        assert_eq!(snapshot.tasks_running, 1);
        assert_eq!(snapshot.tasks_completed, 1);
        assert_eq!(snapshot.tasks_failed, 1);
        assert_eq!(snapshot.tool_calls_total, 1);
        assert_eq!(snapshot.active_sessions, 1);
        assert!((snapshot.task_error_rate - 0.5).abs() < f64::EPSILON);
        assert!((snapshot.task_latency_avg_ms - 1200.0).abs() < f64::EPSILON);

        let rendered = render_prometheus(&snapshot);
        assert!(rendered.contains("rove_tasks_total 3"));
        assert!(rendered.contains("rove_tool_calls_total 1"));
        assert!(rendered.contains("rove_active_sessions 1"));
    }
}
