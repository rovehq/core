//! Default configuration values
//!
//! This module contains all default value functions used by the configuration system.
//! Default values are chosen to be safe and sensible for most users.
//!
//! # Clamping Rules
//!
//! Some values have clamp ranges that are enforced after parsing user config:
//! - `consolidation_interval_mins`: clamped to 5-1440 (5 min to 24 hours)
//! - `query_limit`: clamped to 1-20
//! - `min_importance_to_inject`: clamped to 0.1-0.9
//! - `min_to_consolidate`: minimum 1

use std::path::PathBuf;

use super::{ApprovalMode, DaemonProfile, SearchProviderKind, SecretBackend};

/// Default webui bind address
pub fn default_webui_bind_addr() -> String {
    format!("127.0.0.1:{}", crate::config::metadata::DEFAULT_PORT)
}

pub fn default_webui_idle_timeout_secs() -> u64 {
    20 * 60
}

pub fn default_webui_absolute_timeout_secs() -> u64 {
    12 * 60 * 60
}

pub fn default_webui_reauth_window_secs() -> u64 {
    10 * 60
}

pub fn default_webui_allowed_origins() -> Vec<String> {
    vec![
        "https://app.roveai.co".to_string(),
        "https://staging.roveai.co".to_string(),
        "http://localhost:3000".to_string(),
        "http://127.0.0.1:3000".to_string(),
    ]
}

pub fn default_privacy_mode() -> String {
    "local_only".to_string()
}

/// Default log level
pub fn default_log_level() -> String {
    "info".to_string()
}

/// Default true value
pub fn default_true() -> bool {
    true
}

/// Default false value
pub fn default_false() -> bool {
    false
}

pub fn default_daemon_profile() -> DaemonProfile {
    DaemonProfile::Desktop
}

pub fn default_approval_mode() -> ApprovalMode {
    ApprovalMode::Default
}

pub fn default_secret_backend() -> SecretBackend {
    SecretBackend::Auto
}

/// Default data directory
pub fn default_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("ROVE_DATA_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    super::paths::rove_home().join("data")
}

pub fn default_search_provider() -> SearchProviderKind {
    SearchProviderKind::Searxng
}

pub fn default_searxng_base_url() -> String {
    "http://127.0.0.1:8080".to_string()
}

pub fn default_search_timeout_secs() -> u64 {
    15
}

/// Default sensitivity threshold
pub fn default_sensitivity_threshold() -> f64 {
    0.7
}

/// Default complexity threshold
pub fn default_complexity_threshold() -> f64 {
    0.8
}

/// Default Ollama base URL
pub fn default_ollama_base_url() -> String {
    "http://localhost:11434".to_string()
}

/// Default OpenAI base URL
pub fn default_openai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

/// Default Anthropic base URL
pub fn default_anthropic_base_url() -> String {
    "https://api.anthropic.com/v1".to_string()
}

/// Default Gemini base URL
pub fn default_gemini_base_url() -> String {
    "https://generativelanguage.googleapis.com/v1beta".to_string()
}

/// Default Ollama model
pub fn default_ollama_model() -> String {
    "qwen2.5-coder:0.5b".to_string()
}

/// Default OpenAI model
pub fn default_openai_model() -> String {
    "gpt-4o-mini".to_string()
}

/// Default Anthropic model
pub fn default_anthropic_model() -> String {
    "claude-3-haiku-20240307".to_string()
}

/// Default Gemini model
pub fn default_gemini_model() -> String {
    "gemini-2.5-flash".to_string()
}

/// Default Nvidia NIM base URL
pub fn default_nvidia_nim_base_url() -> String {
    "https://integrate.api.nvidia.com/v1".to_string()
}

/// Default Nvidia NIM model
pub fn default_nvidia_nim_model() -> String {
    "meta/llama3-70b-instruct".to_string()
}

/// Default max risk tier
pub fn default_max_risk_tier() -> u8 {
    2
}

/// Default Tier 1 delay in seconds
pub fn default_tier1_delay() -> u64 {
    10
}

/// Default max iterations
pub fn default_max_iterations() -> u32 {
    20
}

/// Default confirm after iterations
pub fn default_confirm_after() -> u32 {
    10
}

/// Default RAM limit in MB
pub fn default_ram_limit() -> u64 {
    4096
}

/// Default fallback provider
pub fn default_fallback_provider() -> String {
    "ollama".to_string()
}

/// Default max session tokens
pub fn default_max_session_tokens() -> usize {
    4096
}

/// Default policy directory.
pub fn default_policy_dir() -> PathBuf {
    super::paths::rove_home().join("policy")
}

pub fn default_steering_dir() -> PathBuf {
    default_policy_dir()
}

/// Default episodic retention days
pub fn default_episodic_retention_days() -> u32 {
    30
}

/// Default consolidation interval in minutes
/// Clamped to 5-1440 (5 minutes to 24 hours)
pub fn default_consolidation_interval_mins() -> u64 {
    30
}

/// Default minimum memories to consolidate
/// Minimum value: 1
pub fn default_min_to_consolidate() -> usize {
    3
}

/// Default query limit
/// Clamped to 1-20
pub fn default_query_limit() -> u32 {
    5
}

/// Default minimum importance to inject
/// Clamped to 0.1-0.9
pub fn default_min_importance_to_inject() -> f32 {
    0.4
}

/// Default WebSocket URL
pub fn default_ws_url() -> String {
    "ws://localhost:8080/ws".to_string()
}

pub fn default_zerotier_service_url() -> String {
    "http://127.0.0.1:9993".to_string()
}

/// Default WebSocket reconnect delay
pub fn default_ws_reconnect_delay() -> u64 {
    5
}

/// Default gateway poll interval in milliseconds
pub fn default_gateway_poll_interval() -> u64 {
    200
}

/// Default gateway poll limit
pub fn default_gateway_poll_limit() -> i64 {
    10
}

/// Default WASM plugin memory cap in megabytes.
/// Matches ZeroClaw baseline; clamped to 1-1024 MB.
pub fn default_wasm_max_memory_mb() -> u32 {
    10
}

/// Default WASM plugin per-call timeout in seconds.
/// Clamped to 1-600s.
pub fn default_wasm_timeout_secs() -> u64 {
    60
}

/// Default WASM plugin fuel limit (instruction budget per call).
/// Minimum value: 1.
pub fn default_wasm_fuel_limit() -> u64 {
    50_000_000
}
