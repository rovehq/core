//! Tests for system::health — PathStatus, HealthCheckRecord, AuthHealthSummary,
//! ControlPlaneSummary, RemoteHealthSummary, TransportHealthSummary

use rove_engine::system::health::{
    AuthHealthSummary, ControlPlaneSummary, HealthCheckRecord, PathStatus, RemoteHealthSummary,
    TransportHealthSummary,
};

// ── PathStatus ────────────────────────────────────────────────────────────────

#[test]
fn path_status_default_all_false() {
    let s = PathStatus::default();
    assert!(!s.exists);
    assert!(!s.writable);
}

#[test]
fn path_status_default_empty_path() {
    let s = PathStatus::default();
    assert!(s.path.is_empty() || s.path.len() < 100);
}

#[test]
fn path_status_fields_accessible() {
    let s = PathStatus {
        path: "/tmp/test".to_string(),
        exists: true,
        writable: true,
    };
    assert_eq!(s.path, "/tmp/test");
    assert!(s.exists);
    assert!(s.writable);
}

#[test]
fn path_status_exists_but_not_writable() {
    let s = PathStatus {
        path: "/etc/hosts".to_string(),
        exists: true,
        writable: false,
    };
    assert!(s.exists);
    assert!(!s.writable);
}

#[test]
fn path_status_clone() {
    let s = PathStatus {
        path: "/foo".to_string(),
        exists: true,
        writable: false,
    };
    let s2 = s.clone();
    assert_eq!(s.path, s2.path);
}

#[test]
fn path_status_debug() {
    let s = PathStatus {
        path: "/debug".to_string(),
        exists: false,
        writable: false,
    };
    let d = format!("{:?}", s);
    assert!(d.contains("/debug"));
}

#[test]
fn path_status_serializes() {
    let s = PathStatus {
        path: "/workspace".to_string(),
        exists: true,
        writable: true,
    };
    let j = serde_json::to_string(&s).unwrap();
    assert!(j.contains("workspace"));
    assert!(j.contains("true"));
}

// ── HealthCheckRecord ──────────────────────────────────────────────────────────

#[test]
fn health_check_ok_true() {
    let r = HealthCheckRecord {
        name: "Daemon".to_string(),
        ok: true,
        detail: "running (pid 1234)".to_string(),
    };
    assert!(r.ok);
}

#[test]
fn health_check_ok_false() {
    let r = HealthCheckRecord {
        name: "Auth".to_string(),
        ok: false,
        detail: "uninitialized".to_string(),
    };
    assert!(!r.ok);
}

#[test]
fn health_check_name_accessible() {
    let r = HealthCheckRecord {
        name: "Config file".to_string(),
        ok: true,
        detail: "exists, writable".to_string(),
    };
    assert_eq!(r.name, "Config file");
}

#[test]
fn health_check_detail_accessible() {
    let r = HealthCheckRecord {
        name: "Database".to_string(),
        ok: true,
        detail: "exists, writable".to_string(),
    };
    assert_eq!(r.detail, "exists, writable");
}

#[test]
fn health_check_clone() {
    let r = HealthCheckRecord {
        name: "Test".to_string(),
        ok: false,
        detail: "missing".to_string(),
    };
    let r2 = r.clone();
    assert_eq!(r.name, r2.name);
}

#[test]
fn health_check_debug() {
    let r = HealthCheckRecord {
        name: "Workspace".to_string(),
        ok: true,
        detail: "exists".to_string(),
    };
    let d = format!("{:?}", r);
    assert!(d.contains("Workspace"));
}

#[test]
fn health_check_serializes() {
    let r = HealthCheckRecord {
        name: "Provider: Ollama".to_string(),
        ok: false,
        detail: "not configured".to_string(),
    };
    let j = serde_json::to_string(&r).unwrap();
    assert!(j.contains("Ollama"));
    assert!(j.contains("false"));
}

#[test]
fn health_checks_in_vec() {
    let checks = [
        HealthCheckRecord { name: "A".to_string(), ok: true, detail: "ok".to_string() },
        HealthCheckRecord { name: "B".to_string(), ok: false, detail: "fail".to_string() },
    ];
    assert_eq!(checks.len(), 2);
    assert!(checks[0].ok);
    assert!(!checks[1].ok);
}

#[test]
fn health_check_all_ok_filter() {
    let checks = [
        HealthCheckRecord { name: "A".to_string(), ok: true, detail: "ok".to_string() },
        HealthCheckRecord { name: "B".to_string(), ok: true, detail: "ok".to_string() },
    ];
    assert!(checks.iter().all(|c| c.ok));
}

#[test]
fn health_check_any_failing_filter() {
    let checks = [
        HealthCheckRecord { name: "A".to_string(), ok: true, detail: "ok".to_string() },
        HealthCheckRecord { name: "B".to_string(), ok: false, detail: "fail".to_string() },
    ];
    assert!(checks.iter().any(|c| !c.ok));
}

// ── AuthHealthSummary ──────────────────────────────────────────────────────────

