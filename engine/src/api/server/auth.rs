//! Local daemon authentication and session middleware.
//!
//! The daemon is the trust boundary for the hosted WebUI. It exposes a
//! password-based local auth model with short-lived bearer sessions.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sdk::{AuthState, AuthStatus, SessionInfo};
use uuid::Uuid;

use super::AppState;
use crate::config::Config;
use crate::remote::RemoteManager;
use crate::security::{
    configure_password_for_config, password_protection_state, verify_password,
    PasswordProtectionState,
};
use crate::storage::{AuthSession, Database};

const SENSITIVE_AREAS: &[&str] = &["secrets", "extensions", "remote_trust", "policy"];

#[derive(Clone)]
pub struct AuthManager {
    db: Arc<Database>,
}

impl AuthManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn auth_state(&self) -> Result<AuthState> {
        let config = Config::load_or_create()?;
        Ok(match password_protection_state(&config)? {
            PasswordProtectionState::Uninitialized => AuthState::Uninitialized,
            PasswordProtectionState::Tampered => AuthState::Tampered,
            PasswordProtectionState::LegacyUnsealed | PasswordProtectionState::Sealed => {
                AuthState::Locked
            }
        })
    }

    pub fn ensure_origin_allowed(&self, headers: &HeaderMap) -> Result<()> {
        let config = Config::load_or_create()?;
        let Some(origin) = headers.get("origin").and_then(header_to_str) else {
            return Ok(());
        };

        if config
            .webui
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin)
        {
            return Ok(());
        }

        bail!("Origin '{}' is not allowed to control the daemon", origin);
    }

    pub async fn setup(
        &self,
        password: &str,
        node_name: Option<&str>,
        privacy_mode: Option<&str>,
        headers: &HeaderMap,
    ) -> Result<SessionInfo> {
        self.ensure_origin_allowed(headers)?;

        let mut config = Config::load_or_create()?;
        if config.webui.password_hash.is_some() {
            bail!("Daemon password is already configured");
        }

        let config_path = Config::config_path()?;
        configure_password_for_config(&config_path, &mut config.webui, password)?;
        if let Some(mode) = privacy_mode.filter(|value| !value.trim().is_empty()) {
            config.webui.privacy_mode = mode.trim().to_string();
        }
        config.save()?;

        if let Some(name) = node_name.filter(|value| !value.trim().is_empty()) {
            let _ = RemoteManager::new(config.clone()).rename(name.trim());
        }

        self.db.auth().revoke_all_sessions().await?;
        self.create_session(headers).await
    }

    pub async fn login(&self, password: &str, headers: &HeaderMap) -> Result<SessionInfo> {
        self.ensure_origin_allowed(headers)?;

        let config = Config::load_or_create()?;
        if matches!(
            password_protection_state(&config)?,
            PasswordProtectionState::Tampered
        ) {
            bail!(
                "Password integrity check failed. Run `rove auth reset-password` on this machine"
            );
        }
        let Some(password_hash) = &config.webui.password_hash else {
            bail!("Daemon password has not been configured yet");
        };

        if !verify_password(password, password_hash)? {
            bail!("Invalid password");
        }

        self.create_session(headers).await
    }

    pub async fn reauth(
        &self,
        token: &str,
        password: &str,
        headers: &HeaderMap,
    ) -> Result<AuthStatus> {
        self.ensure_origin_allowed(headers)?;
        let validated = self.validate_session(token, true).await?;

        let config = Config::load_or_create()?;
        if matches!(
            password_protection_state(&config)?,
            PasswordProtectionState::Tampered
        ) {
            bail!(
                "Password integrity check failed. Run `rove auth reset-password` on this machine"
            );
        }
        let Some(password_hash) = &config.webui.password_hash else {
            bail!("Daemon password has not been configured yet");
        };
        if !verify_password(password, password_hash)? {
            bail!("Invalid password");
        }

        self.db
            .auth()
            .set_reauth(
                &validated.session.session_id,
                config.webui.reauth_window_secs as i64,
            )
            .await?;

        self.status_for_token(token).await
    }

    pub async fn lock(&self, token: &str) -> Result<()> {
        self.db.auth().revoke_session(token).await
    }

    pub async fn status_for_token(&self, token: &str) -> Result<AuthStatus> {
        let validated = self.validate_session(token, true).await?;
        Ok(validated.status)
    }

    pub async fn validate_session(&self, token: &str, touch: bool) -> Result<ValidatedSession> {
        let config = Config::load_or_create()?;
        match password_protection_state(&config)? {
            PasswordProtectionState::Uninitialized => {
                bail!("Daemon password has not been configured yet");
            }
            PasswordProtectionState::Tampered => {
                bail!("Password integrity check failed. Run `rove auth reset-password` on this machine");
            }
            PasswordProtectionState::LegacyUnsealed | PasswordProtectionState::Sealed => {}
        }

        let Some(mut session) = self.db.auth().get_session(token).await? else {
            bail!("Session not found");
        };

        let now = now_ts()?;
        if session.revoked_at.is_some()
            || session.expires_at <= now
            || session.absolute_expires_at <= now
        {
            let _ = self.db.auth().revoke_session(token).await;
            bail!("Session expired");
        }

        if touch {
            session = self
                .db
                .auth()
                .touch_session(token, config.webui.idle_timeout_secs as i64)
                .await?
                .context("Session disappeared while updating expiry")?;
        }

        let reauth = self.db.auth().get_reauth(token).await?;
        let reauth_valid = reauth
            .as_ref()
            .is_some_and(|reauth| reauth.expires_at > now);
        let state = if session.requires_reauth && !reauth_valid {
            AuthState::ReauthRequired
        } else {
            AuthState::Unlocked
        };

        Ok(ValidatedSession {
            session: session.clone(),
            status: AuthStatus {
                state,
                idle_expires_in_secs: Some(session.expires_at.saturating_sub(now) as u64),
                absolute_expires_in_secs: Some(
                    session.absolute_expires_at.saturating_sub(now) as u64
                ),
            },
        })
    }

    pub fn bearer_token(headers: &HeaderMap) -> Option<String> {
        headers
            .get("Authorization")
            .and_then(header_to_str)
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    async fn create_session(&self, headers: &HeaderMap) -> Result<SessionInfo> {
        let config = Config::load_or_create()?;
        let session_id = Uuid::new_v4().to_string();
        let origin = headers.get("origin").and_then(header_to_str);
        let user_agent = headers.get("user-agent").and_then(header_to_str);

        let session = self
            .db
            .auth()
            .create_session(
                &session_id,
                config.webui.idle_timeout_secs as i64,
                config.webui.absolute_timeout_secs as i64,
                Some("webui"),
                origin,
                user_agent,
            )
            .await?;

        Ok(SessionInfo {
            access_token: session.session_id,
            expires_in_secs: config.webui.idle_timeout_secs,
            absolute_expires_in_secs: config.webui.absolute_timeout_secs,
            reauth_required_for: SENSITIVE_AREAS
                .iter()
                .map(|value| value.to_string())
                .collect(),
        })
    }
}

pub struct ValidatedSession {
    pub session: AuthSession,
    pub status: AuthStatus,
}

pub async fn require_session_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let manager = AuthManager::new(state.db.clone());
    if let Err(error) = manager.ensure_origin_allowed(req.headers()) {
        return (StatusCode::FORBIDDEN, error.to_string()).into_response();
    }

    let Some(token) = AuthManager::bearer_token(req.headers()) else {
        return (StatusCode::UNAUTHORIZED, "Missing bearer token").into_response();
    };

    match manager.validate_session(&token, true).await {
        Ok(validated) => {
            if matches!(validated.status.state, AuthState::ReauthRequired) {
                return (StatusCode::UNAUTHORIZED, "Reauthentication required").into_response();
            }
            next.run(req).await
        }
        Err(error) => (StatusCode::UNAUTHORIZED, error.to_string()).into_response(),
    }
}

fn header_to_str(value: &HeaderValue) -> Option<&str> {
    value.to_str().ok()
}

fn now_ts() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before unix epoch")?
        .as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_extracts_value() {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_static("Bearer abc123"));
        assert_eq!(
            AuthManager::bearer_token(&headers).as_deref(),
            Some("abc123")
        );
    }
}
