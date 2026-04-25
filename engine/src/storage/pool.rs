use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;
use sqlx::Row;
use std::path::Path;
use std::str::FromStr;
use tracing::{debug, info};

use super::{
    AgentRunRepository, AuthRepository, ExtensionCatalogRepository, InstalledPluginRepository,
    KnowledgeRepository, ManagedAgentRepository, MemoryAuditRepository, PendingTaskRepository,
    PluginRepository, RemoteDiscoveryRepository, ScheduleRepository, TaskRepository,
    TelegramAuditRepository, ThreadRepository,
};

/// Database connection pool.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection backed by SQLite WAL mode.
    pub async fn new(db_path: &Path) -> Result<Self> {
        debug!("Initializing database at: {}", db_path.display());

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
        debug!("Running database schema");

        // Pre-schema compatibility patches: run BEFORE base.sql so that any
        // index or constraint in base.sql that references a newly-added column
        // finds that column already present on existing databases.
        self.ensure_agent_events_parent_task_id_compat()
            .await
            .context("Failed to apply pre-schema agent_events compatibility patch")?;
        self.ensure_task_history_columns()
            .await
            .context("Failed to apply pre-schema task history columns patch")?;
        self.ensure_pending_task_profile_column()
            .await
            .context("Failed to apply pre-schema pending task profile patch")?;

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
        self.ensure_workflow_run_status_supports_canceled()
            .await
            .context("Failed to apply workflow run cancel-status schema patch")?;
        self.ensure_graph_provenance_columns()
            .await
            .context("Failed to apply graph provenance schema patch")?;
        self.ensure_episodic_memory_kind_column()
            .await
            .context("Failed to apply episodic memory_kind schema patch")?;
        self.ensure_memory_graph_edges_table()
            .await
            .context("Failed to apply memory_graph_edges schema patch")?;
        self.ensure_scheduled_task_columns()
            .await
            .context("Failed to apply scheduled task schema patch")?;
        // Also run post-schema in case new installs need the index created.
        self.ensure_task_history_columns()
            .await
            .context("Failed to apply task history schema patch")?;
        self.ensure_memory_audit_tables()
            .await
            .context("Failed to apply memory audit schema patch")?;
        self.ensure_agent_threads_table()
            .await
            .context("Failed to apply agent_threads schema patch")?;
        self.ensure_managed_agent_sessions_table()
            .await
            .context("Failed to apply managed agent sessions schema patch")?;
        self.ensure_knowledge_provenance_columns()
            .await
            .context("Failed to apply knowledge provenance schema patch")?;
        self.ensure_knowledge_jobs_table()
            .await
            .context("Failed to apply knowledge_jobs schema patch")?;

        debug!("Database schema loaded successfully");
        Ok(())
    }

    async fn ensure_knowledge_provenance_columns(&self) -> Result<()> {
        if !self.table_exists("knowledge_documents").await? {
            return Ok(());
        }
        let columns = self.table_columns("knowledge_documents").await?;
        for (col, sql_type) in [("ingested_by", "TEXT"), ("ingest_job_id", "TEXT")] {
            if !columns.iter().any(|c| c == col) {
                sqlx::query(&format!(
                    "ALTER TABLE knowledge_documents ADD COLUMN {col} {sql_type}"
                ))
                .execute(&self.pool)
                .await
                .with_context(|| {
                    format!("Failed to add knowledge_documents.{col} column")
                })?;
            }
        }
        Ok(())
    }

    async fn ensure_knowledge_jobs_table(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS knowledge_jobs (
                id          TEXT PRIMARY KEY,
                kind        TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'running',
                source      TEXT NOT NULL,
                total       INTEGER NOT NULL DEFAULT 0,
                processed   INTEGER NOT NULL DEFAULT 0,
                errors_json TEXT NOT NULL DEFAULT '[]',
                started_at  INTEGER NOT NULL,
                finished_at INTEGER
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create knowledge_jobs table")?;
        Ok(())
    }

    async fn ensure_task_history_columns(&self) -> Result<()> {
        if !self.table_exists("tasks").await? {
            return Ok(());
        }

        let columns = self.table_columns("tasks").await?;
        for (column, sql_type, default_clause) in [
            ("source", "TEXT", " NOT NULL DEFAULT 'cli'"),
            ("agent_id", "TEXT", ""),
            ("agent_name", "TEXT", ""),
            ("thread_id", "TEXT", ""),
            ("worker_preset_id", "TEXT", ""),
            ("worker_preset_name", "TEXT", ""),
        ] {
            if !columns.iter().any(|existing| existing == column) {
                sqlx::query(&format!(
                    "ALTER TABLE tasks ADD COLUMN {column} {sql_type}{default_clause}"
                ))
                .execute(&self.pool)
                .await
                .with_context(|| format!("Failed to add tasks.{column} compatibility column"))?;
            }
        }

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_agent_created_at ON tasks(agent_id, created_at DESC)")
            .execute(&self.pool)
            .await
            .context("Failed to create task agent history index")?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_thread_created_at ON tasks(thread_id, created_at DESC)")
            .execute(&self.pool)
            .await
            .context("Failed to create task thread history index")?;

        Ok(())
    }

    async fn ensure_pending_task_profile_column(&self) -> Result<()> {
        if !self.table_exists("pending_tasks").await? {
            return Ok(());
        }

        let columns = self.table_columns("pending_tasks").await?;
        if !columns
            .iter()
            .any(|existing| existing == "execution_profile_json")
        {
            sqlx::query("ALTER TABLE pending_tasks ADD COLUMN execution_profile_json TEXT")
                .execute(&self.pool)
                .await
                .context(
                    "Failed to add pending_tasks.execution_profile_json compatibility column",
                )?;
        }

        Ok(())
    }

    async fn ensure_memory_audit_tables(&self) -> Result<()> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS memory_versions (
                id TEXT PRIMARY KEY,
                entity_kind TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                version_num INTEGER NOT NULL,
                action TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                snapshot_json TEXT NOT NULL,
                actor TEXT NOT NULL,
                source_task_id TEXT,
                created_at INTEGER NOT NULL
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_versions_entity_version
               ON memory_versions(entity_kind, entity_id, version_num)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_memory_versions_entity_created
               ON memory_versions(entity_kind, entity_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS memory_audit_log (
                id TEXT PRIMARY KEY,
                entity_kind TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                action TEXT NOT NULL,
                actor TEXT NOT NULL,
                source_task_id TEXT,
                precondition_hash TEXT,
                content_hash TEXT,
                metadata_json TEXT,
                created_at INTEGER NOT NULL
            )"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_memory_audit_entity_created
               ON memory_audit_log(entity_kind, entity_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

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
            ("cancel_requested", "INTEGER", " NOT NULL DEFAULT 0"),
            ("cancel_requested_at", "INTEGER", ""),
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

    async fn ensure_workflow_run_status_supports_canceled(&self) -> Result<()> {
        if !self.table_exists("workflow_runs").await? {
            return Ok(());
        }

        let create_sql = self
            .table_sql("workflow_runs")
            .await?
            .unwrap_or_default()
            .to_ascii_lowercase();
        if create_sql.contains("'canceled'") {
            return Ok(());
        }

        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&self.pool)
            .await
            .context("Failed to disable foreign keys for workflow_runs rebuild")?;

        sqlx::query("ALTER TABLE workflow_runs RENAME TO workflow_runs_legacy")
            .execute(&self.pool)
            .await
            .context("Failed to rename legacy workflow_runs table")?;

        sqlx::query(
            r#"CREATE TABLE workflow_runs (
                run_id         TEXT PRIMARY KEY,
                workflow_id    TEXT NOT NULL,
                status         TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
                input          TEXT NOT NULL,
                output         TEXT,
                error          TEXT,
                steps_total    INTEGER NOT NULL DEFAULT 0,
                steps_completed INTEGER NOT NULL DEFAULT 0,
                current_step_index INTEGER,
                current_step_id TEXT,
                current_step_name TEXT,
                retry_count    INTEGER NOT NULL DEFAULT 0,
                last_task_id   TEXT,
                cancel_requested INTEGER NOT NULL DEFAULT 0,
                cancel_requested_at INTEGER,
                created_at     INTEGER NOT NULL,
                completed_at   INTEGER
            )"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create upgraded workflow_runs table")?;

        sqlx::query(
            r#"INSERT INTO workflow_runs (
                   run_id, workflow_id, status, input, output, error,
                   steps_total, steps_completed, current_step_index, current_step_id, current_step_name,
                   retry_count, last_task_id, cancel_requested, cancel_requested_at, created_at, completed_at
               )
               SELECT run_id, workflow_id, status, input, output, error,
                      steps_total, steps_completed, current_step_index, current_step_id, current_step_name,
                      retry_count, last_task_id, COALESCE(cancel_requested, 0), cancel_requested_at, created_at, completed_at
               FROM workflow_runs_legacy"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to copy legacy workflow_runs rows")?;

        sqlx::query("DROP TABLE workflow_runs_legacy")
            .execute(&self.pool)
            .await
            .context("Failed to drop legacy workflow_runs table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow ON workflow_runs(workflow_id, created_at DESC)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to recreate workflow_runs index")?;

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&self.pool)
            .await
            .context("Failed to re-enable foreign keys after workflow_runs rebuild")?;

        Ok(())
    }

    async fn ensure_graph_provenance_columns(&self) -> Result<()> {
        if self.table_exists("graph_nodes").await? {
            let columns = self.table_columns("graph_nodes").await?;
            for (column, sql_type, default_clause) in [
                ("source_kind", "TEXT", " NOT NULL DEFAULT 'deterministic'"),
                ("source_scope", "TEXT", " NOT NULL DEFAULT 'per_node'"),
                ("source_ref", "TEXT", ""),
                ("confidence", "REAL", " NOT NULL DEFAULT 1.0"),
            ] {
                if !columns.iter().any(|existing| existing == column) {
                    sqlx::query(&format!(
                        "ALTER TABLE graph_nodes ADD COLUMN {column} {sql_type}{default_clause}"
                    ))
                    .execute(&self.pool)
                    .await
                    .with_context(|| format!("Failed to add graph_nodes.{column}"))?;
                }
            }
        }

        if self.table_exists("graph_edges").await? {
            let columns = self.table_columns("graph_edges").await?;
            for (column, sql_type, default_clause) in [
                ("source_kind", "TEXT", " NOT NULL DEFAULT 'deterministic'"),
                ("source_scope", "TEXT", " NOT NULL DEFAULT 'per_node'"),
                ("source_ref", "TEXT", ""),
                ("confidence", "REAL", " NOT NULL DEFAULT 1.0"),
                ("updated_at", "INTEGER", " NOT NULL DEFAULT 0"),
            ] {
                if !columns.iter().any(|existing| existing == column) {
                    sqlx::query(&format!(
                        "ALTER TABLE graph_edges ADD COLUMN {column} {sql_type}{default_clause}"
                    ))
                    .execute(&self.pool)
                    .await
                    .with_context(|| format!("Failed to add graph_edges.{column}"))?;
                }
            }

            sqlx::query(
                "UPDATE graph_edges SET updated_at = created_at WHERE updated_at = 0 OR updated_at IS NULL",
            )
            .execute(&self.pool)
            .await
            .context("Failed to backfill graph_edges.updated_at")?;
        }

        Ok(())
    }

    async fn ensure_scheduled_task_columns(&self) -> Result<()> {
        if !self.table_exists("scheduled_tasks").await? {
            return Ok(());
        }

        let columns = self.table_columns("scheduled_tasks").await?;
        for (column, sql_type, default_clause) in [
            ("target_kind", "TEXT", " NOT NULL DEFAULT 'task'"),
            ("target_id", "TEXT", ""),
        ] {
            if !columns.iter().any(|existing| existing == column) {
                sqlx::query(&format!(
                    "ALTER TABLE scheduled_tasks ADD COLUMN {column} {sql_type}{default_clause}"
                ))
                .execute(&self.pool)
                .await
                .with_context(|| {
                    format!("Failed to add scheduled_tasks.{column} compatibility column")
                })?;
            }
        }

        Ok(())
    }

    async fn ensure_memory_graph_edges_table(&self) -> Result<()> {
        // CREATE TABLE IF NOT EXISTS in base.sql handles new installs.
        // For existing DBs that predate Section 13, the table simply won't exist
        // yet — base.sql will create it on the next run. Nothing else to migrate.
        // Add the indexes explicitly in case the base.sql run skipped partial execution.
        for idx in [
            "CREATE INDEX IF NOT EXISTS idx_mem_graph_from ON memory_graph_edges(from_id)",
            "CREATE INDEX IF NOT EXISTS idx_mem_graph_to ON memory_graph_edges(to_id)",
            "CREATE INDEX IF NOT EXISTS idx_mem_graph_type_from ON memory_graph_edges(edge_type, from_id)",
        ] {
            if self.table_exists("memory_graph_edges").await? {
                let _ = sqlx::query(idx).execute(&self.pool).await;
            }
        }
        Ok(())
    }

    async fn ensure_episodic_memory_kind_column(&self) -> Result<()> {
        if !self.table_exists("episodic_memory").await? {
            return Ok(());
        }
        let columns = self.table_columns("episodic_memory").await?;
        if !columns.iter().any(|c| c == "memory_kind") {
            sqlx::query(
                "ALTER TABLE episodic_memory ADD COLUMN memory_kind TEXT NOT NULL DEFAULT 'general'",
            )
            .execute(&self.pool)
            .await
            .context("Failed to add episodic_memory.memory_kind")?;
        }
        Ok(())
    }

    async fn ensure_agent_threads_table(&self) -> Result<()> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS agent_threads (
                id                  TEXT PRIMARY KEY,
                parent_agent_id     TEXT NOT NULL,
                callable_agent_id   TEXT NOT NULL,
                callable_agent_name TEXT NOT NULL,
                status              TEXT NOT NULL CHECK(status IN ('active', 'idle', 'completed')),
                task_count          INTEGER NOT NULL DEFAULT 0,
                created_at          INTEGER NOT NULL,
                last_active_at      INTEGER NOT NULL
            )"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create agent_threads table")?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_agent_threads_parent
               ON agent_threads(parent_agent_id, last_active_at DESC)"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create agent_threads parent index")?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_agent_threads_callable
               ON agent_threads(parent_agent_id, callable_agent_id)"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create agent_threads callable index")?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS agent_thread_events (
                id          TEXT PRIMARY KEY,
                thread_id   TEXT NOT NULL,
                event_type  TEXT NOT NULL,
                task_id     TEXT,
                payload     TEXT NOT NULL,
                created_at  INTEGER NOT NULL,
                FOREIGN KEY (thread_id) REFERENCES agent_threads(id) ON DELETE CASCADE
            )"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create agent_thread_events table")?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_thread_events_thread
               ON agent_thread_events(thread_id, created_at ASC)"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create agent_thread_events index")?;

        Ok(())
    }

    async fn ensure_managed_agent_sessions_table(&self) -> Result<()> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS managed_agent_sessions (
                id                TEXT PRIMARY KEY,
                agent_id          TEXT NOT NULL,
                environment_id    TEXT NOT NULL,
                profile_name      TEXT NOT NULL,
                loadout_name      TEXT NOT NULL,
                primary_thread_id TEXT NOT NULL,
                status            TEXT NOT NULL CHECK(status IN ('ready', 'running', 'idle', 'failed')),
                last_task_id      TEXT,
                created_at        INTEGER NOT NULL,
                last_active_at    INTEGER NOT NULL
            )"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create managed_agent_sessions table")?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_managed_agent_sessions_agent
               ON managed_agent_sessions(agent_id, last_active_at DESC)"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create managed_agent_sessions agent index")?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS managed_agent_session_events (
                position     INTEGER PRIMARY KEY AUTOINCREMENT,
                id           TEXT NOT NULL UNIQUE,
                session_id   TEXT NOT NULL,
                event_type   TEXT NOT NULL,
                task_id      TEXT,
                thread_id    TEXT,
                payload_json TEXT NOT NULL,
                created_at   INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES managed_agent_sessions(id) ON DELETE CASCADE
            )"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create managed_agent_session_events table")?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_managed_agent_session_events_session
               ON managed_agent_session_events(session_id, position ASC)"#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create managed_agent_session_events session index")?;

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

    async fn table_sql(&self, table: &str) -> Result<Option<String>> {
        sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        )
        .bind(table)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to inspect create SQL for table '{table}'"))
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

    pub fn threads(&self) -> ThreadRepository {
        ThreadRepository::new(self.pool.clone())
    }

    pub fn managed_agents(&self) -> ManagedAgentRepository {
        ManagedAgentRepository::new(self.pool.clone())
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

    pub fn knowledge(&self) -> KnowledgeRepository {
        KnowledgeRepository::new(self.pool.clone())
    }

    pub fn memory_audit(&self) -> MemoryAuditRepository {
        MemoryAuditRepository::new(self.pool.clone())
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
        assert!(tables.contains(&"agent_threads".to_string()));
        assert!(tables.contains(&"agent_thread_events".to_string()));
        assert!(tables.contains(&"managed_agent_sessions".to_string()));
        assert!(tables.contains(&"managed_agent_session_events".to_string()));

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
