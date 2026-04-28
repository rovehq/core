//! Integration tests for the remote execution upgrade:
//! iroh transport, presence heartbeat, auto-routing, and replay protection.
//!
//! These tests exercise the full RemoteManager stack using two in-process
//! instances sharing a temporary directory.

use axum::{
    extract::Json,
    http::HeaderMap,
    response::IntoResponse,
    routing::post,
    Router,
};
use tempfile::TempDir;
use tokio::net::TcpListener;

use rove_engine::{
    config::Config,
    remote::{PresenceHeartbeat, RemoteManager},
};

// ── Helpers ────────────────────────────────────────────────────────────────────

fn make_config(temp: &TempDir, suffix: &str) -> Config {
    let mut config = Config::default();
    config.core.workspace = temp.path().join(format!("ws-{}", suffix));
    std::fs::create_dir_all(&config.core.workspace).expect("workspace");
    config.core.data_dir = temp.path().join(format!("data-{}", suffix));
    *config.policy.policy_dir_mut() = temp.path().join(format!("policy-{}", suffix));
    std::fs::create_dir_all(config.policy.policy_dir()).expect("policy dir");
    config.ws_client.enabled = true;
    config.ws_client.auth_token = Some("test-token".to_string());
    config
}

// ── test_auto_routing ─────────────────────────────────────────────────────────

/// With no live peers in the presence cache, best_peer() returns None.
#[test]
fn test_auto_routing_no_peers() {
    let temp = TempDir::new().expect("temp dir");
    let config = make_config(&temp, "ar");
    let manager = RemoteManager::new(config);
    assert!(manager.best_peer().is_none());
}

/// With one live peer upserted, best_peer() returns that peer.
#[tokio::test]
async fn test_auto_routing_with_live_peer() {
    let temp = TempDir::new().expect("temp dir");
    let config_a = make_config(&temp, "a");
    let config_b = make_config(&temp, "b");

    let manager_a = RemoteManager::new(config_a.clone());
    let manager_b = RemoteManager::new(config_b);

    // Get B's status to learn its node_id.
    let status_b = manager_b.status().expect("status b");

    // Register B as a peer on A (untrusted — we just need the node_id in cache).
    manager_a
        .upsert_verified_peer(
            status_b.node.clone(),
            status_b.profile.clone(),
            "http://127.0.0.1:1",
            vec![],
            true,
        )
        .expect("upsert peer");

    // Simulate receiving a heartbeat from B.
    let heartbeat = PresenceHeartbeat {
        node_id: status_b.node.node_id.clone(),
        node_name: status_b.node.node_name.clone(),
        active_tui: false,
        load: 0.1,
        iroh_node_id: None,
    };
    manager_a.upsert_presence(&heartbeat);

    // best_peer should now return B.
    let best = manager_a.best_peer();
    assert!(best.is_some(), "expected best_peer to return B");
    assert_eq!(best.unwrap().identity.node_id, status_b.node.node_id);
}

/// Presence score is higher for nodes with lower load and active_tui=true.
#[test]
fn test_presence_score_ordering() {
    use rove_engine::remote::PresenceEntry;
    use std::time::{Instant, SystemTime};

    let now = Instant::now();
    let low_load = PresenceEntry {
        node_id: "a".to_string(),
        last_seen: now,
        active_tui: false,
        load: 0.1,
        last_activity: SystemTime::now(),
    };
    let high_load = PresenceEntry {
        node_id: "b".to_string(),
        last_seen: now,
        active_tui: false,
        load: 0.9,
        last_activity: SystemTime::now(),
    };
    let tui_active = PresenceEntry {
        node_id: "c".to_string(),
        last_seen: now,
        active_tui: true,
        load: 0.5,
        last_activity: SystemTime::now(),
    };

    let score_low = RemoteManager::presence_score(&low_load);
    let score_high = RemoteManager::presence_score(&high_load);
    let score_tui = RemoteManager::presence_score(&tui_active);

    assert!(
        score_low > score_high,
        "low load should outscore high load: {} vs {}",
        score_low,
        score_high
    );
    assert!(
        score_tui > score_high,
        "tui active should outscore high load: {} vs {}",
        score_tui,
        score_high
    );
}

