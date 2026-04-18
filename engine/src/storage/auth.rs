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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthPasskeyRecord {
    pub id: String,
    pub user_uuid: String,
    pub rp_id: String,
    pub credential_id: String,
    pub label: Option<String>,
    pub passkey_json: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthPasskeyChallenge {
    pub challenge_id: String,
    pub challenge_type: String,
    pub session_id: Option<String>,
    pub rp_id: String,
    pub origin: String,
    pub state_json: String,
    pub label: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
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

        sqlx::query(
            "UPDATE auth_sessions SET last_seen_at = ?, expires_at = ? WHERE session_id = ?",
        )
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
        self.record_event("all_sessions_revoked", None, None)
            .await?;
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

    pub async fn list_passkeys(&self) -> Result<Vec<AuthPasskeyRecord>> {
        let rows = sqlx::query(
            "SELECT id, user_uuid, rp_id, credential_id, label, passkey_json, created_at, last_used_at
             FROM auth_passkeys
             ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list auth passkeys")?;

        Ok(rows.into_iter().map(map_passkey).collect())
    }

    pub async fn list_passkeys_for_rp(&self, rp_id: &str) -> Result<Vec<AuthPasskeyRecord>> {
        let rows = sqlx::query(
            "SELECT id, user_uuid, rp_id, credential_id, label, passkey_json, created_at, last_used_at
             FROM auth_passkeys
             WHERE rp_id = ?
             ORDER BY created_at DESC",
        )
        .bind(rp_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list auth passkeys for rp_id")?;

        Ok(rows.into_iter().map(map_passkey).collect())
    }

    pub async fn insert_passkey(
        &self,
        id: &str,
        user_uuid: &str,
        rp_id: &str,
        credential_id: &str,
        label: Option<&str>,
        passkey_json: &str,
    ) -> Result<()> {
        let now = now_ts()?;
        sqlx::query(
            "INSERT INTO auth_passkeys (
                id, user_uuid, rp_id, credential_id, label, passkey_json, created_at, last_used_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind(id)
        .bind(user_uuid)
        .bind(rp_id)
        .bind(credential_id)
        .bind(label)
        .bind(passkey_json)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert auth passkey")?;
        Ok(())
    }

    pub async fn update_passkey(
        &self,
        id: &str,
        passkey_json: &str,
        last_used_at: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE auth_passkeys
             SET passkey_json = ?, last_used_at = COALESCE(?, last_used_at)
             WHERE id = ?",
        )
        .bind(passkey_json)
        .bind(last_used_at)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to update auth passkey")?;
        Ok(())
    }

    pub async fn delete_passkey(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM auth_passkeys WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete auth passkey")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn create_passkey_challenge(
        &self,
        challenge: &AuthPasskeyChallenge,
    ) -> Result<()> {
        self.delete_expired_passkey_challenges().await?;
        sqlx::query(
            "INSERT INTO auth_passkey_challenges (
                challenge_id, challenge_type, session_id, rp_id, origin, state_json, label, created_at, expires_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&challenge.challenge_id)
        .bind(&challenge.challenge_type)
        .bind(challenge.session_id.as_deref())
        .bind(&challenge.rp_id)
        .bind(&challenge.origin)
        .bind(&challenge.state_json)
        .bind(challenge.label.as_deref())
        .bind(challenge.created_at)
        .bind(challenge.expires_at)
        .execute(&self.pool)
        .await
        .context("Failed to create auth passkey challenge")?;
        Ok(())
    }

    pub async fn take_passkey_challenge(
        &self,
        challenge_id: &str,
        challenge_type: &str,
    ) -> Result<Option<AuthPasskeyChallenge>> {
        self.delete_expired_passkey_challenges().await?;
        let row = sqlx::query(
            "SELECT challenge_id, challenge_type, session_id, rp_id, origin, state_json, label, created_at, expires_at
             FROM auth_passkey_challenges
             WHERE challenge_id = ? AND challenge_type = ?",
        )
        .bind(challenge_id)
        .bind(challenge_type)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch auth passkey challenge")?;

        let Some(row) = row else {
            return Ok(None);
        };
        let challenge = map_passkey_challenge(row);
        sqlx::query("DELETE FROM auth_passkey_challenges WHERE challenge_id = ?")
            .bind(challenge_id)
            .execute(&self.pool)
            .await
            .context("Failed to delete consumed auth passkey challenge")?;
        Ok(Some(challenge))
    }

    async fn delete_expired_passkey_challenges(&self) -> Result<()> {
        let now = now_ts()?;
        sqlx::query("DELETE FROM auth_passkey_challenges WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await
            .context("Failed to delete expired auth passkey challenges")?;
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

fn map_passkey(row: sqlx::sqlite::SqliteRow) -> AuthPasskeyRecord {
    AuthPasskeyRecord {
        id: row.get("id"),
        user_uuid: row.get("user_uuid"),
        rp_id: row.get("rp_id"),
        credential_id: row.get("credential_id"),
        label: row.get("label"),
        passkey_json: row.get("passkey_json"),
        created_at: row.get("created_at"),
        last_used_at: row.get("last_used_at"),
    }
}

fn map_passkey_challenge(row: sqlx::sqlite::SqliteRow) -> AuthPasskeyChallenge {
    AuthPasskeyChallenge {
        challenge_id: row.get("challenge_id"),
        challenge_type: row.get("challenge_type"),
        session_id: row.get("session_id"),
        rp_id: row.get("rp_id"),
        origin: row.get("origin"),
        state_json: row.get("state_json"),
        label: row.get("label"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
    }
}

fn now_ts() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?
        .as_secs() as i64)
}
