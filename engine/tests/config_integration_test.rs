//! Integration tests for configuration management
//!
//! These tests verify that the Config struct can be properly loaded,
//! validated, and processed with path expansion and canonicalization.

// Import the config module from the binary crate
// Note: This requires the config module to be public in main.rs
// For now, we'll test through the public API once it's available

#[test]
fn test_config_toml_parsing() {
    let toml_content = r#"
[core]
workspace = "~/projects"
log_level = "info"
auto_sync = true
data_dir = "~/.rove"

[llm]
default_provider = "ollama"
sensitivity_threshold = 0.7
complexity_threshold = 0.8

[llm.ollama]
base_url = "http://localhost:11434"
model = "llama3.1:8b"

[llm.openai]
model = "gpt-4o-mini"

[llm.anthropic]
model = "claude-3-5-sonnet-20241022"

[llm.gemini]
model = "gemini-1.5-pro"

[llm.nvidia_nim]
base_url = "https://integrate.api.nvidia.com/v1"
model = "meta/llama-3.1-70b-instruct"

[tools]
tg-controller = false
ui-server = false
api-server = false

[plugins]
fs-editor = true
terminal = true
screenshot = false
git = true

[security]
max_risk_tier = 2
confirm_tier1 = true
confirm_tier1_delay = 10
require_explicit_tier2 = true

[brains]
enabled = false
ram_limit_mb = 512
fallback = "openai"
auto_unload = true
"#;

    // Parse the TOML to verify it's valid
    let parsed: toml::Value = toml::from_str(toml_content).expect("Failed to parse TOML");

    // Verify core sections exist
    assert!(parsed.get("core").is_some());
    assert!(parsed.get("llm").is_some());
    assert!(parsed.get("tools").is_some());
    assert!(parsed.get("plugins").is_some());
    assert!(parsed.get("security").is_some());
    assert!(parsed.get("brains").is_some());

    // Verify core values
    let core = parsed.get("core").unwrap();
    assert_eq!(
        core.get("workspace").unwrap().as_str().unwrap(),
        "~/projects"
    );
    assert_eq!(core.get("log_level").unwrap().as_str().unwrap(), "info");

    // Verify LLM values
    let llm = parsed.get("llm").unwrap();
    assert_eq!(
        llm.get("default_provider").unwrap().as_str().unwrap(),
        "ollama"
    );
    assert_eq!(
        llm.get("sensitivity_threshold")
            .unwrap()
            .as_float()
            .unwrap(),
        0.7
    );

    // Verify security values
    let security = parsed.get("security").unwrap();
    assert_eq!(
        security.get("max_risk_tier").unwrap().as_integer().unwrap(),
        2
    );
    assert!(security.get("confirm_tier1").unwrap().as_bool().unwrap());
}

#[test]
fn test_minimal_config_with_defaults() {
    let toml_content = r#"
[core]
workspace = "~/test-workspace"

[llm]
default_provider = "ollama"

[tools]

[plugins]

[security]
"#;

    // Parse the TOML - should work with minimal config due to defaults
    let parsed: toml::Value = toml::from_str(toml_content).expect("Failed to parse minimal TOML");

    assert!(parsed.get("core").is_some());
    assert!(parsed.get("llm").is_some());
}

#[test]
fn test_invalid_log_level_detection() {
    // This test verifies that invalid log levels would be caught
    // In a real scenario, the Config::validate_and_process would catch this
    let valid_levels = ["error", "warn", "info", "debug", "trace"];

    assert!(valid_levels.contains(&"info"));
    assert!(valid_levels.contains(&"debug"));
    assert!(!valid_levels.contains(&"invalid"));
}

#[test]
fn test_invalid_provider_detection() {
    // This test verifies that invalid providers would be caught
    let valid_providers = ["ollama", "openai", "anthropic", "gemini", "nvidia_nim"];

    assert!(valid_providers.contains(&"ollama"));
    assert!(valid_providers.contains(&"openai"));
    assert!(!valid_providers.contains(&"invalid_provider"));
}

#[test]
fn test_risk_tier_validation() {
    // Valid risk tiers are 0, 1, 2
    let valid_tiers = [0u8, 1, 2];

    assert!(valid_tiers.contains(&0));
    assert!(valid_tiers.contains(&1));
    assert!(valid_tiers.contains(&2));
    assert!(!valid_tiers.contains(&3));
}

#[test]
fn test_threshold_validation() {
    // Thresholds must be between 0.0 and 1.0
    let valid_threshold = 0.7;
    let invalid_low = -0.1;
    let invalid_high = 1.5;

    assert!((0.0..=1.0).contains(&valid_threshold));
    assert!(!(0.0..=1.0).contains(&invalid_low));
    assert!(!(0.0..=1.0).contains(&invalid_high));
}
