use crate::secrets::string::SecretString;
use crate::secrets::{SecretManager, SecretSource};
use sdk::errors::EngineError;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// An in-memory cache for secrets retrieved from the OS keychain.
///
/// This avoids hitting the OS keychain repeatedly during operations.
/// It works in tandem with `SecretManager`.
///
/// # One-Time Unlock Pattern
///
/// At daemon startup, call `unlock()` to load all secrets from the OS keychain
/// into memory. This requires user authentication (password/biometric) once.
/// After unlock, all secret access is from memory - no more prompts.
#[derive(Clone)]
pub struct SecretCache {
    manager: Arc<SecretManager>,
    cache: Arc<RwLock<HashMap<String, SecretString>>>,
    /// True if unlock() has been called successfully
    unlocked: Arc<RwLock<bool>>,
}

impl SecretCache {
    /// Creates a new SecretCache wrapping the provided SecretManager
    pub fn new(manager: Arc<SecretManager>) -> Self {
        Self {
            manager,
            cache: Arc::new(RwLock::new(HashMap::new())),
            unlocked: Arc::new(RwLock::new(false)),
        }
    }

    /// Checks if the cache has been unlocked
    pub async fn is_unlocked(&self) -> bool {
        *self.unlocked.read().await
    }

    /// Unlocks all secrets by loading them from the OS keychain.
    ///
    /// This should be called once at daemon startup. It will trigger
    /// OS keychain authentication (password/biometric) once, then cache
    /// all secrets in memory for the session lifetime.
    ///
    /// # Arguments
    ///
    /// * `keys` - List of secret keys to load (e.g., &["openai_api_key", "anthropic_api_key"])
    ///
    /// # Returns
    ///
    /// * `Ok(())` if all keys were loaded successfully
    /// * `Err` if any key failed to load (but partial loads are still cached)
    ///
    /// # Environment Variable Fallback
    ///
    /// For each key, checks environment variables first:
    /// - `{SERVICE_NAME}_{KEY}` (e.g., `ROVE_OPENAI_API_KEY`)
    /// - `{KEY}` (e.g., `OPENAI_API_KEY`)
    ///
    /// This allows users to use env vars instead of keychain.
    pub async fn unlock(&self, keys: &[&str]) -> Result<(), EngineError> {
        tracing::info!("Unlocking secret cache for {} keys...", keys.len());

        let mut cache = self.cache.write().await;
        let mut failures = Vec::new();
        let mut from_env = Vec::new();
        let mut from_keychain = Vec::new();
        let mut from_memory = Vec::new();

        for key in keys {
            let Some((secret_value, source)) = self.manager.lookup_secret(key).await else {
                failures.push(*key);
                continue;
            };

            match source {
                SecretSource::Env => from_env.push(*key),
                SecretSource::Keychain => from_keychain.push(*key),
                SecretSource::Memory => from_memory.push(*key),
            }

            cache.insert(key.to_string(), SecretString::new(secret_value));
        }

        drop(cache);
        *self.unlocked.write().await = true;

        if !from_env.is_empty() {
            tracing::info!(
                "Loaded {} secrets from environment: {:?}",
                from_env.len(),
                from_env
            );
        }
        if !from_keychain.is_empty() {
            tracing::info!(
                "Loaded {} secrets from keychain: {:?}",
                from_keychain.len(),
                from_keychain
            );
        }
        if !from_memory.is_empty() {
            tracing::info!(
                "Loaded {} secrets from memory: {:?}",
                from_memory.len(),
                from_memory
            );
        }

        if failures.is_empty() {
            tracing::info!("Secret cache unlocked successfully");
            Ok(())
        } else {
            tracing::info!(
                "Secret cache partially unlocked ({} unavailable: {:?})",
                failures.len(),
                failures
            );
            Ok(()) // Still consider success - user can add missing keys later
        }
    }

    /// Retrieves a secret. It checks the memory cache first.
    /// If not found and cache is unlocked, returns error (shouldn't happen).
    /// If cache is not unlocked, prompts via SecretManager and caches.
    pub async fn get_secret(&self, key: &str) -> Result<SecretString, EngineError> {
        // Read lock - check cache first
        {
            let cache = self.cache.read().await;
            if let Some(secret) = cache.get(key) {
                return Ok(secret.clone());
            }
        }

        // Cache miss
        let is_unlocked = *self.unlocked.read().await;

        if is_unlocked {
            // Cache should have all secrets - this is an error
            tracing::warn!("Secret '{}' not found in unlocked cache", key);
            return Err(EngineError::KeyringError(format!(
                "Secret '{}' not found - was it configured before unlock?",
                key
            )));
        }

        // Cache not unlocked yet - fall back to SecretManager (may prompt)
        let raw_secret = self.manager.get_secret(key).await?;
        let secret = SecretString::new(raw_secret);

        // Cache for future use
        {
            let mut cache = self.cache.write().await;
            cache.insert(key.to_string(), secret.clone());
        }

        Ok(secret)
    }

    /// Pre-loads a set of keys. This ensures any interactive prompts happen early.
    /// Deprecated: use `unlock()` instead for one-time keychain authentication.
    pub async fn preload(&self, keys: &[&str]) -> Result<(), EngineError> {
        self.unlock(keys).await
    }

    /// Clears all cached secrets (for logout/security)
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        *self.unlocked.write().await = false;
        tracing::info!("Secret cache cleared");
    }
}
