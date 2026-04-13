use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;
use sqlx::Row;
use std::path::Path;
use std::str::FromStr;
use tracing::{debug, info};

use super::{
    AgentRunRepository, AuthRepository, ExtensionCatalogRepository, InstalledPluginRepository,
    PendingTaskRepository, PluginRepository, RemoteDiscoveryRepository, ScheduleRepository,
    TaskRepository, TelegramAuditRepository,
};

/// Database connection pool.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection backed by SQLite WAL mode.
    pub async fn new(db_path: &Path) -> Result<Self> {
        info!("Initializing database at: {}", db_path.display());

        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create database directory")?;
        }

        let connection_string = format!("sqlite:{}", db_path.display());
        let options = SqliteConnectOptions::from_str(&connection_string)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true)
            .disable_statement_logging();

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to database")?;

        debug!("Database connection established");

        let db = Self { pool };
        db.run_schema().await?;
        Ok(db)
    }

    async fn run_schema(&self) -> Result<()> {
        info!("Running database schema");

        self.ensure_agent_events_parent_task_id_compat()
            .await
            .context("Failed to apply pre-schema agent_events compatibility patch")?;

        sqlx::raw_sql(include_str!("../../schemas/base.sql"))
            .execute(&self.pool)
            .await
            .context("Failed to execute schemas/base.sql")?;

        self.ensure_agent_events_parent_task_id()
            .await
            .context("Failed to apply agent_events parent_task_id schema patch")?;
        self.ensure_installed_plugin_provenance_columns()
            .await
            .context("Failed to apply installed plugin provenance schema patch")?;
        self.ensure_workflow_run_columns()
            .await
            .context("Failed to apply workflow run schema patch")?;

        info!("Database schema loaded successfully");
        Ok(())
    }

    async fn ensure_agent_events_parent_task_id_compat(&self) -> Result<()> {
        if !self.table_exists("agent_events").await? {
            return Ok(());
        }

        let columns = self.table_columns("agent_events").await?;
        if columns.iter().any(|column| column == "parent_task_id") {
            return Ok(());
        }

        sqlx::query("ALTER TABLE agent_events ADD COLUMN parent_task_id TEXT")
            .execute(&self.pool)
            .await
            .context("Failed to add agent_events.parent_task_id during compatibility patch")?;

        Ok(())
    }

    async fn ensure_agent_events_parent_task_id(&self) -> Result<()> {
        let columns = self.table_columns("agent_events").await?;

        if !columns.iter().any(|column| column == "parent_task_id") {
            sqlx::query("ALTER TABLE agent_events ADD COLUMN parent_task_id TEXT")
                .execute(&self.pool)
                .await
                .context("Failed to add agent_events.parent_task_id")?;
        }

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_agent_events_parent ON agent_events(parent_task_id, step_num)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create agent_events parent index")?;

        Ok(())
    }

    async fn ensure_installed_plugin_provenance_columns(&self) -> Result<()> {
        if !self.table_exists("installed_plugins").await? {
            return Ok(());
        }

        let columns = self.table_columns("installed_plugins").await?;
        for (column, sql_type) in [
            ("provenance_source", "TEXT"),
            ("provenance_registry", "TEXT"),
            ("catalog_trust_badge", "TEXT"),
        ] {
            if !columns.iter().any(|existing| existing == column) {
                sqlx::query(&format!(
                    "ALTER TABLE installed_plugins ADD COLUMN {column} {sql_type}"
                ))
                .execute(&self.pool)
                .await
                .with_context(|| {
                    format!("Failed to add installed_plugins.{column} compatibility column")
                })?;
            }
        }

        Ok(())
    }

    async fn ensure_workflow_run_columns(&self) -> Result<()> {
        if !self.table_exists("workflow_runs").await? {
            return Ok(());
        }

        let columns = self.table_columns("workflow_runs").await?;
        for (column, sql_type, default_clause) in [
            ("steps_total", "INTEGER", " NOT NULL DEFAULT 0"),
            ("steps_completed", "INTEGER", " NOT NULL DEFAULT 0"),
            ("current_step_index", "INTEGER", ""),
            ("current_step_id", "TEXT", ""),
            ("current_step_name", "TEXT", ""),
            ("retry_count", "INTEGER", " NOT NULL DEFAULT 0"),
            ("last_task_id", "TEXT", ""),
        ] {
            if !columns.iter().any(|existing| existing == column) {
                sqlx::query(&format!(
                    "ALTER TABLE workflow_runs ADD COLUMN {column} {sql_type}{default_clause}"
                ))
                .execute(&self.pool)
                .await
                .with_context(|| {
                    format!("Failed to add workflow_runs.{column} compatibility column")
                })?;
            }
        }

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_workflow_run_steps_run ON workflow_run_steps(run_id, step_index)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create workflow_run_steps run index")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_workflow_run_steps_status ON workflow_run_steps(status, started_at DESC)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create workflow_run_steps status index")?;

        Ok(())
    }

    async fn table_exists(&self, table: &str) -> Result<bool> {
        let row =
            sqlx::query("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1")
                .bind(table)
                .fetch_optional(&self.pool)
                .await
                .with_context(|| format!("Failed to inspect sqlite_master for table '{table}'"))?;
        Ok(row.is_some())
    }

    async fn table_columns(&self, table: &str) -> Result<Vec<String>> {
        let pragma = format!("PRAGMA table_info({table})");
        sqlx::query(&pragma)
            .fetch_all(&self.pool)
            .await
            .with_context(|| format!("Failed to inspect columns for table '{table}'"))
            .map(|rows| rows.into_iter().map(|row| row.get("name")).collect())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn flush_wal(&self) -> Result<()> {
        info!("Flushing WAL to disk");

        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
            .context("Failed to flush WAL")?;

        debug!("WAL flushed successfully");
        Ok(())
    }

    pub async fn close(self) -> Result<()> {
        info!("Closing database connection");
        self.flush_wal().await?;
        self.pool.close().await;
        info!("Database connection closed");
        Ok(())
    }

    pub fn tasks(&self) -> TaskRepository {
        TaskRepository::new(self.pool.clone())
    }

    pub fn agent_runs(&self) -> AgentRunRepository {
        AgentRunRepository::new(self.pool.clone())
    }

    pub fn auth(&self) -> AuthRepository {
        AuthRepository::new(self.pool.clone())
    }

    pub fn telegram_audit(&self) -> TelegramAuditRepository {
        TelegramAuditRepository::new(self.pool.clone())
    }

    pub fn plugins(&self) -> PluginRepository {
        PluginRepository::new(self.pool.clone())
    }

    pub fn installed_plugins(&self) -> InstalledPluginRepository {
        InstalledPluginRepository::new(self.pool.clone())
    }

    pub fn extension_catalog(&self) -> ExtensionCatalogRepository {
        ExtensionCatalogRepository::new(self.pool.clone())
    }

    pub fn pending_tasks(&self) -> PendingTaskRepository {
        PendingTaskRepository::new(self.pool.clone())
    }

    pub fn schedules(&self) -> ScheduleRepository {
        ScheduleRepository::new(self.pool.clone())
    }

    pub fn remote_discovery(&self) -> RemoteDiscoveryRepository {
        RemoteDiscoveryRepository::new(self.pool.clone())
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

        assert!(db_path.exists());
        assert!(sqlx::query("SELECT 1").fetch_one(db.pool()).await.is_ok());

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_schema_create_tables() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .fetch_all(db.pool())
                .await
                .unwrap();

        assert!(tables.contains(&"tasks".to_string()));
        assert!(tables.contains(&"task_steps".to_string()));
        assert!(tables.contains(&"plugins".to_string()));
        assert!(tables.contains(&"installed_plugins".to_string()));
        assert!(tables.contains(&"pending_tasks".to_string()));
        assert!(tables.contains(&"scheduled_tasks".to_string()));
        assert!(tables.contains(&"agent_events".to_string()));
        assert!(tables.contains(&"agent_runs".to_string()));
        assert!(tables.contains(&"workflow_runs".to_string()));
        assert!(tables.contains(&"workflow_run_steps".to_string()));

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_wal_mode_enabled() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

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

        let foreign_keys: i32 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(db.pool())
            .await
            .unwrap();

        assert_eq!(foreign_keys, 1);

        db.close().await.unwrap();
    }
}