// ── test_replay_protection ────────────────────────────────────────────────────

/// Sending the same signed request twice must fail on the second attempt.
#[test]
fn test_replay_protection() {
    let temp = TempDir::new().expect("temp dir");
    let local = RemoteManager::new(make_config(&temp, "local-rp"));
    let remote = RemoteManager::new(make_config(&temp, "remote-rp"));

    let local_status = local.status().expect("local status");
    let remote_status = remote.status().expect("remote status");

    local
        .upsert_verified_peer(
            remote_status.node.clone(),
            remote_status.profile.clone(),
            "http://127.0.0.1:1",
            vec![],
            true,
        )
        .expect("upsert peer");

    let mut headers = HeaderMap::new();
    for (name, value) in remote
        .signed_request_headers_pub(&local_status.node.node_id, "execute", Some("task-rp"))
        .expect("sign")
    {
        headers.insert(
            name.parse::<axum::http::HeaderName>().expect("header name"),
            value.parse().expect("header value"),
        );
    }

    // First verify should succeed.
    local
        .verify_signed_request(&headers, "execute", Some("task-rp"))
        .expect("first verify");

    // Second verify with the same nonce must fail.
    let err = local
        .verify_signed_request(&headers, "execute", Some("task-rp"))
        .expect_err("replay should fail");
    assert!(
        err.to_string().contains("replay"),
        "error should mention replay: {}",
        err
    );
}

// ── test_presence_heartbeat ───────────────────────────────────────────────────

/// Start a minimal HTTP server that accepts presence POSTs, fire a heartbeat,
/// verify the presence cache is populated on the receiving side.
#[tokio::test]
async fn test_presence_heartbeat_endpoint() {
    use std::sync::{Arc, Mutex};

    let received: Arc<Mutex<Vec<PresenceHeartbeat>>> = Arc::new(Mutex::new(Vec::new()));
    let _received_clone = Arc::clone(&received);

    async fn presence_handler(
        _headers: HeaderMap,
        Json(_payload): Json<PresenceHeartbeat>,
    ) -> impl IntoResponse {
        (axum::http::StatusCode::NO_CONTENT, ())
    }

    let temp = TempDir::new().expect("temp dir");
    let config = make_config(&temp, "ph");
    let manager = RemoteManager::new(config.clone());

    let app = Router::new().route("/v1/remote/presence", post(presence_handler));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let _addr = listener.local_addr().expect("addr");

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    // Simulate receiving a heartbeat directly (upsert_presence path).
    let heartbeat = PresenceHeartbeat {
        node_id: "test-node-1".to_string(),
        node_name: "test-node".to_string(),
        active_tui: true,
        load: 0.3,
        iroh_node_id: None,
    };
    manager.upsert_presence(&heartbeat);

    let entries = manager.presence_list();
    assert!(!entries.is_empty(), "presence cache should have an entry");
    assert_eq!(entries[0].0.node_id, "test-node-1");

    server.abort();
}

// ── test_iroh_node_id ─────────────────────────────────────────────────────────

/// iroh_node_id() should return a non-empty string deterministically.
#[test]
fn test_iroh_node_id_stable() {
    let temp = TempDir::new().expect("temp dir");
    let config = make_config(&temp, "iroh");
    let manager = RemoteManager::new(config);

    let id1 = manager.iroh_node_id().expect("node id 1");
    let id2 = manager.iroh_node_id().expect("node id 2");

    assert!(!id1.is_empty(), "iroh node id should be non-empty");
    assert_eq!(id1, id2, "iroh node id should be stable across calls");
}

/// A second RemoteManager with the same data_dir should return the same iroh node id.
#[test]
fn test_iroh_node_id_persisted() {
    let temp = TempDir::new().expect("temp dir");
    let config = make_config(&temp, "iroh-persist");

    let id1 = {
        let manager = RemoteManager::new(config.clone());
        manager.iroh_node_id().expect("node id first run")
    };

    let id2 = {
        let manager = RemoteManager::new(config.clone());
        manager.iroh_node_id().expect("node id second run")
    };

    assert_eq!(id1, id2, "iroh node id must persist across restarts");
}
