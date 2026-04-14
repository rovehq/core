//! Memory Importance Decay
//!
//! Applies time-based decay to unused memories and prunes fully decayed entries.
//! Called at the end of each consolidation pass.

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tracing::info;

/// Decay importance for unused memories and prune fully decayed entries.
///
/// Applies time-based decay multipliers:
/// - 0.9× for memories not accessed in 30 days
/// - 0.7× for memories not accessed in 90 days
/// - Deletes memories with importance < 0.05 and access_count = 0
///
/// # Arguments
/// * `pool` - SQLite connection pool
/// * `enabled` - Whether decay is enabled (from config.importance_decay_enabled)
/// * `retention_days` - Maximum age for episodic memory rows before pruning
pub async fn decay_importance(pool: &SqlitePool, enabled: bool, retention_days: u32) -> Result<()> {
    if !enabled {
        return Ok(());
    }

    let now = crate::conductor::scorer::unix_now();
    let thirty_days_ago = now - 30 * 86_400;
    let ninety_days_ago = now - 90 * 86_400;
    let retention_cutoff = now - retention_days as i64 * 86_400;

    // Mild decay for memories not accessed in 30 days
    sqlx::query(
        r#"UPDATE episodic_memory 
           SET importance = importance * 0.9
           WHERE (last_accessed IS NULL OR last_accessed < ?)
             AND importance > 0.1"#,
    )
    .bind(thirty_days_ago)
    .execute(pool)
    .await
    .context("Failed to apply 30-day decay")?;

    // Strong decay for memories not accessed in 90 days
    sqlx::query(
        r#"UPDATE episodic_memory 
           SET importance = importance * 0.7
           WHERE (last_accessed IS NULL OR last_accessed < ?)
             AND importance > 0.1"#,
    )
    .bind(ninety_days_ago)
    .execute(pool)
    .await
    .context("Failed to apply 90-day decay")?;

    // Prune fully decayed, never accessed memories
    let deleted = sqlx::query(
        r#"DELETE FROM episodic_memory 
           WHERE importance < 0.05 AND access_count = 0"#,
    )
    .execute(pool)
    .await
    .context("Failed to prune decayed memories")?;

    if deleted.rows_affected() > 0 {
        info!("Pruned {} fully decayed memories", deleted.rows_affected());
    }

    if retention_days > 0 {
        let expired_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT id
               FROM episodic_memory
               WHERE created_at < ?"#,
        )
        .bind(retention_cutoff)
        .fetch_all(pool)
        .await
        .context("Failed to query expired memories")?;

        if !expired_ids.is_empty() {
            for memory_id in &expired_ids {
                let _ = sqlx::query(
                    r#"DELETE FROM memory_graph_edges
                       WHERE from_id = ? OR to_id = ?"#,
                )
                .bind(memory_id)
                .bind(memory_id)
                .execute(pool)
                .await;
            }

            let deleted_expired = sqlx::query(
                r#"DELETE FROM episodic_memory
                   WHERE created_at < ?"#,
            )
            .bind(retention_cutoff)
            .execute(pool)
            .await
            .context("Failed to prune expired episodic memories")?;

            if deleted_expired.rows_affected() > 0 {
                info!(
                    "Pruned {} episodic memories older than {} days",
                    deleted_expired.rows_affected(),
                    retention_days
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    #[tokio::test]
    async fn test_decay_importance_disabled() {
        use std::str::FromStr;

        let opts = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();

        // Should return Ok without doing anything when disabled
        let result = decay_importance(&pool, false, 30).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_decay_importance_enabled() {
        use std::str::FromStr;

        let opts = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();

        // Run migrations to create tables
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS episodic_memory (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                entities TEXT,
                topics TEXT,
                importance REAL NOT NULL,
                consolidated INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_accessed INTEGER,
                access_count INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        // Insert test memories
        let now = crate::conductor::scorer::unix_now();
        let thirty_days_ago = now - 31 * 86_400; // 31 days ago
        let ninety_days_ago = now - 91 * 86_400; // 91 days ago

        // Memory 1: Recent, should not decay
        sqlx::query(
            r#"INSERT INTO episodic_memory 
               (id, task_id, summary, importance, consolidated, created_at, last_accessed, access_count)
               VALUES (?, ?, ?, ?, 0, ?, ?, 1)"#,
        )
        .bind("mem1")
        .bind("task1")
        .bind("Recent memory")
        .bind(0.8)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        // Memory 2: 31 days old, should decay by 0.9
        sqlx::query(
            r#"INSERT INTO episodic_memory 
               (id, task_id, summary, importance, consolidated, created_at, last_accessed, access_count)
               VALUES (?, ?, ?, ?, 0, ?, ?, 0)"#,
        )
        .bind("mem2")
        .bind("task2")
        .bind("Old memory")
        .bind(0.8)
        .bind(now)
        .bind(thirty_days_ago)
        .execute(&pool)
        .await
        .unwrap();

        // Memory 3: 91 days old, should decay by 0.7
        sqlx::query(
            r#"INSERT INTO episodic_memory 
               (id, task_id, summary, importance, consolidated, created_at, last_accessed, access_count)
               VALUES (?, ?, ?, ?, 0, ?, ?, 0)"#,
        )
        .bind("mem3")
        .bind("task3")
        .bind("Very old memory")
        .bind(0.8)
        .bind(now)
        .bind(ninety_days_ago)
        .execute(&pool)
        .await
        .unwrap();

        // Memory 4: Low importance, never accessed, should be deleted
        sqlx::query(
            r#"INSERT INTO episodic_memory 
               (id, task_id, summary, importance, consolidated, created_at, last_accessed, access_count)
               VALUES (?, ?, ?, ?, 0, ?, NULL, 0)"#,
        )
        .bind("mem4")
        .bind("task4")
        .bind("Low importance memory")
        .bind(0.04)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        // Run decay
        decay_importance(&pool, true, 30).await.unwrap();

        // Verify results
        // Memory 1 should be unchanged
        let row = sqlx::query("SELECT importance FROM episodic_memory WHERE id = ?")
            .bind("mem1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let importance: f32 = row.get("importance");
        assert!((importance - 0.8).abs() < 0.01);

        // Memory 2 should be decayed by 0.9
        let row = sqlx::query("SELECT importance FROM episodic_memory WHERE id = ?")
            .bind("mem2")
            .fetch_one(&pool)
            .await
            .unwrap();
        let importance: f32 = row.get("importance");
        assert!((importance - 0.72).abs() < 0.01); // 0.8 * 0.9 = 0.72

        // Memory 3 should be decayed by both 0.9 and 0.7
        let row = sqlx::query("SELECT importance FROM episodic_memory WHERE id = ?")
            .bind("mem3")
            .fetch_one(&pool)
            .await
            .unwrap();
        let importance: f32 = row.get("importance");
        // 0.8 * 0.9 * 0.7 = 0.504
        assert!((importance - 0.504).abs() < 0.01);

        // Memory 4 should be deleted
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM episodic_memory WHERE id = ?")
            .bind("mem4")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_decay_importance_enforces_retention_window() {
        use std::str::FromStr;

        let opts = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS episodic_memory (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                entities TEXT,
                topics TEXT,
                importance REAL NOT NULL,
                consolidated INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_accessed INTEGER,
                access_count INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS memory_graph_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                edge_type TEXT NOT NULL,
                entity TEXT,
                weight REAL NOT NULL DEFAULT 1.0,
                confidence REAL NOT NULL DEFAULT 1.0,
                created_at INTEGER NOT NULL
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let now = crate::conductor::scorer::unix_now();
        let expired = now - 40 * 86_400;

        sqlx::query(
            r#"INSERT INTO episodic_memory
               (id, task_id, summary, importance, consolidated, created_at, last_accessed, access_count)
               VALUES ('old', 'task-old', 'expired memory', 0.9, 0, ?, ?, 0)"#,
        )
        .bind(expired)
        .bind(expired)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"INSERT INTO memory_graph_edges
               (id, from_id, to_id, edge_type, entity, weight, confidence, created_at)
               VALUES ('edge-old', 'old', 'other', 'shares_entity', NULL, 1.0, 1.0, ?)"#,
        )
        .bind(expired)
        .execute(&pool)
        .await
        .unwrap();

        decay_importance(&pool, true, 30).await.unwrap();

        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM episodic_memory WHERE id = 'old'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);

        let remaining_edges: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memory_graph_edges WHERE id = 'edge-old'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining_edges, 0);
    }
}
