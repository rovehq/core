use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedAgentSessionStatus {
    Ready,
    Running,
    Idle,
    Failed,
}

impl ManagedAgentSessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Failed => "failed",
        }
    }
}

impl std::str::FromStr for ManagedAgentSessionStatus {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "idle" => Ok(Self::Idle),
            "failed" => Ok(Self::Failed),
            other => anyhow::bail!("unknown managed agent session status: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedAgentSession {
    pub id: String,
    pub agent_id: String,
    pub environment_id: String,
    pub profile_name: String,
    pub loadout_name: String,
    pub primary_thread_id: String,
    pub status: ManagedAgentSessionStatus,
    pub last_task_id: Option<String>,
    pub created_at: i64,
    pub last_active_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedAgentSessionEvent {
    pub position: i64,
    pub id: String,
    pub session_id: String,
    pub event_type: String,
    pub task_id: Option<String>,
    pub thread_id: Option<String>,
    pub payload: Value,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct ManagedAgentRepository {
    pool: SqlitePool,
}

impl ManagedAgentRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_session(
        &self,
        agent_id: &str,
        environment_id: &str,
        profile_name: &str,
        loadout_name: &str,
    ) -> Result<ManagedAgentSession> {
        let now = chrono::Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();
        let primary_thread_id = format!("compat:{id}:primary");
        sqlx::query(
            r#"INSERT INTO managed_agent_sessions
               (id, agent_id, environment_id, profile_name, loadout_name, primary_thread_id,
                status, created_at, last_active_at)
               VALUES (?, ?, ?, ?, ?, ?, 'ready', ?, ?)"#,
        )
        .bind(&id)
        .bind(agent_id)
        .bind(environment_id)
        .bind(profile_name)
        .bind(loadout_name)
        .bind(&primary_thread_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create managed agent session")?;

        Ok(ManagedAgentSession {
            id,
            agent_id: agent_id.to_string(),
            environment_id: environment_id.to_string(),
            profile_name: profile_name.to_string(),
            loadout_name: loadout_name.to_string(),
            primary_thread_id,
            status: ManagedAgentSessionStatus::Ready,
            last_task_id: None,
            created_at: now,
            last_active_at: now,
        })
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<ManagedAgentSession>> {
        sqlx::query(
            r#"SELECT id, agent_id, environment_id, profile_name, loadout_name, primary_thread_id,
                      status, last_task_id, created_at, last_active_at
               FROM managed_agent_sessions
               WHERE id = ?"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch managed agent session")
        .and_then(|row| row.map(row_to_session).transpose())
    }

    pub async fn list_sessions(
        &self,
        limit: i64,
        agent_id: Option<&str>,
    ) -> Result<Vec<ManagedAgentSession>> {
        let rows = sqlx::query(
            r#"SELECT id, agent_id, environment_id, profile_name, loadout_name, primary_thread_id,
                      status, last_task_id, created_at, last_active_at
               FROM managed_agent_sessions
               WHERE (?1 IS NULL OR agent_id = ?1)
               ORDER BY last_active_at DESC
               LIMIT ?2"#,
        )
        .bind(agent_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list managed agent sessions")?;

        rows.into_iter().map(row_to_session).collect()
    }

    pub async fn touch_session(&self, session_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"UPDATE managed_agent_sessions
               SET last_active_at = ?
               WHERE id = ?"#,
        )
        .bind(now)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .context("Failed to touch managed agent session")?;
        Ok(())
    }

    pub async fn update_status(
        &self,
        session_id: &str,
        status: ManagedAgentSessionStatus,
        last_task_id: Option<&str>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"UPDATE managed_agent_sessions
               SET status = ?,
                   last_task_id = COALESCE(?, last_task_id),
                   last_active_at = ?
               WHERE id = ?"#,
        )
        .bind(status.as_str())
        .bind(last_task_id)
        .bind(now)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .context("Failed to update managed agent session status")?;
        Ok(())
    }

    pub async fn append_event(
        &self,
        session_id: &str,
        event_type: &str,
        task_id: Option<&str>,
        thread_id: Option<&str>,
        payload: &Value,
    ) -> Result<ManagedAgentSessionEvent> {
        let now = chrono::Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();
        let payload_json =
            serde_json::to_string(payload).context("Failed to serialize managed session event")?;

        let result = sqlx::query(
            r#"INSERT INTO managed_agent_session_events
               (id, session_id, event_type, task_id, thread_id, payload_json, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(event_type)
        .bind(task_id)
        .bind(thread_id)
        .bind(&payload_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to append managed agent session event")?;

        Ok(ManagedAgentSessionEvent {
            position: result.last_insert_rowid(),
            id,
            session_id: session_id.to_string(),
            event_type: event_type.to_string(),
            task_id: task_id.map(ToOwned::to_owned),
            thread_id: thread_id.map(ToOwned::to_owned),
            payload: payload.clone(),
            created_at: now,
        })
    }

    pub async fn list_events(
        &self,
        session_id: &str,
        after_position: Option<i64>,
        limit: i64,
    ) -> Result<Vec<ManagedAgentSessionEvent>> {
        let rows = sqlx::query(
            r#"SELECT position, id, session_id, event_type, task_id, thread_id, payload_json, created_at
               FROM managed_agent_session_events
               WHERE session_id = ?
                 AND (?2 IS NULL OR position > ?2)
               ORDER BY position ASC
               LIMIT ?3"#,
        )
        .bind(session_id)
        .bind(after_position)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list managed agent session events")?;

        rows.into_iter().map(row_to_event).collect()
    }
}

fn row_to_session(row: sqlx::sqlite::SqliteRow) -> Result<ManagedAgentSession> {
    Ok(ManagedAgentSession {
        id: row.get("id"),
        agent_id: row.get("agent_id"),
        environment_id: row.get("environment_id"),
        profile_name: row.get("profile_name"),
        loadout_name: row.get("loadout_name"),
        primary_thread_id: row.get("primary_thread_id"),
        status: row
            .get::<String, _>("status")
            .parse()
            .unwrap_or(ManagedAgentSessionStatus::Failed),
        last_task_id: row.get("last_task_id"),
        created_at: row.get("created_at"),
        last_active_at: row.get("last_active_at"),
    })
}

fn row_to_event(row: sqlx::sqlite::SqliteRow) -> Result<ManagedAgentSessionEvent> {
    let payload = row.get::<String, _>("payload_json");
    Ok(ManagedAgentSessionEvent {
        position: row.get("position"),
        id: row.get("id"),
        session_id: row.get("session_id"),
        event_type: row.get("event_type"),
        task_id: row.get("task_id"),
        thread_id: row.get("thread_id"),
        payload: serde_json::from_str(&payload)
            .with_context(|| format!("Failed to parse managed session event payload: {payload}"))?,
        created_at: row.get("created_at"),
    })
}

#[cfg(test)]
mod tests {
    use super::ManagedAgentSessionStatus;
    use crate::storage::Database;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn managed_agent_sessions_persist_and_list() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("managed-agents.db"))
            .await
            .unwrap();
        let repo = db.managed_agents();

        let session = repo
            .create_session("agent.release", "env:default", "default", "default")
            .await
            .unwrap();

        let listed = repo.list_sessions(10, Some("agent.release")).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, session.id);
        assert_eq!(listed[0].status, ManagedAgentSessionStatus::Ready);
        assert_eq!(
            listed[0].primary_thread_id,
            format!("compat:{}:primary", session.id)
        );
    }

    #[tokio::test]
    async fn managed_agent_events_are_append_only_and_filterable() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("managed-agent-events.db"))
            .await
            .unwrap();
        let repo = db.managed_agents();

        let session = repo
            .create_session("agent.release", "env:default", "default", "default")
            .await
            .unwrap();

        let created = repo
            .append_event(
                &session.id,
                "session.created",
                None,
                Some(&session.primary_thread_id),
                &json!({"agent_id": "agent.release"}),
            )
            .await
            .unwrap();
        let completed = repo
            .append_event(
                &session.id,
                "task.completed",
                Some("task-1"),
                Some(&session.primary_thread_id),
                &json!({"answer": "done"}),
            )
            .await
            .unwrap();

        assert!(completed.position > created.position);

        let filtered = repo
            .list_events(&session.id, Some(created.position), 10)
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].event_type, "task.completed");
        assert_eq!(filtered[0].task_id.as_deref(), Some("task-1"));
        assert_eq!(filtered[0].payload["answer"], "done");
    }

    #[tokio::test]
    async fn managed_agent_session_status_updates_last_task() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("managed-agent-status.db"))
            .await
            .unwrap();
        let repo = db.managed_agents();

        let session = repo
            .create_session("agent.release", "env:default", "default", "default")
            .await
            .unwrap();

        repo.update_status(
            &session.id,
            ManagedAgentSessionStatus::Idle,
            Some("task-42"),
        )
        .await
        .unwrap();

        let fetched = repo.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(fetched.status, ManagedAgentSessionStatus::Idle);
        assert_eq!(fetched.last_task_id.as_deref(), Some("task-42"));
    }
}
