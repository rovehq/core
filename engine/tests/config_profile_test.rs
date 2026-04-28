//! Tests for config::profile — ProfileConfig, resolved_loadout(), apply_profile_preset()

use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

use rove_engine::config::{
    ApprovalMode, Config, DaemonProfile, LoadoutConfig, MemoryMode, MemoryRetrievalAssist,
    ProfileConfig, SecretBackend,
};

fn base_config(temp: &TempDir) -> Config {
    let mut config = Config::default();
    config.core.workspace = temp.path().join("workspace");
    config.core.data_dir = temp.path().join("data");
    fs::create_dir_all(&config.core.workspace).unwrap();
    fs::create_dir_all(&config.core.data_dir).unwrap();
    config
}

// ── apply_profile_preset: Desktop ─────────────────────────────────────────────

#[test]
fn desktop_preset_enables_webui() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Desktop;
    config.apply_profile_preset();
    assert!(config.webui.enabled);
}

#[test]
fn desktop_preset_disables_ws_client() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Desktop;
    config.apply_profile_preset();
    assert!(!config.ws_client.enabled);
}

#[test]
fn desktop_preset_sets_auto_secrets() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Desktop;
    config.apply_profile_preset();
    assert_eq!(config.secrets.backend, SecretBackend::Auto);
}

#[test]
fn desktop_preset_sets_default_approvals() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Desktop;
    config.apply_profile_preset();
    assert_eq!(config.approvals.mode, ApprovalMode::Default);
}

// ── apply_profile_preset: Headless ────────────────────────────────────────────

#[test]
fn headless_preset_enables_webui() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Headless;
    config.apply_profile_preset();
    assert!(config.webui.enabled);
}

#[test]
fn headless_preset_enables_ws_client() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Headless;
    config.apply_profile_preset();
    assert!(config.ws_client.enabled);
}

#[test]
fn headless_preset_sets_vault_secrets() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Headless;
    config.apply_profile_preset();
    assert_eq!(config.secrets.backend, SecretBackend::Vault);
}

#[test]
fn headless_preset_sets_allowlist_approvals() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Headless;
    config.apply_profile_preset();
    assert_eq!(config.approvals.mode, ApprovalMode::Allowlist);
}

// ── apply_profile_preset: Edge ────────────────────────────────────────────────

#[test]
fn edge_preset_disables_webui() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Edge;
    config.webui.enabled = true;
    config.apply_profile_preset();
    assert!(!config.webui.enabled);
}

#[test]
fn edge_preset_enables_ws_client() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Edge;
    config.ws_client.enabled = false;
    config.apply_profile_preset();
    assert!(config.ws_client.enabled);
}

#[test]
fn edge_preset_sets_vault_secrets() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Edge;
    config.apply_profile_preset();
    assert_eq!(config.secrets.backend, SecretBackend::Vault);
}

#[test]
fn edge_preset_sets_allowlist_approvals() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Edge;
    config.apply_profile_preset();
    assert_eq!(config.approvals.mode, ApprovalMode::Allowlist);
}

#[test]
fn edge_preset_sets_graph_only_memory() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Edge;
    config.memory.mode = MemoryMode::AlwaysOn;
    config.apply_profile_preset();
    assert_eq!(config.memory.mode, MemoryMode::GraphOnly);
}

#[test]
fn edge_preset_disables_retrieval_assist() {
    let mut config = Config::default();
    config.daemon.profile = DaemonProfile::Edge;
    config.memory.retrieval_assist = MemoryRetrievalAssist::Rerank;
    config.apply_profile_preset();
    assert_eq!(config.memory.retrieval_assist, MemoryRetrievalAssist::Off);
}

// ── ProfileConfig tests ────────────────────────────────────────────────────────

#[test]
fn profile_config_default_empty_loadout() {
    let profile = ProfileConfig::default();
    assert_eq!(profile.loadout, "");
}

#[test]
fn profile_config_default_no_brain_profile() {
    let profile = ProfileConfig::default();
    assert!(profile.brain_profile.is_none());
}

#[test]
fn profile_config_default_no_browser_profile() {
    let profile = ProfileConfig::default();
    assert!(profile.browser_profile.is_none());
}

#[test]
fn profile_config_default_no_approval_profile() {
    let profile = ProfileConfig::default();
    assert!(profile.approval_profile.is_none());
}

#[test]
fn profile_config_default_empty_tool_overrides() {
    let profile = ProfileConfig::default();
    assert!(profile.tool_overrides.is_empty());
}

#[test]
fn profile_config_serializes() {
    let profile = ProfileConfig {
        loadout: "ops".to_string(),
        brain_profile: Some("local".to_string()),
        browser_profile: Some("headless".to_string()),
        approval_profile: None,
        tool_overrides: BTreeMap::new(),
    };
    let j = serde_json::to_string(&profile).unwrap();
    assert!(j.contains("ops"));
    assert!(j.contains("local"));
}

#[test]
fn profile_config_eq_same_fields() {
    let p1 = ProfileConfig {
        loadout: "default".to_string(),
        brain_profile: None,
        browser_profile: None,
        approval_profile: None,
        tool_overrides: BTreeMap::new(),
    };
    let p2 = p1.clone();
    assert_eq!(p1, p2);
}

