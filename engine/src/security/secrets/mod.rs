pub mod cache;
pub mod string;

pub use cache::SecretCache;
pub use string::SecretString;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use keyring::Entry;
use rand_core::{OsRng, RngCore};
use regex::Regex;
use sdk::errors::EngineError;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::metadata::SERVICE_NAME;
use crate::config::{default_data_dir, Config, DaemonProfile, SecretBackend};

/// Regex patterns for detecting common secret formats.
static SECRET_PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();

fn get_secret_patterns() -> &'static Vec<Regex> {
    SECRET_PATTERNS.get_or_init(compile_secret_patterns)
}

fn compile_secret_patterns() -> Vec<Regex> {
    [
        ("OpenAI", r"sk-[a-zA-Z0-9\-_]{8,}"),
        ("Google", r"AIza[0-9A-Za-z\-_]{35}"),
        ("Telegram", r"\b[0-9]{10}:[a-zA-Z0-9\-_]{35}\b"),
        ("GitHub", r"ghp_[a-zA-Z0-9]{36}"),
        ("Bearer", r"Bearer\s+[^\s]{20,}"),
    ]
    .into_iter()
    .filter_map(|(name, pattern)| match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(error) => {
            tracing::error!("Invalid {} secret pattern: {}", name, error);
            None
        }
    })
    .collect()
}

pub fn scrub_text(text: &str) -> String {
    let patterns = get_secret_patterns();
    let mut result = text.to_string();
    for pattern in patterns {
        result = pattern.replace_all(&result, "[REDACTED]").to_string();
    }
    result
}

pub fn scrub_text_with_values(text: &str, secret_values: &[String]) -> String {
    let mut result = text.to_string();
    for value in secret_values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        result = result.replace(trimmed, "[REDACTED]");
    }
    scrub_text(&result)
}