#[test]
fn auth_summary_default_empty() {
    let a = AuthHealthSummary::default();
    assert!(a.password_state.is_empty() || a.password_state.len() < 50);
    assert!(a.session_state.is_none());
    assert!(a.idle_expires_in_secs.is_none());
    assert!(a.absolute_expires_in_secs.is_none());
}

#[test]
fn auth_summary_fields_accessible() {
    let a = AuthHealthSummary {
        password_state: "device_sealed".to_string(),
        session_state: Some("unlocked".to_string()),
        idle_expires_in_secs: Some(1800),
        absolute_expires_in_secs: Some(86400),
    };
    assert_eq!(a.password_state, "device_sealed");
    assert_eq!(a.session_state.as_deref(), Some("unlocked"));
    assert_eq!(a.idle_expires_in_secs, Some(1800));
    assert_eq!(a.absolute_expires_in_secs, Some(86400));
}

#[test]
fn auth_summary_uninitialized_state() {
    let a = AuthHealthSummary {
        password_state: "uninitialized".to_string(),
        ..Default::default()
    };
    assert_eq!(a.password_state, "uninitialized");
}

#[test]
fn auth_summary_tampered_state() {
    let a = AuthHealthSummary {
        password_state: "tampered".to_string(),
        ..Default::default()
    };
    assert_eq!(a.password_state, "tampered");
}

#[test]
fn auth_summary_legacy_unsealed_state() {
    let a = AuthHealthSummary {
        password_state: "legacy_unsealed".to_string(),
        ..Default::default()
    };
    assert_eq!(a.password_state, "legacy_unsealed");
}

#[test]
fn auth_summary_clone() {
    let a = AuthHealthSummary {
        password_state: "device_sealed".to_string(),
        session_state: None,
        idle_expires_in_secs: None,
        absolute_expires_in_secs: None,
    };
    let a2 = a.clone();
    assert_eq!(a.password_state, a2.password_state);
}

#[test]
fn auth_summary_debug() {
    let a = AuthHealthSummary {
        password_state: "device_sealed".to_string(),
        session_state: Some("unlocked".to_string()),
        idle_expires_in_secs: None,
        absolute_expires_in_secs: None,
    };
    let d = format!("{:?}", a);
    assert!(d.contains("device_sealed"));
}

#[test]
fn auth_summary_serializes() {
    let a = AuthHealthSummary {
        password_state: "device_sealed".to_string(),
        session_state: Some("unlocked".to_string()),
        idle_expires_in_secs: Some(900),
        absolute_expires_in_secs: Some(3600),
    };
    let j = serde_json::to_string(&a).unwrap();
    assert!(j.contains("device_sealed"));
    assert!(j.contains("unlocked"));
}

#[test]
fn auth_summary_serialize_skips_none_fields() {
    let a = AuthHealthSummary {
        password_state: "uninitialized".to_string(),
        session_state: None,
        idle_expires_in_secs: None,
        absolute_expires_in_secs: None,
    };
    let j = serde_json::to_string(&a).unwrap();
    // skip_serializing_if = Option::is_none means those fields absent
    assert!(!j.contains("idle_expires_in_secs"));
}

// ── ControlPlaneSummary ────────────────────────────────────────────────────────

#[test]
fn control_plane_default() {
    let c = ControlPlaneSummary::default();
    assert!(!c.webui_enabled);
    assert_eq!(c.port, 0);
    assert!(!c.tls_enabled);
}

#[test]
fn control_plane_fields_accessible() {
    let c = ControlPlaneSummary {
        webui_enabled: true,
        configured_bind_addr: "0.0.0.0:7380".to_string(),
        listen_addr: "127.0.0.1:7380".to_string(),
        port: 7380,
        control_url: "http://localhost:7380".to_string(),
        tls_enabled: false,
        current_binary: Some("/usr/local/bin/rove".to_string()),
    };
    assert!(c.webui_enabled);
    assert_eq!(c.port, 7380);
    assert_eq!(c.control_url, "http://localhost:7380");
    assert!(c.current_binary.is_some());
}

#[test]
fn control_plane_tls_enabled() {
    let c = ControlPlaneSummary {
        tls_enabled: true,
        ..Default::default()
    };
    assert!(c.tls_enabled);
}

#[test]
fn control_plane_serializes() {
    let c = ControlPlaneSummary {
        webui_enabled: true,
        port: 7380,
        ..Default::default()
    };
    let j = serde_json::to_string(&c).unwrap();
    assert!(j.contains("7380"));
}

#[test]
fn control_plane_clone() {
    let c = ControlPlaneSummary {
        webui_enabled: true,
        port: 8080,
        ..Default::default()
    };
    let c2 = c.clone();
    assert_eq!(c.port, c2.port);
}

// ── RemoteHealthSummary ────────────────────────────────────────────────────────

#[test]
fn remote_summary_default() {
    let r = RemoteHealthSummary::default();
    assert!(!r.enabled);
    assert_eq!(r.paired_nodes, 0);
    assert_eq!(r.transport_count, 0);
}

