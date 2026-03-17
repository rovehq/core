use std::sync::Arc;

use anyhow::{anyhow, Result};
use brain::reasoning::LocalBrain;

use crate::config::{metadata::SERVICE_NAME, AnthropicConfig, Config, GeminiConfig, OpenAIConfig};
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::gemini::GeminiProvider;
use crate::llm::nvidia_nim::NvidiaNimProvider;
use crate::llm::ollama::OllamaProvider;
use crate::llm::openai::OpenAIProvider;
use crate::security::secrets::{SecretCache, SecretManager};

pub async fn build(
    config: &Config,
) -> Result<(
    Vec<Box<dyn crate::llm::LLMProvider>>,
    Option<Arc<LocalBrain>>,
)> {
    let secret_manager = Arc::new(SecretManager::new(SERVICE_NAME));
    let secret_cache = Arc::new(SecretCache::new(secret_manager.clone()));

    let api_keys = vec![
        "openai_api_key",
        "anthropic_api_key",
        "gemini_api_key",
        "nvidia_nim_api_key",
    ];
    if let Err(error) = secret_cache.unlock(&api_keys).await {
        tracing::warn!("Failed to unlock some secrets: {}", error);
        tracing::warn!("LLM providers may prompt for credentials on first use");
    }

    let mut providers: Vec<Box<dyn crate::llm::LLMProvider>> = Vec::new();
    match OllamaProvider::new(
        config.llm.ollama.base_url.clone(),
        config.llm.ollama.model.clone(),
    ) {
        Ok(provider) => providers.push(Box::new(provider)),
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
        if !secret_manager.has_secret(&provider.secret_key).await {
            tracing::debug!(
                "Skipping custom provider '{}': no API key in keychain",
                provider.name
            );
            continue;
        }

        match provider.protocol.as_str() {
            "openai" => providers.push(Box::new(OpenAIProvider::new(
                OpenAIConfig {
                    base_url: provider.base_url.clone(),
                    model: provider.model.clone(),
                },
                secret_cache.clone(),
            ))),
            "gemini" => providers.push(Box::new(GeminiProvider::new(
                GeminiConfig {
                    base_url: provider.base_url.clone(),
                    model: provider.model.clone(),
                },
                secret_cache.clone(),
            ))),
            "anthropic" => providers.push(Box::new(AnthropicProvider::new(
                AnthropicConfig {
                    base_url: provider.base_url.clone(),
                    model: provider.model.clone(),
                },
                secret_cache.clone(),
            ))),
            other => tracing::warn!(
                "Unknown protocol '{}' for custom provider '{}'",
                other,
                provider.name
            ),
        }
    }

    if providers.is_empty() {
        return Err(anyhow!(
            "No LLM providers are available. Start Ollama or configure a provider API key."
        ));
    }

    let local_brain = detect_local_brain().await;
    Ok((providers, local_brain))
}

async fn detect_local_brain() -> Option<Arc<LocalBrain>> {
    let brain = Arc::new(LocalBrain::new(
        "http://localhost:8080",
        "qwen2.5-coder-0.5b",
    ));

    if brain.check_available().await {
        tracing::info!("LocalBrain (llama-server) detected and available");
        Some(brain)
    } else {
        tracing::debug!("LocalBrain not available (llama-server not running)");
        None
    }
}
