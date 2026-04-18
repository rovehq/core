use std::sync::Arc;

use anyhow::Result;
use brain::reasoning::LocalBrain;
use sdk::NodeExecutionRole;
use serde::Deserialize;

use crate::config::{metadata::SERVICE_NAME, Config};
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::custom::CustomLLMProvider;
use crate::llm::gemini::GeminiProvider;
use crate::llm::nvidia_nim::NvidiaNimProvider;
use crate::llm::ollama::OllamaProvider;
use crate::llm::openai::OpenAIProvider;
use crate::llm::LLMProvider;
use crate::security::secrets::{SecretCache, SecretManager};

pub async fn build(
    config: &Config,
) -> Result<(
    Vec<Box<dyn crate::llm::LLMProvider>>,
    Option<Arc<LocalBrain>>,
)> {
    build_for_execution_role(config, NodeExecutionRole::Full).await
}

pub async fn build_for_execution_role(
    config: &Config,
    execution_role: NodeExecutionRole,
) -> Result<(
    Vec<Box<dyn crate::llm::LLMProvider>>,
    Option<Arc<LocalBrain>>,
)> {
    if matches!(execution_role, NodeExecutionRole::ExecutorOnly) {
        tracing::info!(
            "Executor-only node profile detected; skipping provider discovery and local brain bootstrap"
        );
        return Ok((Vec::new(), None));
    }

    let secret_manager = Arc::new(SecretManager::new(SERVICE_NAME));
    let secret_cache = Arc::new(SecretCache::new(secret_manager.clone()));

    let mut api_keys = vec![
        "openai_api_key",
        "anthropic_api_key",
        "gemini_api_key",
        "nvidia_nim_api_key",
    ];
    // Also pre-warm keys for custom providers that have one configured.
    let custom_key_strings: Vec<String> = config
        .llm
        .custom_providers
        .iter()
        .filter(|p| !p.secret_key.is_empty())
        .map(|p| p.secret_key.clone())
        .collect();
    for k in &custom_key_strings {
        api_keys.push(k.as_str());
    }
    if let Err(error) = secret_cache.unlock(&api_keys).await {
        tracing::warn!("Failed to unlock some secrets: {}", error);
        tracing::warn!("LLM providers may prompt for credentials on first use");
    }

    let local_brain = detect_local_brain().await;
    let mut providers: Vec<Box<dyn crate::llm::LLMProvider>> = Vec::new();
    match OllamaProvider::new(
        config.llm.ollama.base_url.clone(),
        config.llm.ollama.model.clone(),
    ) {
        Ok(provider) => {
            if provider.check_health().await {
                providers.push(Box::new(provider));
            } else {
                tracing::warn!("Skipping Ollama provider: health check failed");
            }
        }
        Err(error) => tracing::warn!("Skipping Ollama provider: {}", error),
    }

    if secret_manager.has_secret("openai_api_key").await {
        providers.push(Box::new(OpenAIProvider::new(
            config.llm.openai.clone(),
            secret_cache.clone(),
        )));
    }

    if secret_manager.has_secret("anthropic_api_key").await {
        providers.push(Box::new(AnthropicProvider::new(
            config.llm.anthropic.clone(),
            secret_cache.clone(),
        )));
    }

    if secret_manager.has_secret("gemini_api_key").await {
        providers.push(Box::new(GeminiProvider::new(
            config.llm.gemini.clone(),
            secret_cache.clone(),
        )));
    }

    if secret_manager.has_secret("nvidia_nim_api_key").await {
        providers.push(Box::new(NvidiaNimProvider::new(
            config.llm.nvidia_nim.clone(),
            secret_cache.clone(),
        )));
    }

    for provider in &config.llm.custom_providers {
        // Keyless providers (e.g. local proxies) have an empty secret_key — always load them.
        // Key-gated providers are only skipped if the key is explicitly configured AND missing.
        if !provider.secret_key.is_empty()
            && !secret_manager.has_secret(&provider.secret_key).await
        {
            tracing::warn!(
                "Skipping custom provider '{}': secret '{}' not found in keychain. \
                 Run `rove secrets set {}` to add it.",
                provider.name,
                provider.secret_key,
                provider.name,
            );
            continue;
        }

        let api_key_name = if provider.secret_key.is_empty() {
            None
        } else {
            Some(provider.secret_key.clone())
        };

        providers.push(Box::new(CustomLLMProvider::new(
            provider.name.clone(),
            provider.protocol.clone(),
            provider.base_url.clone(),
            provider.model.clone(),
            api_key_name,
            secret_cache.clone(),
        )));

        tracing::info!(
            "Registered custom provider '{}' ({} @ {})",
            provider.name,
            provider.protocol,
            provider.base_url
        );
    }

    if providers.is_empty() && local_brain.is_none() {
        tracing::warn!(
            "No LLM providers are configured. Run `rove secrets set openai` or start the local brain."
        );
    }

    Ok((providers, local_brain))
}

async fn detect_local_brain() -> Option<Arc<LocalBrain>> {
    let metadata = read_local_brain_metadata();
    let port = metadata.as_ref().map(|meta| meta.port).unwrap_or(8080);
    let model = metadata
        .as_ref()
        .map(|meta| meta.model_name())
        .unwrap_or_else(|| "qwen2.5-coder-0.5b".to_string());
    let brain = Arc::new(LocalBrain::new(format!("http://localhost:{}", port), model));

    if brain.check_available().await {
        tracing::info!("LocalBrain (llama-server) detected and available");
        Some(brain)
    } else {
        tracing::debug!("LocalBrain not available (llama-server not running)");
        None
    }
}

#[derive(Debug, Deserialize)]
struct LocalBrainMetadata {
    model_path: String,
    port: u16,
}

impl LocalBrainMetadata {
    fn model_name(&self) -> String {
        std::path::Path::new(&self.model_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("qwen2.5-coder-0.5b")
            .to_string()
    }
}

fn read_local_brain_metadata() -> Option<LocalBrainMetadata> {
    let path = LocalBrain::default_brain_dir()?.join("llama-server.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::build_for_execution_role;

    #[tokio::test]
    async fn executor_only_profile_skips_provider_bootstrap() {
        let config = Config::default();
        let (providers, local_brain) =
            build_for_execution_role(&config, sdk::NodeExecutionRole::ExecutorOnly)
                .await
                .expect("build providers");

        assert!(providers.is_empty());
        assert!(local_brain.is_none());
    }
}