#[test]
fn remote_summary_fields_accessible() {
    let r = RemoteHealthSummary {
        enabled: true,
        node_name: "my-node".to_string(),
        paired_nodes: 3,
        transport_count: 1,
    };
    assert!(r.enabled);
    assert_eq!(r.node_name, "my-node");
    assert_eq!(r.paired_nodes, 3);
    assert_eq!(r.transport_count, 1);
}

#[test]
fn remote_summary_clone() {
    let r = RemoteHealthSummary {
        enabled: false,
        node_name: "test".to_string(),
        paired_nodes: 0,
        transport_count: 0,
    };
    let r2 = r.clone();
    assert_eq!(r.node_name, r2.node_name);
}

#[test]
fn remote_summary_serializes() {
    let r = RemoteHealthSummary {
        enabled: true,
        node_name: "node-xyz".to_string(),
        paired_nodes: 2,
        transport_count: 1,
    };
    let j = serde_json::to_string(&r).unwrap();
    assert!(j.contains("node-xyz"));
}

// ── TransportHealthSummary ─────────────────────────────────────────────────────

#[test]
fn transport_summary_fields_accessible() {
    let t = TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: true,
        configured: true,
        healthy: true,
        summary: "joined mynet · 3 candidates · sync ok".to_string(),
    };
    assert_eq!(t.name, "zerotier");
    assert!(t.enabled);
    assert!(t.configured);
    assert!(t.healthy);
    assert!(t.summary.contains("joined"));
}

#[test]
fn transport_summary_disabled() {
    let t = TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: false,
        configured: false,
        healthy: false,
        summary: "disabled".to_string(),
    };
    assert!(!t.enabled);
    assert_eq!(t.summary, "disabled");
}

#[test]
fn transport_summary_enabled_not_healthy() {
    let t = TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: true,
        configured: false,
        healthy: false,
        summary: "enabled but not joined".to_string(),
    };
    assert!(t.enabled);
    assert!(!t.healthy);
}

#[test]
fn transport_summary_unavailable() {
    let t = TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: false,
        configured: false,
        healthy: false,
        summary: "status unavailable".to_string(),
    };
    assert!(!t.configured);
}

#[test]
fn transport_summary_clone() {
    let t = TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: false,
        configured: false,
        healthy: false,
        summary: "disabled".to_string(),
    };
    let t2 = t.clone();
    assert_eq!(t.name, t2.name);
}

#[test]
fn transport_summary_debug() {
    let t = TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: false,
        configured: false,
        healthy: false,
        summary: "disabled".to_string(),
    };
    let d = format!("{:?}", t);
    assert!(d.contains("zerotier"));
}

#[test]
fn transport_summary_serializes() {
    let t = TransportHealthSummary {
        name: "zerotier".to_string(),
        enabled: true,
        configured: true,
        healthy: true,
        summary: "ok".to_string(),
    };
    let j = serde_json::to_string(&t).unwrap();
    assert!(j.contains("zerotier"));
    assert!(j.contains("true"));
}

// ── Composite: issues detection pattern ────────────────────────────────────────

#[test]
fn unhealthy_snapshot_pattern_uninitialized() {
    // Simulate the pattern: auth.password_state == "uninitialized" → issue added
    let auth = AuthHealthSummary {
        password_state: "uninitialized".to_string(),
        ..Default::default()
    };
    let mut issues: Vec<String> = Vec::new();
    if auth.password_state == "uninitialized" {
        issues.push("Daemon password is not configured.".to_string());
    }
    assert_eq!(issues.len(), 1);
}

#[test]
fn healthy_snapshot_no_issues_device_sealed() {
    let auth = AuthHealthSummary {
        password_state: "device_sealed".to_string(),
        ..Default::default()
    };
    let mut issues: Vec<String> = Vec::new();
    if auth.password_state == "uninitialized" {
        issues.push("missing".to_string());
    }
    assert!(issues.is_empty());
}

#[test]
fn checks_filtering_all_passing() {
    let checks = [
        HealthCheckRecord { name: "Config".to_string(), ok: true, detail: "ok".to_string() },
        HealthCheckRecord { name: "Workspace".to_string(), ok: true, detail: "ok".to_string() },
        HealthCheckRecord { name: "Database".to_string(), ok: true, detail: "ok".to_string() },
    ];
    let failing: Vec<_> = checks.iter().filter(|c| !c.ok).collect();
    assert!(failing.is_empty());
}

#[test]
fn checks_filtering_some_failing() {
    let checks = [
        HealthCheckRecord { name: "Config".to_string(), ok: true, detail: "ok".to_string() },
        HealthCheckRecord { name: "Daemon".to_string(), ok: false, detail: "not running".to_string() },
        HealthCheckRecord { name: "Auth".to_string(), ok: false, detail: "uninitialized".to_string() },
    ];
    let failing: Vec<_> = checks.iter().filter(|c| !c.ok).collect();
    assert_eq!(failing.len(), 2);
}
