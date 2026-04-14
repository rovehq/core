use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{bail, Context, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::config::{Config, WebUIConfig};
use crate::platform::{keychain_get, keychain_set};

type HmacSha256 = Hmac<Sha256>;

const AUTH_KEYCHAIN_PREFIX: &str = "rove.daemon-auth.v1";
const RECOVERY_PREFIX: &str = "RVE";

static KEYCHAIN_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordProtectionState {
    Uninitialized,
    LegacyUnsealed,
    Sealed,
    Tampered,
}

#[derive(Debug, Clone)]
pub struct PasswordSetupArtifacts {
    pub protection_state: PasswordProtectionState,
    pub recovery_code: String,
}

pub fn hash_password(password: &str) -> Result<String> {
    if password.trim().len() < 8 {
        bail!("Password must be at least 8 characters");
    }
    hash_secret_value(password)
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<bool> {
    verify_secret_hash(password_hash, password)
}

pub fn configure_password_for_config(
    config_path: &Path,
    webui: &mut WebUIConfig,
    password: &str,
) -> Result<PasswordSetupArtifacts> {
    apply_password_config(config_path, webui, password)
}

pub fn reset_password_for_config(
    config_path: &Path,
    webui: &mut WebUIConfig,
    password: &str,
) -> Result<PasswordSetupArtifacts> {
    if webui.password_hash.is_none() {
        bail!("Daemon password has not been configured yet");
    }
    apply_password_config(config_path, webui, password)
}

pub fn verify_recovery_code(webui: &WebUIConfig, recovery_code: &str) -> Result<bool> {
    let Some(recovery_hash) = &webui.recovery_code_hash else {
        return Ok(false);
    };
    verify_secret_hash(recovery_hash, &normalize_recovery_code(recovery_code))
}

pub fn can_reset_with_device_secret(config_path: &Path, webui: &WebUIConfig) -> Result<bool> {
    if webui.password_hash.is_none() || webui.password_integrity.is_none() {
        return Ok(false);
    }
    Ok(existing_auth_secret(config_path)?.is_some())
}

pub fn password_protection_state(config: &Config) -> Result<PasswordProtectionState> {
    password_protection_state_for(&Config::config_path()?, &config.webui)
}

pub fn password_protection_state_for(
    config_path: &Path,
    webui: &WebUIConfig,
) -> Result<PasswordProtectionState> {
    match (&webui.password_hash, &webui.password_integrity) {
        (None, _) => Ok(PasswordProtectionState::Uninitialized),
        (Some(_), None) => Ok(PasswordProtectionState::LegacyUnsealed),
        (Some(password_hash), Some(integrity)) => {
            let Some(secret) = existing_auth_secret(config_path)? else {
                return Ok(PasswordProtectionState::Tampered);
            };
            let expected = integrity_for_secret(&secret, password_hash)?;
            if expected == *integrity {
                Ok(PasswordProtectionState::Sealed)
            } else {
                Ok(PasswordProtectionState::Tampered)
            }
        }
    }
}

pub fn describe_protection_state(state: PasswordProtectionState) -> &'static str {
    match state {
        PasswordProtectionState::Uninitialized => "uninitialized",
        PasswordProtectionState::LegacyUnsealed => "legacy-unsealed",
        PasswordProtectionState::Sealed => "device-sealed",
        PasswordProtectionState::Tampered => "tampered",
    }
}

fn apply_password_config(
    config_path: &Path,
    webui: &mut WebUIConfig,
    password: &str,
) -> Result<PasswordSetupArtifacts> {
    let password_hash = hash_password(password)?;
    let recovery_code = generate_recovery_code();
    let recovery_code_hash = hash_secret_value(&normalize_recovery_code(&recovery_code))?;
    let password_integrity = ensure_auth_secret(config_path)?
        .map(|secret| integrity_for_secret(&secret, &password_hash))
        .transpose()?;

    webui.password_hash = Some(password_hash);
    webui.password_integrity = password_integrity;
    webui.recovery_code_hash = Some(recovery_code_hash);

    Ok(PasswordSetupArtifacts {
        protection_state: if webui.password_integrity.is_some() {
            PasswordProtectionState::Sealed
        } else {
            PasswordProtectionState::LegacyUnsealed
        },
        recovery_code,
    })
}

fn hash_secret_value(value: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(value.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("failed to hash value: {error}"))?
        .to_string())
}

