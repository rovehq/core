//! Bearer Token Authentication Middleware
//!
//! Validates `Authorization: Bearer <token>` header against the
//! configured token. Used to gate the WebUI and REST API endpoints.
//! Token is stored in the OS keychain via `SecretManager`.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::AppState;

/// Middleware: reject requests without a valid bearer token.
///
/// The expected token is stored via `SecretManager` under the key
/// `"webui_token"`. If no token is configured, access is denied
/// until the user runs `rove daemon token --set`.
pub async fn require_bearer_token(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let provided = match auth_header.and_then(|h| h.strip_prefix("Bearer ")) {
        Some(t) => t.to_owned(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid Authorization header",
            )
                .into_response();
        }
    };

    // Look up the configured token from the secret manager
    let expected = match state.secret_manager.get_secret("webui_token").await {
        Ok(t) => t,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "No WebUI token configured. Run: rove daemon token --set",
            )
                .into_response();
        }
    };

    // Constant-time comparison to prevent timing attacks
    if !constant_time_eq(&provided, &expected) {
        return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
    }

    next.run(req).await
}

/// Constant-time byte comparison (avoids early-exit timing leaks).
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq_matches() {
        assert!(constant_time_eq("token123", "token123"));
    }

    #[test]
    fn test_constant_time_eq_different_content() {
        assert!(!constant_time_eq("token123", "token456"));
    }

    #[test]
    fn test_constant_time_eq_different_length() {
        assert!(!constant_time_eq("short", "longer_token"));
    }
}
