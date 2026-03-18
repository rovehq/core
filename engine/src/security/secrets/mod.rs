pub mod cache;
pub mod string;

pub use cache::SecretCache;
pub use string::SecretString;

use regex::Regex;
use sdk::errors::EngineError;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::RwLock;

/// Uses OS-level keychain storage.
/// Falls back to environment variables when keychain entry is not found.
/// If keychain is unavailable or headless (like CI), falls back to in-memory store.
pub struct SecretManager {
    service_name: String,
    /// In-memory fallback for headless/CI environments
    memory_store: RwLock<HashMap<String, String>>,
}

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

use keyring::Entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecretSource {
    Env,
    Keychain,
    Memory,
}

impl SecretManager {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            memory_store: RwLock::new(HashMap::new()),
        }
    }

    /// Get a secret — checks env vars first, then OS keychain
    pub async fn get_secret(&self, key: &str) -> Result<String, EngineError> {
        if let Some((value, source)) = self.lookup_secret(key).await {
            match source {
                SecretSource::Env => tracing::debug!("Retrieved secret '{}' from env var", key),
                SecretSource::Keychain => {
                    tracing::debug!("Retrieved secret '{}' from OS keychain", key)
                }
                SecretSource::Memory => {
                    tracing::debug!("Retrieved secret '{}' from in-memory store", key)
                }
            }
            return Ok(value);
        }

        // 4. Not found — prompt user
        tracing::info!(
            "Secret '{}' not found in env, keychain, or memory. Prompting user.",
            key
        );
        let secret = self.prompt_for_secret(key)?;

        // Save for future use
        self.set_secret(key, &secret).await?;
        Ok(secret)
    }

    /// Store a secret in the OS keychain (or in-memory fallback for CI/headless)
    pub async fn set_secret(&self, key: &str, value: &str) -> Result<(), EngineError> {
        if value.is_empty() {
            return Err(EngineError::KeyringError(
                "Secret value cannot be empty".to_string(),
            ));
        }

        if self.skip_keychain() {
            let mut store = self.memory_store.write().await;
            store.insert(key.to_string(), value.to_string());
            self.write_fallback_secret(key, value)?;
            tracing::info!(
                "Stored secret '{}' in local fallback store (keychain skipped)",
                key
            );
            return Ok(());
        }

        match Entry::new(&self.service_name, key) {
            Ok(entry) => {
                if let Err(e) = entry.set_password(value) {
                    tracing::warn!("Failed to store secret in OS keychain: {}", e);
                    tracing::warn!("(Storing in memory fallback)");
                    // Fall back to in-memory store
                    let mut store = self.memory_store.write().await;
                    store.insert(key.to_string(), value.to_string());
                } else {
                    tracing::info!("Stored secret '{}' in OS keychain", key);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to initialize keyring entry for {} (headless/CI): {}",
                    key,
                    e
                );
                // Fall back to in-memory store
                let mut store = self.memory_store.write().await;
                store.insert(key.to_string(), value.to_string());
            }
        }

        Ok(())
    }

    /// Delete a secret from the OS keychain (and in-memory fallback)
    pub async fn delete_secret(&self, key: &str) -> Result<(), EngineError> {
        if !self.skip_keychain() {
            match Entry::new(&self.service_name, key) {
                Ok(entry) => {
                    if let Err(e) = entry.delete_credential() {
                        if !matches!(e, keyring::Error::NoEntry) {
                            tracing::warn!(
                                "Failed to delete secret '{}' from keychain: {}",
                                key,
                                e
                            );
                        }
                    } else {
                        tracing::info!("Deleted secret '{}' from OS keychain", key);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize keyring entry for deletion: {}", e);
                }
            }
        }

        // Also delete from in-memory fallback
        let mut store = self.memory_store.write().await;
        store.remove(key);
        self.delete_fallback_secret(key)?;

        Ok(())
    }

    /// Check if a secret exists (env var, keychain, or in-memory fallback) — no I/O prompts
    pub async fn has_secret(&self, key: &str) -> bool {
        self.lookup_secret(key).await.is_some()
    }

    // ── Shared utilities ─────────────────────────────────────────────────

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

    pub fn scrub(&self, text: &str) -> String {
        scrub_text(text)
    }

    pub(crate) async fn lookup_secret(&self, key: &str) -> Option<(String, SecretSource)> {
        if let Some(value) = self.read_env_secret(key) {
            return Some((value, SecretSource::Env));
        }

        if !self.skip_keychain() {
            match Entry::new(&self.service_name, key) {
                Ok(entry) => match entry.get_password() {
                    Ok(value) => return Some((value, SecretSource::Keychain)),
                    Err(keyring::Error::NoEntry) => {
                        tracing::debug!("Secret '{}' not found in keychain", key);
                    }
                    Err(error) => {
                        tracing::warn!("Failed to read from OS keychain for '{}': {}", key, error);
                    }
                },
                Err(error) => {
                    tracing::warn!("Failed to initialize keyring entry: {}", error);
                }
            }
        }

        let store = self.memory_store.read().await;
        if let Some(value) = store
            .get(key)
            .cloned()
            .map(|value| (value, SecretSource::Memory))
        {
            return Some(value);
        }

        self.read_fallback_secret(key)
            .ok()
            .flatten()
            .map(|value| (value, SecretSource::Memory))
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

    fn skip_keychain(&self) -> bool {
        matches!(
            std::env::var("ROVE_SKIP_KEYCHAIN"),
            Ok(value) if value == "1" || value.eq_ignore_ascii_case("true")
        )
    }

    fn fallback_store_path(&self) -> Result<PathBuf, EngineError> {
        if let Some(config_path) =
            std::env::var_os("ROVE_CONFIG_PATH").filter(|value| !value.is_empty())
        {
            let config_path = PathBuf::from(config_path);
            let base = config_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            return Ok(base.join("secrets.toml"));
        }

        let home = dirs::home_dir()
            .ok_or_else(|| EngineError::Config("Could not determine home directory".to_string()))?;
        Ok(home.join(".rove").join("secrets.toml"))
    }

    fn read_fallback_secret(&self, key: &str) -> Result<Option<String>, EngineError> {
        let path = self.fallback_store_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            EngineError::Config(format!("Failed to read secrets file: {}", error))
        })?;
        let secrets: HashMap<String, String> = toml::from_str(&contents).map_err(|error| {
            EngineError::Config(format!("Failed to parse secrets file: {}", error))
        })?;

        Ok(secrets
            .get(key)
            .cloned()
            .filter(|value| !value.trim().is_empty()))
    }

    fn write_fallback_secret(&self, key: &str, value: &str) -> Result<(), EngineError> {
        let path = self.fallback_store_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                EngineError::Config(format!("Failed to create secrets directory: {}", error))
            })?;
        }

        let mut secrets = if path.exists() {
            let contents = fs::read_to_string(&path).map_err(|error| {
                EngineError::Config(format!("Failed to read secrets file: {}", error))
            })?;
            toml::from_str::<HashMap<String, String>>(&contents).unwrap_or_default()
        } else {
            HashMap::new()
        };

        secrets.insert(key.to_string(), value.to_string());
        let serialized = toml::to_string_pretty(&secrets).map_err(|error| {
            EngineError::Config(format!("Failed to serialize secrets: {}", error))
        })?;
        fs::write(&path, serialized).map_err(|error| {
            EngineError::Config(format!("Failed to write secrets file: {}", error))
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms).map_err(|error| {
                EngineError::Config(format!("Failed to set secrets file permissions: {}", error))
            })?;
        }

        Ok(())
    }

    fn delete_fallback_secret(&self, key: &str) -> Result<(), EngineError> {
        let path = self.fallback_store_path()?;
        if !path.exists() {
            return Ok(());
        }

        let contents = fs::read_to_string(&path).map_err(|error| {
            EngineError::Config(format!("Failed to read secrets file: {}", error))
        })?;
        let mut secrets: HashMap<String, String> = toml::from_str(&contents).unwrap_or_default();
        secrets.remove(key);

        if secrets.is_empty() {
            let _ = fs::remove_file(path);
            return Ok(());
        }

        let serialized = toml::to_string_pretty(&secrets).map_err(|error| {
            EngineError::Config(format!("Failed to serialize secrets: {}", error))
        })?;
        fs::write(&path, serialized).map_err(|error| {
            EngineError::Config(format!("Failed to write secrets file: {}", error))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_openai_key() {
        let manager = SecretManager::new("test-service");
        let input = "key is sk-abcdefghijklmnopqrstuvwxyz1234567890";
        let result = manager.scrub(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("sk-"));
    }

    #[tokio::test]
    #[ignore = "requires OS keychain — set ROVE_TEST_KEYCHAIN=1 to enable"]
    async fn test_set_and_get_secret() {
        // Use unique service name per test to avoid parallel test pollution
        let manager = SecretManager::new(format!("rove-test-set-{}", std::process::id()));
        let key = "test_vault_key";
        let value = "test_vault_value_12345";

        manager
            .set_secret(key, value)
            .await
            .expect("Failed to set secret");
        let retrieved = manager.get_secret(key).await.expect("Failed to get secret");
        assert_eq!(retrieved, value);

        // Cleanup
        let _ = manager.delete_secret(key).await;
    }

    #[tokio::test]
    #[ignore = "requires OS keychain — set ROVE_TEST_KEYCHAIN=1 to enable"]
    async fn test_delete_secret() {
        let manager = SecretManager::new(format!("rove-test-delete-{}", std::process::id()));
        let key = "test_delete_key";
        let value = "test_delete_value";

        manager.set_secret(key, value).await.unwrap();
        manager.delete_secret(key).await.unwrap();
        assert!(!manager.has_secret(key).await);
    }

    #[tokio::test]
    async fn test_empty_secret_rejected() {
        let manager = SecretManager::new("rove-test");
        let result = manager.set_secret("test_key", "").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_has_secret_returns_false_for_nonexistent() {
        let manager = SecretManager::new("test_has_secret");
        assert!(!manager.has_secret("nonexistent_key_xyz_123").await);
    }

    #[test]
    fn test_scrub_google_api_key() {
        let manager = SecretManager::new("test");
        let input = "key=AIzaSyD4RTtQ7BXpYW8wMVn14COK19b-ubZcPhw";
        let scrubbed = manager.scrub(input);
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_telegram_token() {
        let manager = SecretManager::new("test");
        let input = "token: 1234567890:ABCdefGHIjklMNOpqrsTUVwxyz123456789";
        let scrubbed = manager.scrub(input);
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_github_token() {
        let manager = SecretManager::new("test");
        let input = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        let scrubbed = manager.scrub(input);
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_bearer_token() {
        let manager = SecretManager::new("test");
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.test";
        let scrubbed = manager.scrub(input);
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_no_false_positive() {
        let manager = SecretManager::new("test");
        let input = "This is just a normal log message with no secrets";
        let scrubbed = manager.scrub(input);
        assert_eq!(input, scrubbed);
    }

    #[tokio::test]
    #[ignore = "requires OS keychain — set ROVE_TEST_KEYCHAIN=1 to enable"]
    async fn test_vault_roundtrip() {
        let manager = SecretManager::new(format!("test-vault-{}", std::process::id()));
        let key = "roundtrip_test_key";
        let value = "roundtrip_test_value";

        manager.set_secret(key, value).await.unwrap();
        assert!(manager.has_secret(key).await);

        let retrieved = manager.get_secret(key).await.unwrap();
        assert_eq!(retrieved, value);

        manager.delete_secret(key).await.unwrap();
        assert!(!manager.has_secret(key).await);
    }
}
