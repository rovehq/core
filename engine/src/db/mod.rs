/// Database module for SQLite persistence
///
/// This module provides database operations for task history, plugin metadata,
/// secrets cache, and rate limiting. It uses sqlx with compile-time verified queries
/// and WAL mode for better concurrency.
///
/// Requirements: 12.1, 12.2, 12.3, 12.7, 12.8, 12.9, 12.10
use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;
use std::path::Path;
use std::str::FromStr;
use tracing::{debug, info};

pub mod memory;
pub mod pending_tasks;
pub mod plugins;
pub mod tasks;

// Re-export commonly used types
pub use memory::{EpisodicMemory, MemoryEntry};
pub use pending_tasks::{PendingTask, PendingTaskRepository, PendingTaskStatus};
pub use plugins::{Plugin, PluginRepository};
pub use tasks::{StepType, Task, TaskRepository, TaskStatus, TaskStep};

/// Database connection pool
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection
    ///
    /// This will:
    /// 1. Create the database file if it doesn't exist
    /// 2. Enable WAL mode for better concurrency
    /// 3. Run migrations to set up the schema
    /// 4. Automatically recover from WAL if needed (after unclean shutdown)
    ///
    /// **WAL Recovery (Requirement 12.9):**
    /// SQLite with WAL mode automatically recovers from unclean shutdowns when the
    /// database is reopened. If a WAL file exists with uncommitted transactions,
    /// SQLite will automatically apply them during connection. This is built into
    /// SQLite's WAL implementation and requires no additional code.
    ///
    /// **WAL Flushing (Requirement 12.8):**
    /// During graceful shutdown, call `flush_wal()` or `close()` to checkpoint
    /// the WAL and ensure all data is written to the main database file.
    ///
    /// Requirements: 12.3, 12.8, 12.9
    pub async fn new(db_path: &Path) -> Result<Self> {
        info!("Initializing database at: {}", db_path.display());

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create database directory")?;
        }

        // Configure SQLite connection with WAL mode
        let connection_string = format!("sqlite:{}", db_path.display());
        let options = SqliteConnectOptions::from_str(&connection_string)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true)
            .disable_statement_logging();

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to database")?;

        debug!("Database connection established");

        let db = Self { pool };

        // Run migrations
        db.run_migrations().await?;

        Ok(db)
    }

    /// Run database migrations
    ///
    /// This executes the consolidated schema from schema/base.sql.
    /// The schema is idempotent and can be run multiple times safely.
    ///
    /// Requirements: 12.7
    async fn run_migrations(&self) -> Result<()> {
        info!("Running database migrations");

        // Execute consolidated schema
        sqlx::raw_sql(include_str!("../../schema/base.sql"))
            .execute(&self.pool)
            .await
            .context("Failed to execute schema/base.sql")?;

        info!("Database migrations completed successfully");
        Ok(())
    }

    /// Get a reference to the connection pool
    ///
    /// This allows other modules to execute queries against the database.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Flush the WAL to disk
    ///
    /// This should be called during graceful shutdown to ensure all
    /// pending writes are persisted to the database file.
    ///
    /// Requirements: 12.8
    pub async fn flush_wal(&self) -> Result<()> {
        info!("Flushing WAL to disk");

        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
            .context("Failed to flush WAL")?;

        debug!("WAL flushed successfully");
        Ok(())
    }

    /// Close the database connection
    ///
    /// This flushes the WAL and closes all connections in the pool.
    /// Should be called during shutdown.
    ///
    /// Requirements: 12.8
    pub async fn close(self) -> Result<()> {
        info!("Closing database connection");

        // Flush WAL before closing
        self.flush_wal().await?;

        // Close the pool
        self.pool.close().await;

        info!("Database connection closed");
        Ok(())
    }

    /// Create a task repository
    ///
    /// Requirements: 12.2, 12.4, 12.5
    pub fn tasks(&self) -> TaskRepository {
        TaskRepository::new(self.pool.clone())
    }

    /// Create a plugin repository
    ///
    /// Requirements: 12.2, 12.6
    pub fn plugins(&self) -> PluginRepository {
        PluginRepository::new(self.pool.clone())
    }

    /// Create a pending task repository for durable inbox
    ///
    /// Phase 3 — Gateway + Durable Inbox
    pub fn pending_tasks(&self) -> PendingTaskRepository {
        PendingTaskRepository::new(self.pool.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_database_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

        // Verify database file was created
        assert!(db_path.exists());

        // Verify we can query the database
        let result = sqlx::query("SELECT 1").fetch_one(db.pool()).await;

        assert!(result.is_ok());

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_migrations_create_tables() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

        // Verify all tables were created
        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .fetch_all(db.pool())
                .await
                .unwrap();

        assert!(tables.contains(&"tasks".to_string()));
        assert!(tables.contains(&"task_steps".to_string()));
        assert!(tables.contains(&"plugins".to_string()));
        assert!(tables.contains(&"secrets_cache".to_string()));
        assert!(tables.contains(&"rate_limits".to_string()));

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_wal_mode_enabled() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

        // Verify WAL mode is enabled
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(db.pool())
            .await
            .unwrap();

        assert_eq!(journal_mode.to_lowercase(), "wal");

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_foreign_keys_enabled() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

        // Verify foreign keys are enabled
        let foreign_keys: i32 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(db.pool())
            .await
            .unwrap();

        assert_eq!(foreign_keys, 1);

        db.close().await.unwrap();
    }
}