fn verify_secret_hash(secret_hash: &str, value: &str) -> Result<bool> {
    let parsed = PasswordHash::new(secret_hash)
        .map_err(|error| anyhow::anyhow!("invalid stored secret hash: {error}"))?;
    Ok(Argon2::default()
        .verify_password(value.as_bytes(), &parsed)
        .is_ok())
}

fn integrity_for_secret(secret: &str, password_hash: &str) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .context("failed to initialize password integrity hmac")?;
    mac.update(b"rove-auth-integrity-v1:");
    mac.update(password_hash.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn ensure_auth_secret(config_path: &Path) -> Result<Option<String>> {
    if let Some(secret) = existing_auth_secret(config_path)? {
        return Ok(Some(secret));
    }

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let secret = hex::encode(bytes);
    let key = keychain_secret_key(config_path);

    match keychain_set(&key, &secret) {
        Ok(()) => {
            cache_secret(&key, secret.clone());
            Ok(Some(secret))
        }
        Err(_) => Ok(None),
    }
}

fn existing_auth_secret(config_path: &Path) -> Result<Option<String>> {
    let key = keychain_secret_key(config_path);
    if let Some(cached) = cached_secret(&key) {
        return Ok(Some(cached));
    }

    match keychain_get(&key) {
        Ok(secret) => {
            cache_secret(&key, secret.clone());
            Ok(Some(secret))
        }
        Err(_) => Ok(None),
    }
}

fn keychain_secret_key(config_path: &Path) -> String {
    let digest = Sha256::digest(config_path.to_string_lossy().as_bytes());
    format!("{AUTH_KEYCHAIN_PREFIX}.{}", &hex::encode(digest)[..16])
}

fn generate_recovery_code() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    let hex = hex::encode_upper(bytes);
    let parts = hex
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>();
    format!("{RECOVERY_PREFIX}-{}", parts.join("-"))
}

fn normalize_recovery_code(code: &str) -> String {
    code.chars()
        .filter(|value| value.is_ascii_alphanumeric())
        .map(|value| value.to_ascii_uppercase())
        .collect()
}

fn cache_secret(key: &str, value: String) {
    keychain_cache()
        .lock()
        .expect("keychain cache poisoned")
        .insert(key.to_string(), value);
}

fn cached_secret(key: &str) -> Option<String> {
    keychain_cache()
        .lock()
        .expect("keychain cache poisoned")
        .get(key)
        .cloned()
}

fn keychain_cache() -> &'static Mutex<HashMap<String, String>> {
    KEYCHAIN_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(verify_password("correct horse battery staple", &hash).expect("verify"));
        assert!(!verify_password("wrong", &hash).expect("verify wrong"));
    }

    #[test]
    fn recovery_code_hash_roundtrip() {
        let recovery_code = generate_recovery_code();
        let normalized = normalize_recovery_code(&recovery_code);
        let hash = hash_secret_value(&normalized).expect("hash recovery");
        assert!(verify_secret_hash(&hash, &normalized).expect("verify recovery"));
        assert!(!verify_secret_hash(&hash, "RVEWRONG").expect("verify mismatch"));
    }

    #[test]
    fn integrity_detects_hash_change() {
        let integrity = integrity_for_secret("secret", "hash-a").expect("integrity");
        let changed = integrity_for_secret("secret", "hash-b").expect("changed");
        assert_ne!(integrity, changed);
    }
}
