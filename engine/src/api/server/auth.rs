//! Local daemon authentication and session middleware.
//!
//! The daemon is the trust boundary for the hosted WebUI. It exposes a
//! password-based local auth model with short-lived bearer sessions.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use reqwest::Url;
use sdk::{
    AuthState, AuthStatus, PasskeyChallengeResponse, PasskeyDescriptor, PasskeyFinishRequest,
    PasskeyRegistrationStartRequest, PasskeyStatus, SessionInfo,
};
use uuid::Uuid;
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Webauthn, WebauthnBuilder,
};

use super::AppState;
use crate::config::Config;
use crate::remote::RemoteManager;
use crate::security::{
    configure_password_for_config, password_protection_state, verify_password,
    PasswordProtectionState,
};
use crate::storage::auth::{AuthPasskeyChallenge, AuthPasskeyRecord};
use crate::storage::{AuthSession, Database};

const SENSITIVE_AREAS: &[&str] = &["secrets", "extensions", "remote_trust", "policy"];
const PASSKEY_CHALLENGE_WINDOW_SECS: i64 = 300;

#[derive(Clone)]
pub struct AuthManager {
    db: Arc<Database>,
}

struct PasskeyContext {
    origin_string: String,
    rp_id: String,
    webauthn: Webauthn,
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

    pub async fn passkey_status(&self, headers: &HeaderMap) -> Result<PasskeyStatus> {
        let Some(context) = self.try_passkey_context(headers)? else {
            return Ok(PasskeyStatus {
                supported: false,
                registered: false,
                credential_count: 0,
            });
        };

        let records = self.db.auth().list_passkeys_for_rp(&context.rp_id).await?;
        Ok(PasskeyStatus {
            supported: true,
            registered: !records.is_empty(),
            credential_count: records.len(),
        })
    }

    pub async fn list_passkeys(&self) -> Result<Vec<PasskeyDescriptor>> {
        let records = self.db.auth().list_passkeys().await?;
        Ok(records.into_iter().map(Self::to_passkey_descriptor).collect())
    }

    pub async fn delete_passkey(&self, token: &str, id: &str) -> Result<bool> {
        let validated = self.validate_session(token, true).await?;
        let removed = self.db.auth().delete_passkey(id).await?;
        if removed {
            self.db
                .auth()
                .record_event(
                    "passkey_removed",
                    Some(&validated.session.session_id),
                    Some(&serde_json::json!({ "id": id }).to_string()),
                )
                .await?;
        }
        Ok(removed)
    }

    pub async fn start_passkey_registration(
        &self,
        token: &str,
        payload: &PasskeyRegistrationStartRequest,
        headers: &HeaderMap,
    ) -> Result<PasskeyChallengeResponse> {
        let validated = self.validate_session(token, true).await?;
        let context = self.passkey_context(headers)?;
        let user_uuid = self.ensure_passkey_user_uuid()?;
        let existing_passkeys = self.load_passkeys_for_rp(&context.rp_id).await?;
        let exclude_credentials = if existing_passkeys.is_empty() {
            None
        } else {
            Some(
                existing_passkeys
                    .iter()
                    .map(|(_, passkey)| passkey.cred_id().clone())
                    .collect(),
            )
        };
        let (options, state) = context.webauthn.start_passkey_registration(
            user_uuid,
            "rove-local-user",
            "Rove Local User",
            exclude_credentials,
        )?;
        let challenge = self.new_passkey_challenge(
            "register",
            Some(validated.session.session_id.clone()),
            &context,
            serde_json::to_string(&state)?,
            payload.label.clone(),
        )?;
        self.db.auth().create_passkey_challenge(&challenge).await?;
        Ok(PasskeyChallengeResponse {
            challenge_id: challenge.challenge_id,
            options: serde_json::to_value(options)?,
        })
    }

