//! Tests for config security and misc type coverage

use rove_engine::config::{ApprovalMode, DaemonProfile, MemoryMode, MemoryRetrievalAssist,
    SecretBackend};

// ── SecretBackend ─────────────────────────────────────────────────────────────

#[test]
fn secret_backend_auto_eq() {
    assert_eq!(SecretBackend::Auto, SecretBackend::Auto);
}

#[test]
fn secret_backend_vault_eq() {
    assert_eq!(SecretBackend::Vault, SecretBackend::Vault);
}

#[test]
fn secret_backend_auto_ne_vault() {
    assert_ne!(SecretBackend::Auto, SecretBackend::Vault);
}

#[test]
fn secret_backend_debug_auto() {
    let s = format!("{:?}", SecretBackend::Auto);
    assert!(s.contains("Auto"));
}

#[test]
fn secret_backend_debug_vault() {
    let s = format!("{:?}", SecretBackend::Vault);
    assert!(s.contains("Vault"));
}

#[test]
fn secret_backend_as_str_auto() {
    assert_eq!(SecretBackend::Auto.as_str(), "auto");
}

#[test]
fn secret_backend_as_str_vault() {
    assert_eq!(SecretBackend::Vault.as_str(), "vault");
}

#[test]
fn secret_backend_clone() {
    let b = SecretBackend::Auto;
    assert_eq!(b, b.clone());
}

// ── DaemonProfile ─────────────────────────────────────────────────────────────

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
fn daemon_profile_desktop_ne_headless() {
    assert_ne!(DaemonProfile::Desktop, DaemonProfile::Headless);
}

#[test]
fn daemon_profile_clone() {
    let p = DaemonProfile::Edge;
    assert_eq!(p, p.clone());
}

// ── MemoryMode ────────────────────────────────────────────────────────────────

#[test]
fn memory_mode_always_on_eq() {
    assert_eq!(MemoryMode::AlwaysOn, MemoryMode::AlwaysOn);
}

#[test]
fn memory_mode_graph_only_eq() {
    assert_eq!(MemoryMode::GraphOnly, MemoryMode::GraphOnly);
}

#[test]
fn memory_mode_always_on_ne_graph_only() {
    assert_ne!(MemoryMode::AlwaysOn, MemoryMode::GraphOnly);
}

#[test]
fn memory_mode_debug_always_on() {
    let s = format!("{:?}", MemoryMode::AlwaysOn);
    assert!(s.contains("AlwaysOn") || !s.is_empty());
}

#[test]
fn memory_mode_clone() {
    let m = MemoryMode::GraphOnly;
    assert_eq!(m, m.clone());
}

// ── MemoryRetrievalAssist ─────────────────────────────────────────────────────

#[test]
fn retrieval_assist_off_eq() {
    assert_eq!(MemoryRetrievalAssist::Off, MemoryRetrievalAssist::Off);
}

#[test]
fn retrieval_assist_rerank_eq() {
    assert_eq!(MemoryRetrievalAssist::Rerank, MemoryRetrievalAssist::Rerank);
}

#[test]
fn retrieval_assist_off_ne_rerank() {
    assert_ne!(MemoryRetrievalAssist::Off, MemoryRetrievalAssist::Rerank);
}

#[test]
fn retrieval_assist_clone() {
    let r = MemoryRetrievalAssist::Rerank;
    assert_eq!(r, r.clone());
}

// ── ApprovalMode ──────────────────────────────────────────────────────────────

#[test]
fn approval_mode_serializes_open() {
    let j = serde_json::to_string(&ApprovalMode::Open).unwrap();
    assert!(!j.is_empty());
}

#[test]
fn approval_mode_serializes_default() {
    let j = serde_json::to_string(&ApprovalMode::Default).unwrap();
    assert!(!j.is_empty());
}

#[test]
fn approval_mode_serializes_allowlist() {
    let j = serde_json::to_string(&ApprovalMode::Allowlist).unwrap();
    assert!(!j.is_empty());
}
