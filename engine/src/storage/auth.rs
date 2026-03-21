use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub session_id: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
    pub absolute_expires_at: i64,
    pub revoked_at: Option<i64>,
    pub client_label: Option<String>,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub requires_reauth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthReauth {
    pub session_id: String,
    pub verified_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthEvent {
    pub id: i64,
    pub event_type: String,
    pub created_at: i64,
    pub session_id: Option<String>,
    pub metadata: Option<String>,
}

pub struct AuthRepository {
    pool: SqlitePool,
}

impl AuthRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_session(
        &self,
        session_id: &str,
        idle_timeout_secs: i64,
        absolute_timeout_secs: i64,
        client_label: Option<&str>,
        origin: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<AuthSession> {
        let now = now_ts()?;
        let expires_at = now + idle_timeout_secs;
        let absolute_expires_at = now + absolute_timeout_secs;

        sqlx::query(
            "INSERT INTO auth_sessions (
                session_id, created_at, last_seen_at, expires_at, absolute_expires_at,
                revoked_at, client_label, origin, user_agent, requires_reauth
             ) VALUES (?, ?, ?, ?, ?, NULL, ?, ?, ?, 0)",
        )
        .bind(session_id)
        .bind(now)
        .bind(now)
        .bind(expires_at)
        .bind(absolute_expires_at)
        .bind(client_label)
        .bind(origin)
        .bind(user_agent)
        .execute(&self.pool)
        .await
        .context("Failed to create auth session")?;

        self.record_event("session_created", Some(session_id), None)
            .await?;

        Ok(AuthSession {
            session_id: session_id.to_string(),
            created_at: now,
            last_seen_at: now,
            expires_at,
            absolute_expires_at,
            revoked_at: None,
            client_label: client_label.map(ToOwned::to_owned),
            origin: origin.map(ToOwned::to_owned),
            user_agent: user_agent.map(ToOwned::to_owned),
            requires_reauth: false,
        })
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<AuthSession>> {
        let row = sqlx::query(
            "SELECT session_id, created_at, last_seen_at, expires_at, absolute_expires_at,
                    revoked_at, client_label, origin, user_agent, requires_reauth
             FROM auth_sessions WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch auth session")?;

        Ok(row.map(map_session))
    }

    pub async fn touch_session(
        &self,
        session_id: &str,
        idle_timeout_secs: i64,
    ) -> Result<Option<AuthSession>> {
        let Some(existing) = self.get_session(session_id).await? else {
            return Ok(None);
        };

        let now = now_ts()?;
        let expires_at = (now + idle_timeout_secs).min(existing.absolute_expires_at);

        sqlx::query("UPDATE auth_sessions SET last_seen_at = ?, expires_at = ? WHERE session_id = ?")
            .bind(now)
            .bind(expires_at)
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("Failed to touch auth session")?;

        self.get_session(session_id).await
    }

    pub async fn revoke_session(&self, session_id: &str) -> Result<()> {
        let now = now_ts()?;
        sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE session_id = ?")
            .bind(now)
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("Failed to revoke auth session")?;

        sqlx::query("DELETE FROM auth_reauth WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("Failed to clear reauth state")?;

        self.record_event("session_revoked", Some(session_id), None)
            .await?;
        Ok(())
    }

    pub async fn revoke_all_sessions(&self) -> Result<()> {
        let now = now_ts()?;
        sqlx::query("UPDATE auth_sessions SET revoked_at = ? WHERE revoked_at IS NULL")
            .bind(now)
            .execute(&self.pool)
            .await
            .context("Failed to revoke auth sessions")?;
        sqlx::query("DELETE FROM auth_reauth")
            .execute(&self.pool)
            .await
            .context("Failed to clear reauth table")?;
        self.record_event("all_sessions_revoked", None, None).await?;
        Ok(())
    }

    pub async fn set_reauth(&self, session_id: &str, window_secs: i64) -> Result<AuthReauth> {
        let now = now_ts()?;
        let expires_at = now + window_secs;

        sqlx::query(
            "INSERT INTO auth_reauth (session_id, verified_at, expires_at)
             VALUES (?, ?, ?)
             ON CONFLICT(session_id) DO UPDATE
               SET verified_at = excluded.verified_at,
                   expires_at = excluded.expires_at",
        )
        .bind(session_id)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .context("Failed to store reauth window")?;

        sqlx::query("UPDATE auth_sessions SET requires_reauth = 0 WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("Failed to clear reauth flag")?;

        self.record_event("reauth_verified", Some(session_id), None)
            .await?;

        Ok(AuthReauth {
            session_id: session_id.to_string(),
            verified_at: now,
            expires_at,
        })
    }

    pub async fn get_reauth(&self, session_id: &str) -> Result<Option<AuthReauth>> {
        let row = sqlx::query(
            "SELECT session_id, verified_at, expires_at FROM auth_reauth WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch reauth window")?;

        Ok(row.map(|row| AuthReauth {
            session_id: row.get("session_id"),
            verified_at: row.get("verified_at"),
            expires_at: row.get("expires_at"),
        }))
    }

    pub async fn require_reauth(&self, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE auth_sessions SET requires_reauth = 1 WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("Failed to mark session for reauth")?;
        self.record_event("reauth_required", Some(session_id), None)
            .await?;
        Ok(())
    }

    pub async fn record_event(
        &self,
        event_type: &str,
        session_id: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<()> {
        let now = now_ts()?;
        sqlx::query(
            "INSERT INTO auth_events (event_type, created_at, session_id, metadata)
             VALUES (?, ?, ?, ?)",
        )
        .bind(event_type)
        .bind(now)
        .bind(session_id)
        .bind(metadata)
        .execute(&self.pool)
        .await
        .context("Failed to append auth event")?;
        Ok(())
    }
}

fn map_session(row: sqlx::sqlite::SqliteRow) -> AuthSession {
    AuthSession {
        session_id: row.get("session_id"),
        created_at: row.get("created_at"),
        last_seen_at: row.get("last_seen_at"),
        expires_at: row.get("expires_at"),
        absolute_expires_at: row.get("absolute_expires_at"),
        revoked_at: row.get("revoked_at"),
        client_label: row.get("client_label"),
        origin: row.get("origin"),
        user_agent: row.get("user_agent"),
        requires_reauth: row.get::<i64, _>("requires_reauth") != 0,
    }
}

fn now_ts() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?
        .as_secs() as i64)
}