    pub async fn finish_passkey_registration(
        &self,
        token: &str,
        payload: &PasskeyFinishRequest,
        headers: &HeaderMap,
    ) -> Result<PasskeyDescriptor> {
        let validated = self.validate_session(token, true).await?;
        let context = self.passkey_context(headers)?;
        let challenge = self
            .db
            .auth()
            .take_passkey_challenge(&payload.challenge_id, "register")
            .await?
            .context("Passkey registration challenge expired or was not found")?;
        self.assert_challenge_matches(&challenge, &validated.session.session_id, &context)?;
        let registration: PasskeyRegistration = serde_json::from_str(&challenge.state_json)?;
        let credential: RegisterPublicKeyCredential =
            serde_json::from_value(payload.credential.clone())?;
        let passkey = context
            .webauthn
            .finish_passkey_registration(&credential, &registration)?;
        let label = challenge.label.clone();

        let records = self.db.auth().list_passkeys_for_rp(&context.rp_id).await?;
        if records
            .iter()
            .any(|record| record.credential_id == credential.id)
        {
            bail!("This passkey is already registered");
        }

        let user_uuid = self.ensure_passkey_user_uuid()?;
        let id = Uuid::new_v4().to_string();
        self.db
            .auth()
            .insert_passkey(
                &id,
                &user_uuid.to_string(),
                &context.rp_id,
                &credential.id,
                label.as_deref(),
                &serde_json::to_string(&passkey)?,
            )
            .await?;
        self.db
            .auth()
            .record_event(
                "passkey_registered",
                Some(&validated.session.session_id),
                Some(
                    &serde_json::json!({
                        "id": id,
                        "rp_id": context.rp_id,
                        "label": label,
                    })
                    .to_string(),
                ),
            )
            .await?;

        Ok(PasskeyDescriptor {
            id,
            label,
            rp_id: context.rp_id,
            created_at: now_ts()?,
            last_used_at: None,
        })
    }

    pub async fn start_passkey_login(&self, headers: &HeaderMap) -> Result<PasskeyChallengeResponse> {
        let context = self.passkey_context(headers)?;
        let passkeys = self.load_passkeys_for_rp(&context.rp_id).await?;
        if passkeys.is_empty() {
            bail!("No passkeys are registered for this origin");
        }
        let credentials = passkeys
            .iter()
            .map(|(_, passkey)| passkey.clone())
            .collect::<Vec<_>>();
        let (options, state) = context.webauthn.start_passkey_authentication(&credentials)?;
        let challenge = self.new_passkey_challenge(
            "login",
            None,
            &context,
            serde_json::to_string(&state)?,
            None,
        )?;
        self.db.auth().create_passkey_challenge(&challenge).await?;
        Ok(PasskeyChallengeResponse {
            challenge_id: challenge.challenge_id,
            options: serde_json::to_value(options)?,
        })
    }

    pub async fn finish_passkey_login(
        &self,
        payload: &PasskeyFinishRequest,
        headers: &HeaderMap,
    ) -> Result<SessionInfo> {
        let context = self.passkey_context(headers)?;
        let challenge = self
            .db
            .auth()
            .take_passkey_challenge(&payload.challenge_id, "login")
            .await?
            .context("Passkey login challenge expired or was not found")?;
        self.assert_public_challenge_matches(&challenge, &context)?;
        let authentication: PasskeyAuthentication = serde_json::from_str(&challenge.state_json)?;
        let credential: PublicKeyCredential =
            serde_json::from_value(payload.credential.clone())?;
        let result = context
            .webauthn
            .finish_passkey_authentication(&credential, &authentication)?;
        self.update_passkey_after_authentication(&context.rp_id, &credential.id, &result)
            .await?;
        let session = self.create_session(headers).await?;
        self.db
            .auth()
            .record_event(
                "passkey_login",
                Some(&session.access_token),
                Some(&serde_json::json!({ "rp_id": context.rp_id }).to_string()),
            )
            .await?;
        Ok(session)
    }

