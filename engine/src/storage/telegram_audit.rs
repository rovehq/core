use anyhow::Result;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TelegramAuditEntry {
    pub id: i64,
    pub event_type: String,
    pub telegram_user: i64,
    pub chat_id: Option<i64>,
    pub task_id: Option<String>,
    pub approval_key: Option<String>,
    pub approved: Option<bool>,
    pub operation: Option<String>,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct TelegramAuditRepository {
    pool: SqlitePool,
}

impl TelegramAuditRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log(
        &self,
        event_type: &str,
        telegram_user: i64,
        chat_id: Option<i64>,
        task_id: Option<&str>,
        approval_key: Option<&str>,
        approved: Option<bool>,
        operation: Option<&str>,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        sqlx::query(
            "INSERT INTO telegram_audit_log \
             (event_type, telegram_user, chat_id, task_id, approval_key, approved, operation, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(event_type)
        .bind(telegram_user)
        .bind(chat_id)
        .bind(task_id)
        .bind(approval_key)
        .bind(approved.map(|b| if b { 1 } else { 0 }))
        .bind(operation)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn recent(&self, limit: usize) -> Result<Vec<TelegramAuditEntry>> {
        let rows = sqlx::query_as::<_, TelegramAuditEntry>(
            "SELECT * FROM telegram_audit_log ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    pub async fn count_by_user(&self, telegram_user: i64) -> Result<i64> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM telegram_audit_log WHERE telegram_user = ?")
                .bind(telegram_user)
                .fetch_one(&self.pool)
                .await?;

        Ok(count)
    }
}
