use serde::{Deserialize, Serialize};

use super::defaults::{
    default_anthropic_base_url, default_anthropic_model, default_complexity_threshold,
    default_gemini_base_url, default_gemini_model, default_nvidia_nim_base_url,
    default_nvidia_nim_model, default_ollama_base_url, default_ollama_model,
    default_openai_base_url, default_openai_model, default_sensitivity_threshold,
};

/// Custom LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvider {
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    /// Keychain entry name for the API key. Leave empty for keyless providers
    /// (e.g. local proxies that don't require authentication).
    #[serde(default)]
    pub secret_key: String,
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    /// Default LLM provider (`ollama`, `openai`, `anthropic`, `gemini`, `nvidia_nim`, or custom).
    pub default_provider: String,
    /// Sensitivity threshold for local provider preference (0.0-1.0).
    #[serde(default = "default_sensitivity_threshold")]
    pub sensitivity_threshold: f64,
    /// Complexity threshold for cloud provider preference (0.0-1.0).
    #[serde(default = "default_complexity_threshold")]
    pub complexity_threshold: f64,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub openai: OpenAIConfig,
    #[serde(default)]
    pub anthropic: AnthropicConfig,
    #[serde(default)]
    pub gemini: GeminiConfig,
    #[serde(default)]
    pub nvidia_nim: NvidiaNimConfig,
    #[serde(default)]
    pub custom_providers: Vec<CustomProvider>,
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            default_provider: "ollama".to_string(),
            sensitivity_threshold: default_sensitivity_threshold(),
            complexity_threshold: default_complexity_threshold(),
            ollama: OllamaConfig::default(),
            openai: OpenAIConfig::default(),
            anthropic: AnthropicConfig::default(),
            gemini: GeminiConfig::default(),
            nvidia_nim: NvidiaNimConfig::default(),
            custom_providers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_base_url")]
    pub base_url: String,
    #[serde(default = "default_ollama_model")]
    pub model: String,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: default_ollama_base_url(),
            model: default_ollama_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIConfig {
    #[serde(default = "default_openai_base_url")]
    pub base_url: String,
    #[serde(default = "default_openai_model")]
    pub model: String,
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            base_url: default_openai_base_url(),
            model: default_openai_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicConfig {
    #[serde(default = "default_anthropic_base_url")]
    pub base_url: String,
    #[serde(default = "default_anthropic_model")]
    pub model: String,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            base_url: default_anthropic_base_url(),
            model: default_anthropic_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiConfig {
    #[serde(default = "default_gemini_base_url")]
    pub base_url: String,
    #[serde(default = "default_gemini_model")]
    pub model: String,
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            base_url: default_gemini_base_url(),
            model: default_gemini_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvidiaNimConfig {
    #[serde(default = "default_nvidia_nim_base_url")]
    pub base_url: String,
    #[serde(default = "default_nvidia_nim_model")]
    pub model: String,
}

impl Default for NvidiaNimConfig {
    fn default() -> Self {
        Self {
            base_url: default_nvidia_nim_base_url(),
            model: default_nvidia_nim_model(),
        }
    }
}