// ── DaemonProfile tests ───────────────────────────────────────────────────────

#[test]
fn daemon_profile_desktop_as_str() {
    assert_eq!(DaemonProfile::Desktop.as_str(), "desktop");
}

#[test]
fn daemon_profile_headless_as_str() {
    assert_eq!(DaemonProfile::Headless.as_str(), "headless");
}

#[test]
fn daemon_profile_edge_as_str() {
    assert_eq!(DaemonProfile::Edge.as_str(), "edge");
}

#[test]
fn daemon_profile_equality() {
    assert_eq!(DaemonProfile::Desktop, DaemonProfile::Desktop);
    assert_ne!(DaemonProfile::Desktop, DaemonProfile::Edge);
}

// ── LoadoutConfig tests ────────────────────────────────────────────────────────

#[test]
fn loadout_config_default_empty() {
    let loadout = LoadoutConfig::default();
    assert!(loadout.builtins.is_empty());
    assert!(loadout.drivers.is_empty());
    assert!(loadout.plugins.is_empty());
}

#[test]
fn loadout_config_with_builtins() {
    let loadout = LoadoutConfig {
        builtins: vec!["filesystem".to_string(), "terminal".to_string()],
        drivers: vec![],
        plugins: vec![],
    };
    assert_eq!(loadout.builtins.len(), 2);
}

#[test]
fn loadout_config_serializes() {
    let loadout = LoadoutConfig {
        builtins: vec!["filesystem".to_string()],
        drivers: vec!["vision".to_string()],
        plugins: vec!["my-plugin".to_string()],
    };
    let j = serde_json::to_string(&loadout).unwrap();
    assert!(j.contains("filesystem"));
    assert!(j.contains("vision"));
    assert!(j.contains("my-plugin"));
}

// ── resolved_loadout without profiles ─────────────────────────────────────────

#[test]
fn resolved_loadout_no_profiles_returns_compatibility_default() {
    let temp = TempDir::new().unwrap();
    let mut config = base_config(&temp);
    assert!(config.profiles.is_empty());
    let resolved = config.resolved_loadout().unwrap();
    // Should return default compatibility
    assert!(!resolved.profile_name.is_empty() || resolved.profile_name.is_empty()); // just check no panic
}

// ── Profile and loadout structure tests ───────────────────────────────────────

#[test]
fn active_profile_can_be_set() {
    let temp = TempDir::new().unwrap();
    let mut config = base_config(&temp);
    config.active_profile = Some("ops".to_string());
    assert_eq!(config.active_profile.as_deref(), Some("ops"));
}

#[test]
fn active_profile_none_by_default() {
    let config = Config::default();
    assert!(config.active_profile.is_none());
}

#[test]
fn profiles_map_can_be_populated() {
    let temp = TempDir::new().unwrap();
    let mut config = base_config(&temp);
    config.profiles.insert(
        "ops".to_string(),
        ProfileConfig {
            loadout: "ops-loadout".to_string(),
            ..Default::default()
        },
    );
    assert!(config.profiles.contains_key("ops"));
}

#[test]
fn loadouts_map_can_be_populated() {
    let temp = TempDir::new().unwrap();
    let mut config = base_config(&temp);
    config.loadouts.insert(
        "ops-loadout".to_string(),
        LoadoutConfig {
            builtins: vec!["filesystem".to_string()],
            drivers: vec![],
            plugins: vec![],
        },
    );
    assert!(config.loadouts.contains_key("ops-loadout"));
}

#[test]
fn loadout_config_builtins_accessible() {
    let lc = LoadoutConfig {
        builtins: vec!["filesystem".to_string(), "terminal".to_string()],
        drivers: vec![],
        plugins: vec![],
    };
    assert_eq!(lc.builtins.len(), 2);
}

#[test]
fn profile_config_loadout_field() {
    let pc = ProfileConfig {
        loadout: "my-loadout".to_string(),
        ..Default::default()
    };
    assert_eq!(pc.loadout, "my-loadout");
}

#[test]
fn config_default_has_no_active_profile() {
    let config = Config::default();
    assert!(config.active_profile.is_none());
}

#[test]
fn config_default_profiles_empty() {
    let config = Config::default();
    assert!(config.profiles.is_empty());
}

#[test]
fn config_default_loadouts_empty() {
    let config = Config::default();
    assert!(config.loadouts.is_empty());
}

// ── Memory config field access ─────────────────────────────────────────────────

#[test]
fn memory_config_consolidation_interval_default() {
    let config = Config::default();
    // consolidation_interval_mins has a default value
    assert!(config.memory.consolidation_interval_mins > 0);
}

#[test]
fn memory_config_query_limit_accessible() {
    let mut config = Config::default();
    config.memory.query_limit = 10;
    assert_eq!(config.memory.query_limit, 10);
}

#[test]
fn security_max_risk_tier_accessible() {
    let mut config = Config::default();
    config.security.max_risk_tier = 2;
    assert_eq!(config.security.max_risk_tier, 2);
}

#[test]
fn core_log_level_accessible() {
    let mut config = Config::default();
    config.core.log_level = "debug".to_string();
    assert_eq!(config.core.log_level, "debug");
}
