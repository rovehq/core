//! Episodic Memory Repository
//!
//! Provides full-text search capabilities over previously executed tasks and their steps
//! to allow the context assembler to pull in relevant past lessons.

use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};

/// A memory entry retrieved from past task executions
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub task_id: String,
    pub step_type: String,
    pub content: String,
    pub rank: f64,
}

/// Repository for searching episodic memory
pub struct EpisodicMemory {
    pool: SqlitePool,
}

impl EpisodicMemory {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Search for past task steps matching the query text.
    /// Orders results by FTS bm25 rank, limiting to `limit`.
    pub async fn search(&self, query_text: &str, limit: i64) -> Result<Vec<MemoryEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT task_id, step_type, content, rank
            FROM task_steps_fts
            WHERE content MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
        )
        .bind(query_text)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to execute FTS query on task_steps_fts")?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(MemoryEntry {
                task_id: row.get("task_id"),
                step_type: row.get("step_type"),
                content: row.get("content"),
                rank: row.get("rank"),
            });
        }

        Ok(entries)
    }
}
