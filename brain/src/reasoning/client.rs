use std::path::PathBuf;
use std::time::Duration;

use reqwest::Client;

/// Local reasoning brain using llama.cpp server.
///
/// Connects to a user-managed `llama-server` instance via HTTP.
pub struct LocalBrain {
    pub(super) client: Client,
    pub(super) base_url: String,
    pub(super) model_name: String,
}

impl LocalBrain {
    /// Create a new LocalBrain instance.
    pub fn new(base_url: impl Into<String>, model_name: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            base_url: base_url.into(),
            model_name: model_name.into(),
        }
    }

    /// Check if llama-server is available and responding.
    pub async fn check_available(&self) -> bool {
        let url = format!("{}/health", self.base_url.trim_end_matches('/'));

        match tokio::time::timeout(Duration::from_secs(2), self.client.get(&url).send()).await {
            Ok(Ok(response)) => response.status().is_success(),
            _ => false,
        }
    }

    /// Return the configured model name.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Get the default brain directory (`~/.rove/brains/reasoning/`).
    pub fn default_brain_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".rove/brains/reasoning"))
    }

    /// Check if a model is installed.
    pub fn is_model_installed(model_name: &str) -> bool {
        Self::default_brain_dir()
            .map(|brain_dir| brain_dir.join(format!("{}.gguf", model_name)).exists())
            .unwrap_or(false)
    }

    /// Get the path to the LoRA adapter.
    pub fn adapter_path() -> Option<PathBuf> {
        Self::default_brain_dir().map(|dir| dir.join("adapter.gguf"))
    }
}

#[cfg(test)]
mod tests {
    use super::LocalBrain;
    use sdk::Brain;

    #[test]
    fn test_local_brain_creation() {
        let brain = LocalBrain::new("http://localhost:8080", "qwen2.5-coder-0.5b");
        assert_eq!(brain.name(), "local-brain");
        assert_eq!(brain.model_name(), "qwen2.5-coder-0.5b");
    }

    #[test]
    fn test_default_brain_dir() {
        let dir = LocalBrain::default_brain_dir();
        assert!(dir.is_some());
        if let Some(path) = dir {
            assert!(path.to_string_lossy().contains(".rove/brains/reasoning"));
        }
    }

    #[test]
    fn test_adapter_path() {
        let path = LocalBrain::adapter_path();
        assert!(path.is_some());
        if let Some(path) = path {
            assert!(path.to_string_lossy().ends_with("adapter.gguf"));
        }
    }

    #[tokio::test]
    async fn test_check_available_when_not_running() {
        let brain = LocalBrain::new("http://localhost:9999", "test-model");
        assert!(!brain.check_available().await);
    }
}