    pub async fn start_passkey_reauth(
        &self,
        token: &str,
        headers: &HeaderMap,
    ) -> Result<PasskeyChallengeResponse> {
        let validated = self.validate_session(token, false).await?;
        let context = self.passkey_context(headers)?;
        let passkeys = self.load_passkeys_for_rp(&context.rp_id).await?;
        if passkeys.is_empty() {
            bail!("No passkeys are registered for this origin");
        }
        let credentials = passkeys
            .iter()
            .map(|(_, passkey)| passkey.clone())
            .collect::<Vec<_>>();
        let (options, state) = context.webauthn.start_passkey_authentication(&credentials)?;
        let challenge = self.new_passkey_challenge(
            "reauth",
            Some(validated.session.session_id),
            &context,
            serde_json::to_string(&state)?,
            None,
        )?;
        self.db.auth().create_passkey_challenge(&challenge).await?;
        Ok(PasskeyChallengeResponse {
            challenge_id: challenge.challenge_id,
            options: serde_json::to_value(options)?,
        })
    }

    pub async fn finish_passkey_reauth(
        &self,
        token: &str,
        payload: &PasskeyFinishRequest,
        headers: &HeaderMap,
    ) -> Result<AuthStatus> {
        let validated = self.validate_session(token, false).await?;
        let config = Config::load_or_create()?;
        let context = self.passkey_context(headers)?;
        let challenge = self
            .db
            .auth()
            .take_passkey_challenge(&payload.challenge_id, "reauth")
            .await?
            .context("Passkey reauthentication challenge expired or was not found")?;
        self.assert_challenge_matches(&challenge, &validated.session.session_id, &context)?;
        let authentication: PasskeyAuthentication = serde_json::from_str(&challenge.state_json)?;
        let credential: PublicKeyCredential =
            serde_json::from_value(payload.credential.clone())?;
        let result = context
            .webauthn
            .finish_passkey_authentication(&credential, &authentication)?;
        self.update_passkey_after_authentication(&context.rp_id, &credential.id, &result)
            .await?;
        self.db
            .auth()
            .set_reauth(
                &validated.session.session_id,
                config.webui.reauth_window_secs as i64,
            )
            .await?;
        self.db
            .auth()
            .record_event(
                "passkey_reauth",
                Some(&validated.session.session_id),
                Some(&serde_json::json!({ "rp_id": context.rp_id }).to_string()),
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

    fn query_token(uri: &axum::http::Uri) -> Option<String> {
        uri.query().and_then(|query| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                if key == "token" && !value.trim().is_empty() {
                    Some(value.to_string())
                } else {
                    None
                }
            })
        })
    }

    fn token_from_request(req: &Request) -> Option<String> {
        Self::bearer_token(req.headers()).or_else(|| {
            let path = req.uri().path();
            if path == "/v1/events/ws" || path == "/ws/task" || path == "/ws/telemetry" {
                Self::query_token(req.uri())
            } else {
                None
            }
        })
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

    fn try_passkey_context(&self, headers: &HeaderMap) -> Result<Option<PasskeyContext>> {
        self.ensure_origin_allowed(headers)?;
        let Some(origin_string) = headers.get("origin").and_then(header_to_str) else {
            return Ok(None);
        };
        let origin = Url::parse(origin_string).context("Invalid browser origin for passkeys")?;
        let Some(host) = origin.host_str() else {
            return Ok(None);
        };
        if host.parse::<IpAddr>().is_ok() {
            return Ok(None);
        }
        let webauthn = WebauthnBuilder::new(host, &origin)
            .context("Current origin cannot be used for passkeys")?
            .allow_any_port(true)
            .rp_name("Rove")
            .build()
            .context("Failed to build passkey relying-party configuration")?;

        Ok(Some(PasskeyContext {
            origin_string: origin_string.to_string(),
            rp_id: host.to_string(),
            webauthn,
        }))
    }

    fn passkey_context(&self, headers: &HeaderMap) -> Result<PasskeyContext> {
        self.try_passkey_context(headers)?
            .context("Passkeys require a domain origin such as http://localhost:3000")
    }

    fn ensure_passkey_user_uuid(&self) -> Result<Uuid> {
        let mut config = Config::load_or_create()?;
        if let Some(existing) = config.webui.passkey_user_uuid.as_deref() {
            if let Ok(uuid) = Uuid::parse_str(existing) {
                return Ok(uuid);
            }
        }

        let user_uuid = Uuid::new_v4();
        config.webui.passkey_user_uuid = Some(user_uuid.to_string());
        config.save()?;
        Ok(user_uuid)
    }

    async fn load_passkeys_for_rp(&self, rp_id: &str) -> Result<Vec<(String, Passkey)>> {
        let records = self.db.auth().list_passkeys_for_rp(rp_id).await?;
        let mut passkeys = Vec::with_capacity(records.len());
        for record in records {
            let passkey: Passkey = serde_json::from_str(&record.passkey_json)
                .with_context(|| format!("Stored passkey '{}' could not be decoded", record.id))?;
            passkeys.push((record.id, passkey));
        }
        Ok(passkeys)
    }

    fn new_passkey_challenge(
        &self,
        challenge_type: &str,
        session_id: Option<String>,
        context: &PasskeyContext,
        state_json: String,
        label: Option<String>,
    ) -> Result<AuthPasskeyChallenge> {
        let created_at = now_ts()?;
        Ok(AuthPasskeyChallenge {
            challenge_id: Uuid::new_v4().to_string(),
            challenge_type: challenge_type.to_string(),
            session_id,
            rp_id: context.rp_id.clone(),
            origin: context.origin_string.clone(),
            state_json,
            label,
            created_at,
            expires_at: created_at + PASSKEY_CHALLENGE_WINDOW_SECS,
        })
    }

    fn assert_challenge_matches(
        &self,
        challenge: &AuthPasskeyChallenge,
        session_id: &str,
        context: &PasskeyContext,
    ) -> Result<()> {
        if challenge.session_id.as_deref() != Some(session_id) {
            bail!("Passkey challenge does not belong to the current session");
        }
        self.assert_public_challenge_matches(challenge, context)
    }

    fn assert_public_challenge_matches(
        &self,
        challenge: &AuthPasskeyChallenge,
        context: &PasskeyContext,
    ) -> Result<()> {
        if challenge.rp_id != context.rp_id || challenge.origin != context.origin_string {
            bail!("Passkey challenge origin does not match the current browser origin");
        }
        Ok(())
    }

    async fn update_passkey_after_authentication(
        &self,
        rp_id: &str,
        credential_id: &str,
        result: &webauthn_rs::prelude::AuthenticationResult,
    ) -> Result<()> {
        let now = now_ts()?;
        let records = self.db.auth().list_passkeys_for_rp(rp_id).await?;
        for record in records {
            let mut passkey: Passkey = serde_json::from_str(&record.passkey_json)?;
            let matched = passkey.update_credential(result);
            if matched.is_some() || record.credential_id == credential_id {
                self.db
                    .auth()
                    .update_passkey(&record.id, &serde_json::to_string(&passkey)?, Some(now))
                    .await?;
                return Ok(());
            }
        }
        bail!("Passkey credential is not registered on this origin")
    }

    fn to_passkey_descriptor(record: AuthPasskeyRecord) -> PasskeyDescriptor {
        PasskeyDescriptor {
            id: record.id,
            label: record.label,
            rp_id: record.rp_id,
            created_at: record.created_at,
            last_used_at: record.last_used_at,
        }
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

    let Some(token) = AuthManager::token_from_request(&req) else {
        return (StatusCode::UNAUTHORIZED, "Missing bearer token").into_response();
    };

    match manager.validate_session(&token, true).await {
        Ok(validated) => {
            let path = req.uri().path();
            let allows_reauth = path == "/v1/auth/status"
                || path == "/v1/auth/lock"
                || path == "/v1/auth/reauth"
                || path == "/v1/auth/passkeys/reauth/start"
                || path == "/v1/auth/passkeys/reauth/finish";
            if matches!(validated.status.state, AuthState::ReauthRequired) && !allows_reauth {
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

    #[test]
    fn query_token_extracts_value() {
        let uri: axum::http::Uri = "/v1/events/ws?token=abc123".parse().expect("uri");
        assert_eq!(AuthManager::query_token(&uri).as_deref(), Some("abc123"));
    }
}