/// Secret manager with configurable backends and a Rove-managed encrypted vault.
pub struct SecretManager {
    service_name: String,
    memory_store: RwLock<HashMap<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretSource {
    Env,
    Vault,
    Keychain,
    Memory,
    LegacyFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VaultFile {
    #[serde(default)]
    secrets: HashMap<String, VaultEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultEntry {
    nonce: String,
    ciphertext: String,
    created_at: i64,
    updated_at: i64,
}

impl SecretManager {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            memory_store: RwLock::new(HashMap::new()),
        }
    }

    pub fn configured_backend(&self) -> SecretBackend {
        if let Some(value) = std::env::var("ROVE_SECRET_BACKEND")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
        {
            match value.as_str() {
                "vault" => return SecretBackend::Vault,
                "keychain" => return SecretBackend::Keychain,
                "env" => return SecretBackend::Env,
                "auto" => return SecretBackend::Auto,
                _ => {}
            }
        }

        Config::load_or_create()
            .map(|config| config.secrets.backend)
            .unwrap_or_else(|_| {
                if self.service_name == SERVICE_NAME {
                    SecretBackend::Auto
                } else {
                    SecretBackend::Vault
                }
            })
    }

    pub fn scrub(&self, text: &str) -> String {
        scrub_text(text)
    }

    pub async fn get_secret(&self, key: &str) -> Result<String, EngineError> {
        if let Some((value, source)) = self.lookup_secret(key).await {
            if matches!(
                self.configured_backend(),
                SecretBackend::Auto | SecretBackend::Vault
            ) && !matches!(source, SecretSource::Vault | SecretSource::Memory)
            {
                let _ = self.write_vault_secret(key, &value);
            }

            tracing::debug!("Retrieved secret '{}' from {:?}", key, source);
            return Ok(value);
        }

        tracing::info!("Secret '{}' not found; prompting user.", key);
        let secret = self.prompt_for_secret(key)?;
        match self.set_secret(key, &secret).await {
            Ok(()) => Ok(secret),
            Err(error) if matches!(self.configured_backend(), SecretBackend::Env) => {
                tracing::warn!(
                    "Secret backend is env-only; returning prompted secret '{}' without persistence: {}",
                    key,
                    error
                );
                Ok(secret)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn set_secret(&self, key: &str, value: &str) -> Result<(), EngineError> {
        if value.trim().is_empty() {
            return Err(EngineError::KeyringError(
                "Secret value cannot be empty".to_string(),
            ));
        }

        match self.configured_backend() {
            SecretBackend::Auto | SecretBackend::Vault => self.write_vault_secret(key, value)?,
            SecretBackend::Keychain => self.write_keychain_secret(key, value)?,
            SecretBackend::Env => return Err(EngineError::KeyringError(
                "Configured secret backend 'env' is read-only; set environment variables instead"
                    .to_string(),
            )),
        }

        self.memory_store
            .write()
            .await
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub async fn delete_secret(&self, key: &str) -> Result<(), EngineError> {
        let backend = self.configured_backend();
        if matches!(backend, SecretBackend::Auto | SecretBackend::Vault) {
            let _ = self.delete_vault_secret(key);
        }
        if matches!(backend, SecretBackend::Auto | SecretBackend::Keychain) && !self.skip_keychain()
        {
            let _ = self.delete_keychain_secret(key);
        }

        let mut store = self.memory_store.write().await;
        store.remove(key);
        let _ = self.delete_legacy_fallback_secret(key);
        Ok(())
    }

    pub async fn has_secret(&self, key: &str) -> bool {
        self.lookup_secret(key).await.is_some()
    }

    pub(crate) async fn lookup_secret(&self, key: &str) -> Option<(String, SecretSource)> {
        for source in self.lookup_order() {
            let value = match source {
                SecretSource::Env => self.read_env_secret(key),
                SecretSource::Vault => self.read_vault_secret(key).ok().flatten(),
                SecretSource::Keychain => self.read_keychain_secret(key),
                SecretSource::Memory => self.memory_store.read().await.get(key).cloned(),
                SecretSource::LegacyFallback => {
                    self.read_legacy_fallback_secret(key).ok().flatten()
                }
            };
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                return Some((value, source));
            }
        }
        None
    }

    fn lookup_order(&self) -> Vec<SecretSource> {
        match self.configured_backend() {
            SecretBackend::Auto => vec![
                SecretSource::Env,
                SecretSource::Vault,
                SecretSource::Keychain,
                SecretSource::Memory,
                SecretSource::LegacyFallback,
            ],
            SecretBackend::Vault => vec![
                SecretSource::Vault,
                SecretSource::Env,
                SecretSource::Keychain,
                SecretSource::Memory,
                SecretSource::LegacyFallback,
            ],
            SecretBackend::Keychain => vec![
                SecretSource::Keychain,
                SecretSource::Vault,
                SecretSource::Env,
                SecretSource::Memory,
                SecretSource::LegacyFallback,
            ],
            SecretBackend::Env => vec![SecretSource::Env, SecretSource::Memory],
        }
    }

    fn prompt_for_secret(&self, key: &str) -> Result<String, EngineError> {
        let prompt = format!("Enter value for '{}': ", key);
        let secret = rpassword::read_password_from_tty(Some(&prompt))
            .map_err(|e| EngineError::KeyringError(format!("Failed to read input: {}", e)))?;

        let secret = secret.trim().to_string();
        if secret.is_empty() {
            return Err(EngineError::KeyringError(
                "Secret cannot be empty".to_string(),
            ));
        }
        Ok(secret)
    }

    fn env_key(&self, key: &str) -> String {
        format!(
            "{}_{}",
            self.service_name.to_uppercase().replace('-', "_"),
            key.to_uppercase()
        )
    }

    fn read_env_secret(&self, key: &str) -> Option<String> {
        for env_key in [self.env_key(key), key.to_uppercase()] {
            if let Ok(value) = std::env::var(&env_key) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    fn write_keychain_secret(&self, key: &str, value: &str) -> Result<(), EngineError> {
        if self.skip_keychain() {
            return self.write_vault_secret(key, value);
        }

        match Entry::new(&self.service_name, key) {
            Ok(entry) => entry
                .set_password(value)
                .map_err(|error| EngineError::KeyringError(error.to_string())),
            Err(error) => Err(EngineError::KeyringError(error.to_string())),
        }
    }

    fn read_keychain_secret(&self, key: &str) -> Option<String> {
        if self.skip_keychain() {
            return None;
        }

        match Entry::new(&self.service_name, key) {
            Ok(entry) => match entry.get_password() {
                Ok(value) => Some(value),
                Err(keyring::Error::NoEntry) => None,
                Err(error) => {
                    tracing::warn!("Failed to read secret '{}' from keychain: {}", key, error);
                    None
                }
            },
            Err(error) => {
                tracing::warn!("Failed to initialize keychain entry '{}': {}", key, error);
                None
            }
        }
    }

    fn delete_keychain_secret(&self, key: &str) -> Result<(), EngineError> {
        match Entry::new(&self.service_name, key) {
            Ok(entry) => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(EngineError::KeyringError(error.to_string())),
            },
            Err(error) => Err(EngineError::KeyringError(error.to_string())),
        }
    }

    fn skip_keychain(&self) -> bool {
        matches!(
            std::env::var("ROVE_SKIP_KEYCHAIN"),
            Ok(value) if value == "1" || value.eq_ignore_ascii_case("true")
        )
    }

    fn storage_root(&self) -> Result<PathBuf, EngineError> {
        let config = Config::load_or_create().unwrap_or_default();
        if matches!(
            config.daemon.profile,
            DaemonProfile::Headless | DaemonProfile::Edge
        ) && config.core.data_dir == default_data_dir()
        {
            return Ok(PathBuf::from("/var/lib/rove"));
        }

        if let Some(config_path) =
            std::env::var_os("ROVE_CONFIG_PATH").filter(|value| !value.is_empty())
        {
            let config_path = PathBuf::from(config_path);
            if let Some(parent) = config_path.parent() {
                return Ok(parent.to_path_buf());
            }
        }

        Ok(config
            .core
            .data_dir
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| config.core.data_dir.clone()))
    }

    fn vault_dir(&self) -> Result<PathBuf, EngineError> {
        Ok(self.storage_root()?.join("vault"))
    }

    fn vault_path(&self) -> Result<PathBuf, EngineError> {
        Ok(self.vault_dir()?.join("secrets.db"))
    }

    fn master_key_path(&self) -> Result<PathBuf, EngineError> {
        Ok(self.vault_dir()?.join("master.key"))
    }

    fn load_or_init_master_key(&self) -> Result<[u8; 32], EngineError> {
        let path = self.master_key_path()?;
        if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|error| {
                EngineError::Config(format!(
                    "Failed to read master key '{}': {}",
                    path.display(),
                    error
                ))
            })?;
            let bytes = hex::decode(raw.trim()).map_err(|error| {
                EngineError::Config(format!(
                    "Invalid master key '{}': {}",
                    path.display(),
                    error
                ))
            })?;
            return <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| {
                EngineError::Config(format!("Master key '{}' must be 32 bytes", path.display()))
            });
        }

        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        self.ensure_secure_parent(&path)?;
        fs::write(&path, hex::encode(key)).map_err(|error| {
            EngineError::Config(format!(
                "Failed to write master key '{}': {}",
                path.display(),
                error
            ))
        })?;
        self.lock_down_path(&path)?;
        Ok(key)
    }

    fn read_vault_secret(&self, key: &str) -> Result<Option<String>, EngineError> {
        let path = self.vault_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let file = self.read_vault_file()?;
        let Some(entry) = file.secrets.get(key) else {
            return Ok(None);
        };
        let master_key = self.load_or_init_master_key()?;
        let cipher = XChaCha20Poly1305::new((&master_key).into());
        let nonce_bytes = hex::decode(&entry.nonce).map_err(|error| {
            EngineError::Config(format!("Invalid vault nonce for '{}': {}", key, error))
        })?;
        let ciphertext = hex::decode(&entry.ciphertext).map_err(|error| {
            EngineError::Config(format!("Invalid vault ciphertext for '{}': {}", key, error))
        })?;
        let plaintext = cipher
            .decrypt(XNonce::from_slice(&nonce_bytes), ciphertext.as_ref())
            .map_err(|error| {
                EngineError::KeyringError(format!("Failed to decrypt secret '{}': {}", key, error))
            })?;
        String::from_utf8(plaintext).map(Some).map_err(|error| {
            EngineError::KeyringError(format!("Secret '{}' is not UTF-8: {}", key, error))
        })
    }

    fn write_vault_secret(&self, key: &str, value: &str) -> Result<(), EngineError> {
        let mut file = self.read_vault_file().unwrap_or_default();
        let master_key = self.load_or_init_master_key()?;
        let cipher = XChaCha20Poly1305::new((&master_key).into());

        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(XNonce::from_slice(&nonce), value.as_bytes())
            .map_err(|error| {
                EngineError::KeyringError(format!("Failed to encrypt secret '{}': {}", key, error))
            })?;

        let timestamp = now_ts();
        let created_at = file
            .secrets
            .get(key)
            .map(|entry| entry.created_at)
            .unwrap_or(timestamp);
        file.secrets.insert(
            key.to_string(),
            VaultEntry {
                nonce: hex::encode(nonce),
                ciphertext: hex::encode(ciphertext),
                created_at,
                updated_at: timestamp,
            },
        );
        self.write_vault_file(&file)
    }

    fn delete_vault_secret(&self, key: &str) -> Result<(), EngineError> {
        let path = self.vault_path()?;
        if !path.exists() {
            return Ok(());
        }

        let mut file = self.read_vault_file()?;
        file.secrets.remove(key);
        self.write_vault_file(&file)
    }

    fn read_vault_file(&self) -> Result<VaultFile, EngineError> {
        let path = self.vault_path()?;
        if !path.exists() {
            return Ok(VaultFile::default());
        }
        let raw = fs::read_to_string(&path).map_err(|error| {
            EngineError::Config(format!(
                "Failed to read vault '{}': {}",
                path.display(),
                error
            ))
        })?;
        toml::from_str(&raw).map_err(|error| {
            EngineError::Config(format!(
                "Failed to parse vault '{}': {}",
                path.display(),
                error
            ))
        })
    }

    fn write_vault_file(&self, file: &VaultFile) -> Result<(), EngineError> {
        let path = self.vault_path()?;
        self.ensure_secure_parent(&path)?;
        let serialized = toml::to_string_pretty(file).map_err(|error| {
            EngineError::Config(format!(
                "Failed to serialize vault '{}': {}",
                path.display(),
                error
            ))
        })?;
        fs::write(&path, serialized).map_err(|error| {
            EngineError::Config(format!(
                "Failed to write vault '{}': {}",
                path.display(),
                error
            ))
        })?;
        self.lock_down_path(&path)?;
        Ok(())
    }

    fn ensure_secure_parent(&self, path: &Path) -> Result<(), EngineError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                EngineError::Config(format!(
                    "Failed to create secure storage directory '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
            self.lock_down_dir(parent)?;
        }
        Ok(())
    }

    fn lock_down_path(&self, path: &Path) -> Result<(), EngineError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
                EngineError::Config(format!("Failed to chmod '{}': {}", path.display(), error))
            })?;
        }
        Ok(())
    }

    fn lock_down_dir(&self, path: &Path) -> Result<(), EngineError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
                EngineError::Config(format!("Failed to chmod '{}': {}", path.display(), error))
            })?;
        }
        Ok(())
    }

    fn legacy_fallback_store_path(&self) -> Result<PathBuf, EngineError> {
        Ok(self.storage_root()?.join("secrets.toml"))
    }

    fn read_legacy_fallback_secret(&self, key: &str) -> Result<Option<String>, EngineError> {
        let path = self.legacy_fallback_store_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            EngineError::Config(format!(
                "Failed to read legacy secrets file '{}': {}",
                path.display(),
                error
            ))
        })?;
        let secrets: HashMap<String, String> = toml::from_str(&contents).map_err(|error| {
            EngineError::Config(format!(
                "Failed to parse legacy secrets file '{}': {}",
                path.display(),
                error
            ))
        })?;
        Ok(secrets.get(key).cloned())
    }

    fn delete_legacy_fallback_secret(&self, key: &str) -> Result<(), EngineError> {
        let path = self.legacy_fallback_store_path()?;
        if !path.exists() {
            return Ok(());
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            EngineError::Config(format!(
                "Failed to read legacy secrets file '{}': {}",
                path.display(),
                error
            ))
        })?;
        let mut secrets: HashMap<String, String> = toml::from_str(&contents).unwrap_or_default();
        secrets.remove(key);
        if secrets.is_empty() {
            let _ = fs::remove_file(path);
            return Ok(());
        }

        let serialized = toml::to_string_pretty(&secrets).map_err(|error| {
            EngineError::Config(format!("Failed to serialize legacy secrets: {}", error))
        })?;
        fs::write(&path, serialized).map_err(|error| {
            EngineError::Config(format!("Failed to write legacy secrets: {}", error))
        })?;
        Ok(())
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn configure_temp_root(temp: &TempDir, backend: &str) -> PathBuf {
        fs::create_dir_all(temp.path().join("workspace")).expect("workspace dir");
        std::env::remove_var("ROVE_SECRET_BACKEND");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                r#"[core]
workspace = "{workspace}"
data_dir = "{data_dir}"

[secrets]
backend = "{backend}"
"#,
                workspace = temp.path().join("workspace").display(),
                data_dir = temp.path().join("data").display(),
                backend = backend,
            ),
        )
        .expect("config");
        std::env::set_var("ROVE_CONFIG_PATH", &config_path);
        config_path
    }

    #[test]
    fn test_scrub_openai_key() {
        let manager = SecretManager::new("test-service");
        let input = "key is sk-abcdefghijklmnopqrstuvwxyz1234567890";
        let result = manager.scrub(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("sk-"));
    }

    #[tokio::test]
    async fn vault_roundtrip_and_delete() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let config_path = configure_temp_root(&temp, "vault");
        let manager = SecretManager::new("rove");
        manager
            .set_secret("openai_api_key", "secret-123")
            .await
            .unwrap();
        assert!(manager.has_secret("openai_api_key").await);
        assert_eq!(
            manager.get_secret("openai_api_key").await.unwrap(),
            "secret-123"
        );
        manager.delete_secret("openai_api_key").await.unwrap();
        assert!(!manager.has_secret("openai_api_key").await);
        std::env::remove_var("ROVE_CONFIG_PATH");
        let _ = fs::remove_file(config_path);
    }

    #[tokio::test]
    async fn env_backend_is_read_only() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap();
        let temp = TempDir::new().expect("temp dir");
        let config_path = configure_temp_root(&temp, "env");
        std::env::set_var("ROVE_SECRET_BACKEND", "env");
        let manager = SecretManager::new("rove");
        let error = manager
            .set_secret("openai_api_key", "secret-123")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("read-only"));
        std::env::remove_var("ROVE_SECRET_BACKEND");
        std::env::remove_var("ROVE_CONFIG_PATH");
        let _ = fs::remove_file(config_path);
    }

    #[tokio::test]
    async fn empty_secret_rejected() {
        let manager = SecretManager::new("rove-test");
        let result = manager.set_secret("test_key", "").await;
        assert!(result.is_err());
    }
}
