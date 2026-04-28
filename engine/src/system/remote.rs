use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Context, Result};
use axum::http::HeaderMap;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{HeaderName, HeaderValue, AUTHORIZATION};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use crate::config::metadata::SERVICE_NAME;
use crate::config::Config;
use crate::policy::{infer_domain, PolicyExplainReport, PolicyManager};
use crate::secrets::SecretManager;
use crate::system::identity::IdentityManager;
use sdk::{
    NodeExecutionRole, NodeIdentity, NodeLoadSnapshot, NodeProfile, RemoteEnvelope,
    RemoteExecutionPlan, RemoteTransportRecord,
};

/// ALPN protocol identifier for iroh QUIC streams carrying rove remote traffic.
pub const ROVE_ALPN: &[u8] = b"rove/1";

const HEADER_ORIGIN_NODE_ID: &str = "x-rove-origin-node-id";
const HEADER_TARGET_NODE_ID: &str = "x-rove-target-node-id";
const HEADER_REMOTE_PURPOSE: &str = "x-rove-remote-purpose";
const HEADER_REMOTE_NONCE: &str = "x-rove-remote-nonce";
const HEADER_REMOTE_TIMESTAMP: &str = "x-rove-remote-timestamp";
const HEADER_REMOTE_SIGNATURE: &str = "x-rove-remote-signature";
const REMOTE_SIGNATURE_TTL_SECS: u64 = 30;
const REMOTE_NONCE_TTL_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePeer {
    pub identity: NodeIdentity,
    pub profile: NodeProfile,
    pub target: String,
    pub trusted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load: Option<NodeLoadSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status_error: Option<String>,
    pub auth_secret_key: Option<String>,
    #[serde(default)]
    pub transports: Vec<RemoteTransportRecord>,
    /// iroh node id for QUIC/UDP-hole-punch transport (exchanged during pairing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iroh_node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteStatus {
    pub enabled: bool,
    pub node: NodeIdentity,
    pub profile: NodeProfile,
    pub paired_nodes: usize,
    pub load: Option<NodeLoadSnapshot>,
    #[serde(default)]
    pub transports: Vec<RemoteTransportRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteIdentityStatus {
    pub identity: NodeIdentity,
    pub profile: NodeProfile,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transports: Vec<RemoteTransportRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHandshakeProof {
    pub identity: NodeIdentity,
    pub profile: NodeProfile,
    pub signature: String,
    /// iroh node id advertised by the remote during handshake (optional, omitted by older peers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iroh_node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteSendPreview {
    pub envelope: RemoteEnvelope,
    pub trusted: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteSendResult {
    pub envelope: RemoteEnvelope,
    pub trusted: bool,
    pub status: String,
    pub remote_task_id: String,
    pub answer: Option<String>,
    pub provider: Option<String>,
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
    pub events: Vec<RemoteTaskEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteSendOptions {
    pub node: Option<String>,
    pub required_tags: Vec<String>,
    pub required_capabilities: Vec<String>,
    pub allow_executor_only: bool,
    pub prefer_executor_only: bool,
    pub execution_plan: Option<RemoteExecutionPlan>,
}

#[derive(Debug, Clone, Default)]
struct SelectionContext {
    workspace_name: Option<String>,
    domain_tag: Option<String>,
    policy_tags: Vec<String>,
    preferred_tools: Vec<String>,
    preferred_capabilities: Vec<String>,
    direct_executor_candidate: bool,
}

#[derive(Debug, Clone)]
struct PeerSelectionCandidate {
    peer: RemotePeer,
    load: Option<NodeLoadSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTaskEvent {
    pub task_id: String,
    pub event_type: String,
    pub payload: String,
    pub step_num: i64,
    pub domain: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct RemoteDriverSyncItem {
    pub id: String,
    pub source: String,
    pub registry: Option<String>,
    pub version: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDriverSyncResult {
    pub node_name: String,
    pub driver_id: String,
    pub action: String,
    pub version: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RemoteExtensionInventoryItem {
    id: String,
    kind: String,
    state: String,
    version: Option<String>,
}

/// Live presence record for a paired remote node.
#[derive(Debug, Clone)]
pub struct PresenceEntry {
    pub node_id: String,
    pub last_seen: Instant,
    pub active_tui: bool,
    pub load: f32,
    pub last_activity: SystemTime,
}

/// Heartbeat payload broadcast by each node every 30 s.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceHeartbeat {
    pub node_id: String,
    pub node_name: String,
    pub active_tui: bool,
    pub load: f32,
    #[serde(default)]
    pub iroh_node_id: Option<String>,
}

pub struct RemoteManager {
    config: Config,
    /// Iroh endpoint — lazy-initialised on first use.
    iroh_endpoint: OnceLock<iroh::Endpoint>,
    /// Iroh secret key — loaded/generated on first use.
    iroh_secret_key: OnceLock<iroh::SecretKey>,
}

/// Global presence cache shared across `RemoteManager` instances (one per request).
fn presence_cache() -> &'static DashMap<String, PresenceEntry> {
    static CACHE: OnceLock<DashMap<String, PresenceEntry>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

impl RemoteManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            iroh_endpoint: OnceLock::new(),
            iroh_secret_key: OnceLock::new(),
        }
    }

    // ── iroh helpers ─────────────────────────────────────────────────────────

    /// Return the iroh secret key, loading from disk or generating if absent.
    pub fn iroh_secret_key(&self) -> Result<&iroh::SecretKey> {
        if let Some(key) = self.iroh_secret_key.get() {
            return Ok(key);
        }
        let path = self.iroh_key_file();
        let key = if path.exists() {
            let bytes = fs::read(&path)
                .with_context(|| format!("Failed to read iroh key from {}", path.display()))?;
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
                anyhow::anyhow!(
                    "iroh key file {} has wrong length (expected 32 bytes)",
                    path.display()
                )
            })?;
            iroh::SecretKey::from(bytes)
        } else {
            let key = iroh::SecretKey::generate();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, key.to_bytes())?;
            key
        };
        // Ignore the error if another thread raced us.
        let _ = self.iroh_secret_key.set(key);
        Ok(self.iroh_secret_key.get().expect("just set"))
    }

    /// Return the local iroh node id (= public key derived from the secret key).
    pub fn iroh_node_id(&self) -> Result<String> {
        Ok(self.iroh_secret_key()?.public().to_string())
    }

    /// Lazy-initialise and return the shared iroh Endpoint.
    pub async fn init_iroh(&self) -> Result<&iroh::Endpoint> {
        if let Some(ep) = self.iroh_endpoint.get() {
            return Ok(ep);
        }
        let key = self.iroh_secret_key()?.clone();
        let cfg = &self.config.remote.transports.iroh;

        let mut builder = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .secret_key(key)
            .alpns(vec![ROVE_ALPN.to_vec()]);

        match cfg.relay_mode.as_str() {
            "disabled" => {
                builder = builder.relay_mode(iroh::RelayMode::Disabled);
            }
            "relay_only" | _ => {
                // "auto" or "relay_only" — use default public relay or a custom one
                if let Some(url) = &cfg.relay_url {
                    let relay_url: iroh::RelayUrl = url
                        .parse()
                        .with_context(|| format!("Invalid iroh relay URL: {}", url))?;
                    let relay_map = iroh::RelayMap::empty();
                    relay_map.insert(
                        relay_url.clone(),
                        std::sync::Arc::new(iroh::RelayConfig::new(relay_url, None)),
                    );
                    builder = builder.relay_mode(iroh::RelayMode::Custom(relay_map));
                }
            }
        }

        let endpoint = builder
            .bind()
            .await
            .context("Failed to bind iroh endpoint")?;

        let _ = self.iroh_endpoint.set(endpoint);
        Ok(self.iroh_endpoint.get().expect("just set"))
    }

    fn iroh_key_file(&self) -> PathBuf {
        self.config.core.data_dir.join("iroh_secret_key")
    }

    /// Attempt to deliver `request` to `peer` via an iroh QUIC bidirectional stream.
    ///
    /// The caller sends a newline-terminated JSON payload; the remote node reads it, processes
    /// it, and writes back a JSON `RemoteExecuteResponse`.  Falls back to `None` when the peer
    /// has no recorded iroh node id or the connection attempt fails.
    async fn send_via_iroh(
        &self,
        peer: &RemotePeer,
        request: &RemoteExecuteRequest,
        signed_headers: &[(String, String)],
    ) -> Option<RemoteExecuteResponse> {
        let iroh_node_id_str = peer.iroh_node_id.as_deref()?;

        let node_id: iroh::PublicKey = iroh_node_id_str.parse().ok()?;
        let endpoint = self.init_iroh().await.ok()?;

        let addr = iroh::EndpointAddr::from(node_id);
        let conn = endpoint
            .connect(addr, ROVE_ALPN)
            .await
            .inspect_err(|err| tracing::debug!(peer = %iroh_node_id_str, error = %err, "iroh connect failed"))
            .ok()?;

        let (mut send, mut recv) = conn.open_bi().await.ok()?;

        // Envelope: signed headers + request body, newline-delimited JSON.
        let envelope = serde_json::json!({
            "headers": signed_headers.iter().map(|(k, v)| serde_json::json!({"name": k, "value": v})).collect::<Vec<_>>(),
            "body": request,
        });
        let mut payload = serde_json::to_vec(&envelope).ok()?;
        payload.push(b'\n');
        send.write_all(&payload).await.ok()?;
        send.finish().ok()?;

        let raw = recv
            .read_to_end(4 * 1024 * 1024)
            .await
            .inspect_err(|err| tracing::debug!(error = %err, "iroh recv failed"))
            .ok()?;
        serde_json::from_slice(&raw)
            .inspect_err(|err| tracing::debug!(error = %err, "iroh response parse failed"))
            .ok()
    }

    // ── presence helpers ─────────────────────────────────────────────────────

    /// Score a presence entry for auto-routing (higher = prefer).
    pub fn presence_score(entry: &PresenceEntry) -> f32 {
        let age_secs = entry.last_seen.elapsed().as_secs_f32();
        let recency = (1.0 - (age_secs / 90.0).min(1.0)).max(0.0);
        let tui_bonus = if entry.active_tui { 0.2 } else { 0.0 };
        recency + tui_bonus + (1.0 - entry.load.clamp(0.0, 1.0))
    }

    /// Return the best-scored peer that was seen within the 90 s TTL.
    pub fn best_peer(&self) -> Option<RemotePeer> {
        let ttl = std::time::Duration::from_secs(90);
        let peers = self.load_peers().ok()?;
        presence_cache()
            .iter()
            .filter(|entry| entry.last_seen.elapsed() < ttl)
            .max_by(|a, b| {
                Self::presence_score(a.value())
                    .partial_cmp(&Self::presence_score(b.value()))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .and_then(|entry| {
                peers.into_iter().find(|peer| peer.identity.node_id == *entry.key())
            })
    }

    /// Spawn a background task that broadcasts a presence heartbeat every 30 s.
    /// The returned JoinHandle can be aborted to stop broadcasting.
    pub fn start_presence_broadcaster(config: Config) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let manager = RemoteManager::new(config);
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                manager.broadcast_presence_heartbeat().await;
            }
        })
    }

    async fn broadcast_presence_heartbeat(&self) {
        let peers = match self.load_peers() {
            Ok(peers) => peers,
            Err(_) => return,
        };
        let node_id = match self.load_or_init_node_metadata() {
            Ok(meta) => meta.identity.node_id,
            Err(_) => return,
        };
        let node_name = match self.load_or_init_node_metadata() {
            Ok(meta) => meta.identity.node_name,
            Err(_) => return,
        };
        let iroh_node_id = self.iroh_node_id().ok();
        let heartbeat = PresenceHeartbeat {
            node_id,
            node_name,
            active_tui: false,
            load: 0.0,
            iroh_node_id,
        };
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        for peer in peers.iter().filter(|p| p.trusted) {
            let signed_headers = match self
                .signed_request_headers(&peer.identity.node_id, "presence", None)
            {
                Ok(h) => h,
                Err(_) => continue,
            };
            let url = format!(
                "{}/v1/remote/presence",
                self.peer_endpoint(peer).trim_end_matches('/')
            );
            let mut req = client.post(&url).json(&heartbeat);
            for (name, value) in &signed_headers {
                req = req.header(name.as_str(), value.as_str());
            }
            if let Some(token) = self.optional_auth_token_for_peer(peer).await {
                req = req.bearer_auth(token);
            }
            // Ignore errors — unreachable peers are skipped silently.
            let _ = req.send().await;
        }
    }

    /// Upsert a received presence heartbeat into the local cache.
    pub fn upsert_presence(&self, heartbeat: &PresenceHeartbeat) {
        presence_cache().insert(
            heartbeat.node_id.clone(),
            PresenceEntry {
                node_id: heartbeat.node_id.clone(),
                last_seen: Instant::now(),
                active_tui: heartbeat.active_tui,
                load: heartbeat.load,
                last_activity: SystemTime::now(),
            },
        );
    }

    /// List all presence entries with their scores, sorted best-first.
    pub fn presence_list(&self) -> Vec<(PresenceEntry, f32)> {
        let ttl = std::time::Duration::from_secs(90);
        let mut entries: Vec<_> = presence_cache()
            .iter()
            .filter(|entry| entry.last_seen.elapsed() < ttl)
            .map(|entry| {
                let score = Self::presence_score(entry.value());
                (entry.value().clone(), score)
            })
            .collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
    }

    pub fn status(&self) -> Result<RemoteStatus> {
        let node = self.load_or_init_node_metadata()?;
        let peers = self.load_peers()?;
        Ok(RemoteStatus {
            enabled: self.config.ws_client.enabled,
            node: node.identity,
            profile: node.profile,
            paired_nodes: peers.len(),
            load: None,
            transports: Vec::new(),
        })
    }

    pub fn nodes(&self) -> Result<Vec<RemotePeer>> {
        self.load_peers()
    }

    pub async fn nodes_with_status(&self) -> Result<Vec<RemotePeer>> {
        let mut peers = self.load_peers()?;
        for peer in &mut peers {
            match self.fetch_peer_status(peer).await {
                Ok(status) => {
                    peer.load = status.load;
                    if !status.transports.is_empty() {
                        peer.transports = status.transports;
                    }
                    peer.last_status_error = None;
                }
                Err(error) => {
                    peer.last_status_error = Some(error.to_string());
                }
            }
        }
        Ok(peers)
    }

    pub fn rename(&self, name: &str) -> Result<NodeIdentity> {
        IdentityManager::new(self.config.clone()).rename(name)
    }

    pub fn local_profile(&self) -> Result<NodeProfile> {
        Ok(self.load_or_init_node_metadata()?.profile)
    }

    pub fn identity_status(&self) -> Result<RemoteIdentityStatus> {
        let metadata = self.load_or_init_node_metadata()?;
        Ok(RemoteIdentityStatus {
            identity: metadata.identity,
            profile: metadata.profile,
            transports: Vec::new(),
        })
    }

    pub fn sign_handshake(&self, challenge: &str) -> Result<RemoteHandshakeProof> {
        let metadata = self.load_or_init_node_metadata()?;
        let signature = IdentityManager::new(self.config.clone())
            .sign_message(&handshake_payload(challenge))?;
        let iroh_node_id = self.iroh_node_id().ok();
        Ok(RemoteHandshakeProof {
            identity: metadata.identity,
            profile: metadata.profile,
            signature,
            iroh_node_id,
        })
    }

    pub fn verify_handshake(challenge: &str, proof: &RemoteHandshakeProof) -> bool {
        IdentityManager::verify_message(
            &proof.identity.public_key,
            &handshake_payload(challenge),
            &proof.signature,
        )
    }

    pub fn set_execution_role(&self, execution_role: NodeExecutionRole) -> Result<NodeProfile> {
        let mut metadata = self.load_or_init_node_metadata()?;
        metadata.profile.execution_role = execution_role;
        self.save_node_profile(&metadata.profile)?;
        Ok(metadata.profile)
    }

    pub fn replace_tags(&self, tags: &[String]) -> Result<NodeProfile> {
        let mut metadata = self.load_or_init_node_metadata()?;
        metadata.profile.tags = tags.to_vec();
        self.save_node_profile(&metadata.profile)?;
        Ok(metadata.profile)
    }

    pub fn replace_capabilities(&self, capabilities: &[String]) -> Result<NodeProfile> {
        let mut metadata = self.load_or_init_node_metadata()?;
        metadata.profile.capabilities = normalize_capabilities(capabilities);
        self.save_node_profile(&metadata.profile)?;
        Ok(metadata.profile)
    }

    pub async fn sync_drivers(
        &self,
        node: Option<&str>,
        drivers: &[RemoteDriverSyncItem],
        dry_run: bool,
    ) -> Result<Vec<RemoteDriverSyncResult>> {
        let peers = self.load_peers()?;
        let targets = if let Some(node) = node {
            let Some(peer) = peers.iter().find(|peer| {
                peer.identity.node_name.eq_ignore_ascii_case(node)
                    || peer.target.eq_ignore_ascii_case(node)
                    || peer.identity.node_id == node
            }) else {
                bail!("Remote node '{}' is not paired", node);
            };
            vec![peer.clone()]
        } else {
            peers
                .into_iter()
                .filter(|peer| peer.trusted)
                .collect::<Vec<_>>()
        };

        if targets.is_empty() {
            bail!("No trusted remote nodes are paired yet");
        }

        let mut results = Vec::new();
        for peer in targets {
            results.extend(self.sync_drivers_to_peer(&peer, drivers, dry_run).await?);
        }
        Ok(results)
    }

    pub async fn pair(
        &self,
        target: &str,
        url: Option<&str>,
        token: Option<&str>,
        executor_only: bool,
        tags: &[String],
        capabilities: &[String],
    ) -> Result<RemotePeer> {
        let mut peers = self.load_peers()?;
        let (_, endpoint) = resolve_pair_target(target, url)?;
        let proof = Self::fetch_remote_handshake(&endpoint).await;
        let remote_status = self.fetch_pair_status(&endpoint).await;
        let node_name = proof
            .as_ref()
            .map(|proof| proof.identity.node_name.clone())
            .or_else(|_| {
                remote_status
                    .as_ref()
                    .map(|status| status.node.node_name.clone())
            })
            .unwrap_or_else(|_| target.to_string());

        if peers.iter().any(|peer| {
            peer.identity.node_name.eq_ignore_ascii_case(&node_name)
                || peer.target.eq_ignore_ascii_case(&endpoint)
                || proof
                    .as_ref()
                    .map(|value| peer.identity.node_id == value.identity.node_id)
                    .unwrap_or(false)
                || remote_status
                    .as_ref()
                    .map(|status| peer.identity.node_id == status.node.node_id)
                    .unwrap_or(false)
        }) {
            bail!("Remote node '{}' is already paired", node_name);
        }

        let auth_secret_key = if let Some(token) = token {
            let key = format!("remote_node_token_{}", secret_key_fragment(&node_name));
            SecretManager::new(SERVICE_NAME)
                .set_secret(&key, token)
                .await?;
            Some(key)
        } else {
            None
        };
        if proof.is_err() && remote_status.is_err() {
            if let Err(error) = &remote_status {
                tracing::warn!(
                    target = %endpoint,
                    error = %error,
                    "Falling back to compatibility pairing without remote status"
                );
            }
        }
        if let Err(error) = &proof {
            tracing::warn!(
                target = %endpoint,
                error = %error,
                "Remote handshake unavailable; pairing with compatibility discovery"
            );
        }

        let iroh_node_id = proof
            .as_ref()
            .ok()
            .and_then(|proof| proof.iroh_node_id.clone());

        let peer = RemotePeer {
            identity: proof
                .as_ref()
                .map(|proof| proof.identity.clone())
                .or_else(|_| remote_status.as_ref().map(|status| status.node.clone()))
                .unwrap_or_else(|_| NodeIdentity {
                    node_id: Uuid::new_v4().to_string(),
                    node_name: if node_name.trim().is_empty() {
                        derive_node_name(&endpoint)
                    } else {
                        node_name.clone()
                    },
                    public_key: Uuid::new_v4().simple().to_string(),
                }),
            profile: proof
                .as_ref()
                .map(|proof| {
                    merge_paired_profile(proof.profile.clone(), executor_only, tags, capabilities)
                })
                .or_else(|_| {
                    remote_status.as_ref().map(|status| {
                        merge_paired_profile(
                            status.profile.clone(),
                            executor_only,
                            tags,
                            capabilities,
                        )
                    })
                })
                .unwrap_or_else(|_| NodeProfile {
                    capabilities: normalize_capabilities(capabilities),
                    tags: tags.to_vec(),
                    execution_role: if executor_only {
                        NodeExecutionRole::ExecutorOnly
                    } else {
                        NodeExecutionRole::Full
                    },
                }),
            target: endpoint.clone(),
            trusted: false,
            load: remote_status
                .as_ref()
                .ok()
                .and_then(|status| status.load.clone()),
            last_status_error: remote_status.as_ref().err().map(|error| error.to_string()),
            auth_secret_key,
            transports: proof
                .as_ref()
                .map(|_| guess_transports_for_endpoint(&endpoint))
                .or_else(|_| {
                    remote_status
                        .as_ref()
                        .map(|status| status.transports.clone())
                })
                .unwrap_or_default(),
            iroh_node_id,
        };
        peers.push(peer.clone());
        self.save_peers(&peers)?;
        Ok(peer)
    }

    pub fn upsert_verified_peer(
        &self,
        identity: NodeIdentity,
        profile: NodeProfile,
        target: &str,
        transports: Vec<RemoteTransportRecord>,
        auto_trust: bool,
    ) -> Result<RemotePeer> {
        let mut peers = self.load_peers()?;
        if let Some(conflict) = peers.iter().find(|peer| {
            peer.trusted
                && peer
                    .identity
                    .node_name
                    .eq_ignore_ascii_case(&identity.node_name)
                && peer.identity.node_id != identity.node_id
        }) {
            bail!(
                "Refusing to auto-trust '{}': node name already belongs to trusted node '{}'",
                identity.node_name,
                conflict.identity.node_id
            );
        }

        if let Some(existing) = peers.iter_mut().find(|peer| {
            peer.identity.node_id == identity.node_id
                || peer.identity.public_key == identity.public_key
                || peer.target.eq_ignore_ascii_case(target)
        }) {
            existing.identity = identity.clone();
            existing.profile = profile;
            existing.target = target.to_string();
            existing.transports = transports;
            existing.trusted |= auto_trust;
            let updated = existing.clone();
            self.save_peers(&peers)?;
            return Ok(updated);
        }

        let peer = RemotePeer {
            identity,
            profile,
            target: target.to_string(),
            trusted: auto_trust,
            load: None,
            last_status_error: None,
            auth_secret_key: None,
            transports,
            iroh_node_id: None,
        };
        peers.push(peer.clone());
        self.save_peers(&peers)?;
        Ok(peer)
    }

    pub async fn unpair(&self, name: &str) -> Result<()> {
        let mut peers = self.load_peers()?;
        let mut removed_secret = None;
        let original_len = peers.len();
        peers.retain(|peer| {
            let matched = peer.identity.node_name == name
                || peer.target == name
                || peer.identity.node_id == name;
            if matched {
                removed_secret = peer.auth_secret_key.clone();
            }
            !matched
        });
        if peers.len() == original_len {
            bail!("Remote node '{}' is not paired", name);
        }
        self.save_peers(&peers)?;
        if let Some(secret_key) = removed_secret {
            let _ = SecretManager::new(SERVICE_NAME)
                .delete_secret(&secret_key)
                .await;
        }
        Ok(())
    }

    pub fn trust(&self, name: &str) -> Result<RemotePeer> {
        let mut peers = self.load_peers()?;
        let mut trusted = None;
        for peer in &mut peers {
            if peer.identity.node_name == name
                || peer.target == name
                || peer.identity.node_id == name
            {
                peer.trusted = true;
                trusted = Some(peer.clone());
            }
        }
        let Some(peer) = trusted else {
            bail!("Remote node '{}' is not paired", name);
        };
        self.save_peers(&peers)?;
        Ok(peer)
    }

    pub fn send_preview(&self, node: &str, prompt: &str) -> Result<RemoteSendPreview> {
        if !self.config.ws_client.enabled {
            bail!("Remote service is disabled. Run `rove service enable remote` first.");
        }

        let peers = self.load_peers()?;
        let selection = self.derive_selection_context(prompt);
        let peer = self.resolve_peer_preview(
            &peers,
            prompt,
            &selection,
            &RemoteSendOptions {
                node: Some(node.to_string()),
                ..RemoteSendOptions::default()
            },
        )?;
        let execution_plan =
            if matches!(peer.profile.execution_role, NodeExecutionRole::ExecutorOnly) {
                self.build_executor_plan(prompt, &selection)?
            } else {
                None
            };
        let has_execution_plan = execution_plan.is_some();
        let coordinator = self.load_or_init_node_metadata()?;
        let envelope = RemoteEnvelope {
            origin_node: coordinator.identity.node_name.clone(),
            target_node: peer.identity.node_name.clone(),
            coordinator_node: coordinator.identity.node_name.clone(),
            task_id: Uuid::new_v4().to_string(),
            task_input: prompt.to_string(),
            stream_policy: "events+result".to_string(),
            execution_plan,
        };

        Ok(RemoteSendPreview {
            envelope,
            trusted: peer.trusted,
            message: if matches!(peer.profile.execution_role, NodeExecutionRole::ExecutorOnly)
                && has_execution_plan
            {
                "Remote transport ready. The coordinator will attach a direct execution plan for the executor-only target and stream task events before the final result.".to_string()
            } else {
                "Remote transport ready. The coordinator will submit to the target daemon and stream task events before the final result.".to_string()
            },
        })
    }

    pub async fn send(&self, node: &str, prompt: &str) -> Result<RemoteSendResult> {
        self.send_with_options(
            prompt,
            RemoteSendOptions {
                node: Some(node.to_string()),
                ..RemoteSendOptions::default()
            },
        )
        .await
    }

    pub async fn send_with_options(
        &self,
        prompt: &str,
        options: RemoteSendOptions,
    ) -> Result<RemoteSendResult> {
        if !self.config.ws_client.enabled {
            bail!("Remote service is disabled. Run `rove service enable remote` first.");
        }

        let peers = self.load_peers()?;
        let mut selection = self.derive_selection_context(prompt);
        self.enrich_selection_context_with_policy(prompt, &mut selection)
            .await;
        let peer = self
            .resolve_peer(&peers, prompt, &selection, &options)
            .await?;
        if !peer.trusted {
            bail!(
                "Remote node '{}' is paired but not trusted. Run `rove remote trust {}` first.",
                peer.identity.node_name,
                peer.identity.node_name
            );
        }
        let execution_plan = match options.execution_plan.clone() {
            Some(plan) => Some(plan),
            None if matches!(peer.profile.execution_role, NodeExecutionRole::ExecutorOnly) => {
                Some(self.build_executor_plan(prompt, &selection)?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Remote node '{}' is executor-only, but this task could not be decomposed into a safe direct execution plan. Choose a full node or use an explicit read-only system action.",
                        peer.identity.node_name
                    )
                })?)
            }
            None => None,
        };

        let coordinator = self.load_or_init_node_metadata()?;
        let envelope = RemoteEnvelope {
            origin_node: coordinator.identity.node_name.clone(),
            target_node: peer.identity.node_name.clone(),
            coordinator_node: coordinator.identity.node_name.clone(),
            task_id: Uuid::new_v4().to_string(),
            task_input: prompt.to_string(),
            stream_policy: "events+result".to_string(),
            execution_plan: execution_plan.clone(),
        };

        let signed_headers = self.signed_request_headers(
            &peer.identity.node_id,
            "execute",
            Some(&envelope.task_id),
        )?;
        let client = Client::new();
        let auth_token = self.optional_auth_token_for_peer(&peer).await;
        let request = RemoteExecuteRequest {
            task_id: Some(envelope.task_id.clone()),
            input: Some(prompt.to_string()),
            task: None,
            origin_node: Some(envelope.origin_node.clone()),
            coordinator_node: Some(envelope.coordinator_node.clone()),
            workspace: None,
            team_id: None,
            wait_seconds: Some(1),
            plan: execution_plan,
        };

        // Prefer iroh QUIC when the peer advertises an iroh node id; fall back to HTTP.
        let execute = if let Some(iroh_response) = self
            .send_via_iroh(&peer, &request, &signed_headers)
            .await
        {
            iroh_response
        } else {
            let http_endpoint = format!(
                "{}/v1/remote/execute",
                self.peer_endpoint(&peer).trim_end_matches('/')
            );
            let mut execute = client.post(http_endpoint);
            if let Some(token) = auth_token.as_deref() {
                execute = execute.bearer_auth(token);
            }
            for (name, value) in &signed_headers {
                execute = execute.header(name, value);
            }
            parse_remote_response(
                execute
                    .json(&request)
                    .send()
                    .await
                    .context("Failed to reach remote daemon")?,
            )
            .await?
        };
        if execute.status == "completed" {
            return Ok(RemoteSendResult {
                envelope,
                trusted: peer.trusted,
                status: execute.status,
                remote_task_id: execute.task_id.unwrap_or_else(|| "unknown".to_string()),
                answer: execute.answer,
                provider: execute.provider,
                duration_ms: execute.duration_ms,
                message: execute.message,
                events: Vec::new(),
            });
        }

        let remote_task_id = execute
            .task_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Remote daemon did not return a task id"))?;

        let (events, completion) = match self
            .stream_remote_events(&peer, auth_token.as_deref(), &remote_task_id)
            .await
        {
            Ok((events, completion)) => (events, completion),
            Err(error) => {
                tracing::warn!(
                    node = %peer.identity.node_name,
                    task_id = %remote_task_id,
                    error = %error,
                    "Remote task event stream failed, falling back to completion polling"
                );
                (
                    Vec::new(),
                    self.poll_remote_completion(
                        &client,
                        &peer,
                        auth_token.as_deref(),
                        &remote_task_id,
                    )
                    .await?,
                )
            }
        };

        Ok(RemoteSendResult {
            envelope,
            trusted: peer.trusted,
            status: completion.status,
            remote_task_id,
            answer: completion.answer,
            provider: completion.provider,
            duration_ms: completion.duration_ms,
            message: completion.message,
            events,
        })
    }

    async fn resolve_peer(
        &self,
        peers: &[RemotePeer],
        prompt: &str,
        selection: &SelectionContext,
        options: &RemoteSendOptions,
    ) -> Result<RemotePeer> {
        if let Some(node) = options
            .node
            .as_deref()
            .filter(|node| !node.eq_ignore_ascii_case("auto"))
        {
            let Some(peer) = peers.iter().find(|peer| {
                peer.identity.node_name.eq_ignore_ascii_case(node)
                    || peer.target.eq_ignore_ascii_case(node)
                    || peer.identity.node_id == node
            }) else {
                bail!("Remote node '{}' is not paired", node);
            };
            self.validate_peer_selection(peer, options)?;
            return Ok(peer.clone());
        }

        self.select_peer(peers, prompt, selection, options).await
    }

    fn resolve_peer_preview(
        &self,
        peers: &[RemotePeer],
        prompt: &str,
        selection: &SelectionContext,
        options: &RemoteSendOptions,
    ) -> Result<RemotePeer> {
        if let Some(node) = options
            .node
            .as_deref()
            .filter(|node| !node.eq_ignore_ascii_case("auto"))
        {
            let Some(peer) = peers.iter().find(|peer| {
                peer.identity.node_name.eq_ignore_ascii_case(node)
                    || peer.target.eq_ignore_ascii_case(node)
                    || peer.identity.node_id == node
            }) else {
                bail!("Remote node '{}' is not paired", node);
            };
            self.validate_peer_selection(peer, options)?;
            return Ok(peer.clone());
        }

        self.select_peer_preview(peers, prompt, selection, options)
    }

    fn validate_peer_selection(
        &self,
        peer: &RemotePeer,
        options: &RemoteSendOptions,
    ) -> Result<()> {
        if !peer.trusted {
            bail!(
                "Remote node '{}' is paired but not trusted. Run `rove remote trust {}` first.",
                peer.identity.node_name,
                peer.identity.node_name
            );
        }
        if !options.allow_executor_only
            && matches!(peer.profile.execution_role, NodeExecutionRole::ExecutorOnly)
        {
            bail!(
                "Remote node '{}' is executor-only. Retry with `--allow-executor-only` or choose a full node.",
                peer.identity.node_name
            );
        }
        for tag in &options.required_tags {
            if !peer
                .profile
                .tags
                .iter()
                .any(|value| value.eq_ignore_ascii_case(tag))
            {
                bail!(
                    "Remote node '{}' does not advertise required tag '{}'.",
                    peer.identity.node_name,
                    tag
                );
            }
        }
        for capability in &options.required_capabilities {
            if !peer
                .profile
                .capabilities
                .iter()
                .any(|value| value.eq_ignore_ascii_case(capability))
            {
                bail!(
                    "Remote node '{}' does not advertise required capability '{}'.",
                    peer.identity.node_name,
                    capability
                );
            }
        }
        Ok(())
    }

    async fn select_peer(
        &self,
        peers: &[RemotePeer],
        prompt: &str,
        selection: &SelectionContext,
        options: &RemoteSendOptions,
    ) -> Result<RemotePeer> {
        let prompt_lower = prompt.to_ascii_lowercase();
        let live_candidates = self.load_peer_selection_candidates(peers).await;
        let mut candidates = live_candidates
            .into_iter()
            .filter(|peer| peer.peer.trusted)
            .filter(|peer| {
                options.allow_executor_only
                    || !matches!(
                        peer.peer.profile.execution_role,
                        NodeExecutionRole::ExecutorOnly
                    )
            })
            .filter(|peer| {
                options.required_tags.iter().all(|tag| {
                    peer.peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(tag))
                })
            })
            .filter(|peer| {
                options.required_capabilities.iter().all(|capability| {
                    peer.peer
                        .profile
                        .capabilities
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(capability))
                })
            })
            .map(|peer| {
                let mut score = 0_i64;
                if prompt_lower.contains(&peer.peer.identity.node_name.to_ascii_lowercase()) {
                    score += 100;
                }
                for tag in &peer.peer.profile.tags {
                    if prompt_lower.contains(&tag.to_ascii_lowercase()) {
                        score += 20;
                    }
                }
                if let Some(workspace_name) = &selection.workspace_name {
                    if peer
                        .peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(workspace_name))
                    {
                        score += 35;
                    }
                }
                if let Some(domain_tag) = &selection.domain_tag {
                    if peer
                        .peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(domain_tag))
                    {
                        score += 15;
                    }
                }
                for tag in &selection.policy_tags {
                    if peer
                        .peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(tag))
                    {
                        score += 10;
                    }
                }
                for capability in &selection.preferred_capabilities {
                    if peer
                        .peer
                        .profile
                        .capabilities
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(capability))
                    {
                        score += 18;
                    }
                }
                let prefer_executor_only =
                    options.prefer_executor_only || selection.direct_executor_candidate;
                if prefer_executor_only {
                    score += match peer.peer.profile.execution_role {
                        NodeExecutionRole::ExecutorOnly => 25,
                        NodeExecutionRole::Full => 5,
                    };
                } else {
                    score += match peer.peer.profile.execution_role {
                        NodeExecutionRole::Full => 25,
                        NodeExecutionRole::ExecutorOnly => 5,
                    };
                }
                score += peer.peer.profile.capabilities.len() as i64;

                if let Some(load) = &peer.load {
                    score -= (load.pending_tasks as i64) * 5;
                    score -= (load.running_tasks as i64) * 12;
                    score -= (load.recent_failures as i64) * 18;
                    score += (load.recent_successes as i64) * 2;
                    if let Some(avg_duration) = load.recent_avg_duration_ms {
                        score -= (avg_duration / 5_000).clamp(0, 12);
                    }
                } else {
                    score -= 6;
                }

                (score, peer)
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| right.0.cmp(&left.0));

        candidates
            .into_iter()
            .map(|(_, peer)| peer.peer)
            .next()
            .ok_or_else(|| {
                let mut message =
                    "No trusted remote node matches the requested selection.".to_string();
                if !options.required_tags.is_empty() {
                    message.push_str(&format!(" tags={}", options.required_tags.join(",")));
                }
                if !options.required_capabilities.is_empty() {
                    message.push_str(&format!(
                        " capabilities={}",
                        options.required_capabilities.join(",")
                    ));
                }
                if !options.allow_executor_only {
                    message.push_str(
                        " Executor-only nodes are excluded by default; retry with `--allow-executor-only` if that is intentional.",
                    );
                }
                anyhow::anyhow!(message)
            })
    }

    fn select_peer_preview(
        &self,
        peers: &[RemotePeer],
        prompt: &str,
        selection: &SelectionContext,
        options: &RemoteSendOptions,
    ) -> Result<RemotePeer> {
        let prompt_lower = prompt.to_ascii_lowercase();
        let mut candidates = peers
            .iter()
            .filter(|peer| peer.trusted)
            .filter(|peer| {
                options.allow_executor_only
                    || !matches!(peer.profile.execution_role, NodeExecutionRole::ExecutorOnly)
            })
            .filter(|peer| {
                options.required_tags.iter().all(|tag| {
                    peer.profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(tag))
                })
            })
            .filter(|peer| {
                options.required_capabilities.iter().all(|capability| {
                    peer.profile
                        .capabilities
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(capability))
                })
            })
            .map(|peer| {
                let mut score = 0_i64;
                if prompt_lower.contains(&peer.identity.node_name.to_ascii_lowercase()) {
                    score += 100;
                }
                for tag in &peer.profile.tags {
                    if prompt_lower.contains(&tag.to_ascii_lowercase()) {
                        score += 20;
                    }
                }
                if let Some(workspace_name) = &selection.workspace_name {
                    if peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(workspace_name))
                    {
                        score += 35;
                    }
                }
                if let Some(domain_tag) = &selection.domain_tag {
                    if peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(domain_tag))
                    {
                        score += 15;
                    }
                }
                (score, peer.clone())
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| right.0.cmp(&left.0));
        candidates
            .into_iter()
            .map(|(_, peer)| peer)
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!("No trusted remote node matches the requested selection.")
            })
    }

    async fn load_peer_selection_candidates(
        &self,
        peers: &[RemotePeer],
    ) -> Vec<PeerSelectionCandidate> {
        let mut candidates = Vec::with_capacity(peers.len());
        for peer in peers {
            let load = self
                .fetch_peer_status(peer)
                .await
                .ok()
                .and_then(|status| status.load);
            candidates.push(PeerSelectionCandidate {
                peer: peer.clone(),
                load,
            });
        }
        candidates
    }

    async fn fetch_peer_status(&self, peer: &RemotePeer) -> Result<RemoteStatus> {
        self.fetch_pair_status(&self.peer_endpoint(peer)).await
    }

    async fn fetch_pair_status(&self, endpoint: &str) -> Result<RemoteStatus> {
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .context("Failed to build remote status client")?;
        let request = client.get(format!(
            "{}/v1/remote/status/public",
            endpoint.trim_end_matches('/')
        ));
        let response = request
            .send()
            .await
            .context("Failed to fetch remote node status")?;
        parse_remote_status_response(response).await
    }

    async fn sync_drivers_to_peer(
        &self,
        peer: &RemotePeer,
        drivers: &[RemoteDriverSyncItem],
        dry_run: bool,
    ) -> Result<Vec<RemoteDriverSyncResult>> {
        if !peer.trusted {
            bail!(
                "Remote node '{}' is paired but not trusted",
                peer.identity.node_name
            );
        }

        let Some(auth_token) = self.optional_auth_token_for_peer(peer).await else {
            bail!(
                "Remote node '{}' does not have a stored auth token. Pair it with --token or configure ws_client.auth_token.",
                peer.identity.node_name
            );
        };

        let remote_extensions = self
            .fetch_remote_driver_inventory(peer, &auth_token)
            .await
            .unwrap_or_default();
        let client = Client::new();
        let endpoint = self.peer_endpoint(peer);

        let mut results = Vec::new();
        for driver in drivers {
            let installed = remote_extensions
                .iter()
                .find(|item| item.id == driver.id && item.kind == "driver");

            let action = if let Some(installed) = installed {
                if installed.version.as_deref() == Some(driver.version.as_str()) {
                    if installed.state
                        == if driver.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    {
                        "unchanged"
                    } else if driver.enabled {
                        "enable"
                    } else {
                        "disable"
                    }
                } else {
                    "upgrade"
                }
            } else {
                "install"
            };

            if dry_run {
                results.push(RemoteDriverSyncResult {
                    node_name: peer.identity.node_name.clone(),
                    driver_id: driver.id.clone(),
                    action: action.to_string(),
                    version: driver.version.clone(),
                    status: "planned".to_string(),
                    detail: None,
                });
                continue;
            }

            match action {
                "unchanged" => results.push(RemoteDriverSyncResult {
                    node_name: peer.identity.node_name.clone(),
                    driver_id: driver.id.clone(),
                    action: action.to_string(),
                    version: driver.version.clone(),
                    status: "ok".to_string(),
                    detail: None,
                }),
                "enable" | "disable" => {
                    self.set_remote_extension_enabled(
                        &client,
                        &endpoint,
                        &auth_token,
                        &driver.id,
                        action == "enable",
                    )
                    .await?;
                    results.push(RemoteDriverSyncResult {
                        node_name: peer.identity.node_name.clone(),
                        driver_id: driver.id.clone(),
                        action: action.to_string(),
                        version: driver.version.clone(),
                        status: "ok".to_string(),
                        detail: None,
                    });
                }
                "install" | "upgrade" => {
                    self.install_remote_driver(
                        &client,
                        &endpoint,
                        &auth_token,
                        driver,
                        action == "upgrade",
                    )
                    .await?;
                    if !driver.enabled {
                        self.set_remote_extension_enabled(
                            &client,
                            &endpoint,
                            &auth_token,
                            &driver.id,
                            false,
                        )
                        .await?;
                    }
                    results.push(RemoteDriverSyncResult {
                        node_name: peer.identity.node_name.clone(),
                        driver_id: driver.id.clone(),
                        action: action.to_string(),
                        version: driver.version.clone(),
                        status: "ok".to_string(),
                        detail: None,
                    });
                }
                _ => unreachable!(),
            }
        }

        Ok(results)
    }

    async fn fetch_remote_driver_inventory(
        &self,
        peer: &RemotePeer,
        auth_token: &str,
    ) -> Result<Vec<RemoteExtensionInventoryItem>> {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("Failed to build remote extension inventory client")?;
        let response = client
            .get(format!(
                "{}/v1/extensions",
                self.peer_endpoint(peer).trim_end_matches('/')
            ))
            .bearer_auth(auth_token)
            .send()
            .await
            .context("Failed to query remote extension inventory")?;
        parse_json_response(response, "Failed to parse remote extension inventory").await
    }

    async fn install_remote_driver(
        &self,
        client: &Client,
        endpoint: &str,
        auth_token: &str,
        driver: &RemoteDriverSyncItem,
        upgrade: bool,
    ) -> Result<()> {
        let path = if upgrade {
            "/v1/extensions/upgrade"
        } else {
            "/v1/extensions/install"
        };
        let response = client
            .post(format!("{}{}", endpoint.trim_end_matches('/'), path))
            .bearer_auth(auth_token)
            .json(&serde_json::json!({
                "kind": "driver",
                "source": driver.source,
                "registry": driver.registry,
                "version": driver.version,
            }))
            .send()
            .await
            .context("Failed to reach remote extension install API")?;
        let _value: serde_json::Value =
            parse_json_response(response, "Remote driver install failed").await?;
        Ok(())
    }

    async fn set_remote_extension_enabled(
        &self,
        client: &Client,
        endpoint: &str,
        auth_token: &str,
        driver_id: &str,
        enabled: bool,
    ) -> Result<()> {
        let action = if enabled { "enable" } else { "disable" };
        let response = client
            .post(format!(
                "{}/v1/extensions/driver/{}/{}",
                endpoint.trim_end_matches('/'),
                urlencoding::encode(driver_id),
                action
            ))
            .bearer_auth(auth_token)
            .send()
            .await
            .context("Failed to reach remote extension state API")?;
        let _value: serde_json::Value =
            parse_json_response(response, "Remote driver state update failed").await?;
        Ok(())
    }

    pub async fn fetch_remote_handshake(endpoint: &str) -> Result<RemoteHandshakeProof> {
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .context("Failed to build remote handshake client")?;
        let challenge = Uuid::new_v4().to_string();
        let response = client
            .post(format!(
                "{}/v1/remote/handshake",
                endpoint.trim_end_matches('/')
            ))
            .json(&serde_json::json!({ "challenge": challenge }))
            .send()
            .await
            .context("Failed to request remote handshake")?;
        let proof: RemoteHandshakeProof =
            parse_json_response(response, "Failed to parse remote handshake").await?;
        if !Self::verify_handshake(&challenge, &proof) {
            bail!("Remote handshake signature verification failed");
        }
        Ok(proof)
    }

    async fn stream_remote_events(
        &self,
        peer: &RemotePeer,
        auth_token: Option<&str>,
        task_id: &str,
    ) -> Result<(Vec<RemoteTaskEvent>, RemoteExecuteResponse)> {
        let mut request = websocket_task_url(&self.peer_endpoint(peer), true)?
            .into_client_request()
            .context("Failed to prepare remote WebSocket request")?;
        if let Some(auth_token) = auth_token.filter(|value| !value.trim().is_empty()) {
            let header = HeaderValue::from_str(&format!("Bearer {}", auth_token))
                .context("Invalid remote bearer token")?;
            request.headers_mut().insert(AUTHORIZATION, header);
        }
        for (name, value) in
            self.signed_request_headers(&peer.identity.node_id, "event_stream", None)?
        {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .context("Invalid remote signed header name")?;
            let header_value =
                HeaderValue::from_str(&value).context("Invalid remote signed header value")?;
            request.headers_mut().insert(header_name, header_value);
        }

        let (mut ws, _) = connect_async(request)
            .await
            .context("Failed to open remote task stream")?;

        let subscribe = RemoteTaskStreamClientMessage::SubscribeTask {
            task_id: task_id.to_string(),
        };
        ws.send(WsMessage::Text(serde_json::to_string(&subscribe)?))
            .await
            .context("Failed to subscribe to remote task stream")?;

        let deadline = Instant::now() + Duration::from_secs(120);
        let mut events = Vec::new();

        while let Some(message) = ws.next().await {
            let message = message.context("Remote task stream failed")?;
            match message {
                WsMessage::Text(text) => {
                    let server: RemoteTaskStreamServerMessage = serde_json::from_str(&text)
                        .with_context(|| format!("Invalid remote task stream message: {}", text))?;
                    match server {
                        RemoteTaskStreamServerMessage::Event {
                            task_id,
                            event_type,
                            payload,
                            step_num,
                            domain,
                            created_at,
                        } => {
                            events.push(RemoteTaskEvent {
                                task_id,
                                event_type,
                                payload,
                                step_num,
                                domain,
                                created_at,
                            });
                        }
                        RemoteTaskStreamServerMessage::Result {
                            answer,
                            provider,
                            duration_ms,
                            ..
                        } => {
                            return Ok((
                                events,
                                RemoteExecuteResponse {
                                    success: true,
                                    task_id: Some(task_id.to_string()),
                                    status: "completed".to_string(),
                                    answer: Some(answer),
                                    provider,
                                    duration_ms: Some(duration_ms),
                                    message: None,
                                },
                            ));
                        }
                        RemoteTaskStreamServerMessage::Error { message } => {
                            return Ok((
                                events,
                                RemoteExecuteResponse {
                                    success: false,
                                    task_id: Some(task_id.to_string()),
                                    status: "failed".to_string(),
                                    answer: None,
                                    provider: None,
                                    duration_ms: None,
                                    message: Some(message),
                                },
                            ));
                        }
                        RemoteTaskStreamServerMessage::Accepted { .. }
                        | RemoteTaskStreamServerMessage::Progress { .. }
                        | RemoteTaskStreamServerMessage::Connected { .. }
                        | RemoteTaskStreamServerMessage::Pong => {}
                    }
                }
                WsMessage::Ping(payload) => {
                    ws.send(WsMessage::Pong(payload))
                        .await
                        .context("Failed to reply to remote task stream ping")?;
                }
                WsMessage::Close(_) => break,
                _ => {}
            }

            if Instant::now() >= deadline {
                bail!("Remote task stream timed out after 120s");
            }
        }

        bail!("Remote task stream closed before task completion")
    }

    async fn poll_remote_completion(
        &self,
        client: &Client,
        peer: &RemotePeer,
        auth_token: Option<&str>,
        task_id: &str,
    ) -> Result<RemoteExecuteResponse> {
        let deadline = Instant::now() + Duration::from_secs(120);
        let status_url = format!(
            "{}/v1/remote/tasks/{}",
            self.peer_endpoint(peer).trim_end_matches('/'),
            task_id
        );

        loop {
            let mut request = client.get(&status_url);
            if let Some(auth_token) = auth_token.filter(|value| !value.trim().is_empty()) {
                request = request.bearer_auth(auth_token);
            }
            for (name, value) in
                self.signed_request_headers(&peer.identity.node_id, "task_status", Some(task_id))?
            {
                request = request.header(&name, value);
            }
            let response = request
                .send()
                .await
                .context("Failed to poll remote daemon")?;
            let completion = parse_remote_response(response).await?;
            match completion.status.as_str() {
                "running" if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                "running" => {
                    return Ok(RemoteExecuteResponse {
                        success: true,
                        task_id: Some(task_id.to_string()),
                        status: "running".to_string(),
                        answer: None,
                        provider: None,
                        duration_ms: None,
                        message: Some(
                            "Remote task is still running; polling timed out after 120s"
                                .to_string(),
                        ),
                    });
                }
                _ => return Ok(completion),
            }
        }
    }

    async fn optional_auth_token_for_peer(&self, peer: &RemotePeer) -> Option<String> {
        if let Some(secret_key) = &peer.auth_secret_key {
            return SecretManager::new(SERVICE_NAME)
                .get_secret(secret_key)
                .await
                .ok();
        }

        self.config
            .ws_client
            .auth_token
            .clone()
            .filter(|token| !token.trim().is_empty())
    }

    fn peer_endpoint(&self, peer: &RemotePeer) -> String {
        if let Some(base_url) =
            best_transport_record(&peer.transports).and_then(|record| record.base_url.clone())
        {
            return base_url;
        }
        peer.target.clone()
    }

    pub fn signed_request_headers_pub(
        &self,
        target_node_id: &str,
        purpose: &str,
        task_id: Option<&str>,
    ) -> Result<Vec<(String, String)>> {
        self.signed_request_headers(target_node_id, purpose, task_id)
    }

    fn signed_request_headers(
        &self,
        target_node_id: &str,
        purpose: &str,
        task_id: Option<&str>,
    ) -> Result<Vec<(String, String)>> {
        let metadata = self.load_or_init_node_metadata()?;
        let timestamp = unix_timestamp_secs()?;
        let nonce = Uuid::new_v4().simple().to_string();
        let payload = signed_request_payload(
            &metadata.identity.node_id,
            target_node_id,
            purpose,
            timestamp,
            &nonce,
            task_id,
        );
        let signature = IdentityManager::new(self.config.clone()).sign_message(&payload)?;
        Ok(vec![
            (
                HEADER_ORIGIN_NODE_ID.to_string(),
                metadata.identity.node_id.clone(),
            ),
            (
                HEADER_TARGET_NODE_ID.to_string(),
                target_node_id.to_string(),
            ),
            (HEADER_REMOTE_PURPOSE.to_string(), purpose.to_string()),
            (HEADER_REMOTE_NONCE.to_string(), nonce),
            (HEADER_REMOTE_TIMESTAMP.to_string(), timestamp.to_string()),
            (HEADER_REMOTE_SIGNATURE.to_string(), signature),
        ])
    }

    pub fn verify_signed_request(
        &self,
        headers: &HeaderMap,
        purpose: &str,
        task_id: Option<&str>,
    ) -> Result<RemotePeer> {
        let origin_node_id = header_value(headers, HEADER_ORIGIN_NODE_ID)?;
        let target_node_id = header_value(headers, HEADER_TARGET_NODE_ID)?;
        let remote_purpose = header_value(headers, HEADER_REMOTE_PURPOSE)?;
        let nonce = header_value(headers, HEADER_REMOTE_NONCE)?;
        let timestamp_raw = header_value(headers, HEADER_REMOTE_TIMESTAMP)?;
        let signature = header_value(headers, HEADER_REMOTE_SIGNATURE)?;

        if remote_purpose != purpose {
            bail!(
                "Remote request purpose mismatch: expected '{}', got '{}'",
                purpose,
                remote_purpose
            );
        }

        let local = self.load_or_init_node_metadata()?;
        if target_node_id != local.identity.node_id {
            bail!("Remote request target does not match this node");
        }

        let timestamp = timestamp_raw
            .parse::<u64>()
            .with_context(|| format!("Invalid remote request timestamp '{}'", timestamp_raw))?;
        let now = unix_timestamp_secs()?;
        if now.abs_diff(timestamp) > REMOTE_SIGNATURE_TTL_SECS {
            bail!("Remote request signature expired");
        }

        prune_remote_nonce_cache(now);
        let cache_key = format!("{}:{}:{}", origin_node_id, purpose, nonce);
        if remote_nonce_cache().contains_key(&cache_key) {
            bail!("Remote request replay detected");
        }

        let peer = self
            .load_peers()?
            .into_iter()
            .find(|peer| peer.identity.node_id == origin_node_id)
            .ok_or_else(|| anyhow::anyhow!("Remote request came from an unknown node"))?;
        if !peer.trusted {
            bail!(
                "Remote node '{}' is paired but not trusted",
                peer.identity.node_name
            );
        }

        let payload = signed_request_payload(
            &origin_node_id,
            &target_node_id,
            &remote_purpose,
            timestamp,
            &nonce,
            task_id,
        );
        if !IdentityManager::verify_message(&peer.identity.public_key, &payload, &signature) {
            bail!("Remote request signature verification failed");
        }

        remote_nonce_cache().insert(cache_key, now);
        Ok(peer)
    }

    fn remote_node_file(&self) -> PathBuf {
        self.config.core.data_dir.join("node-profile.toml")
    }

    fn legacy_remote_node_file(&self) -> PathBuf {
        self.config.core.data_dir.join("remote-node.toml")
    }

    fn remote_peers_file(&self) -> PathBuf {
        self.config.core.data_dir.join("remote-peers.toml")
    }

    fn load_or_init_node_metadata(&self) -> Result<RemoteNodeMetadata> {
        let identity_manager = IdentityManager::new(self.config.clone());
        let mut identity = identity_manager.load_or_init()?;
        let path = self.remote_node_file();
        if path.exists() {
            let raw = fs::read_to_string(&path)?;
            if let Ok(file) = toml::from_str::<NodeProfileFile>(&raw) {
                return Ok(RemoteNodeMetadata {
                    identity,
                    profile: file.profile,
                });
            }
            if let Ok(legacy) = toml::from_str::<RemoteNodeMetadata>(&raw) {
                if identity.node_name != legacy.identity.node_name
                    && !legacy.identity.node_name.trim().is_empty()
                {
                    identity = identity_manager.rename(&legacy.identity.node_name)?;
                }
                self.save_node_profile(&legacy.profile)?;
                return Ok(RemoteNodeMetadata {
                    identity,
                    profile: legacy.profile,
                });
            }
        }

        let legacy_path = self.legacy_remote_node_file();
        if legacy_path.exists() {
            let raw = fs::read_to_string(&legacy_path)?;
            if let Ok(legacy) = toml::from_str::<RemoteNodeMetadata>(&raw) {
                if identity.node_name != legacy.identity.node_name
                    && !legacy.identity.node_name.trim().is_empty()
                {
                    identity = identity_manager.rename(&legacy.identity.node_name)?;
                }
                self.save_node_profile(&legacy.profile)?;
                return Ok(RemoteNodeMetadata {
                    identity,
                    profile: legacy.profile,
                });
            }
        }

        let profile = default_local_profile(&self.config);
        self.save_node_profile(&profile)?;
        Ok(RemoteNodeMetadata { identity, profile })
    }

    fn save_node_profile(&self, profile: &NodeProfile) -> Result<()> {
        let path = self.remote_node_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            path,
            toml::to_string_pretty(&NodeProfileFile {
                profile: profile.clone(),
            })?,
        )?;
        Ok(())
    }

    fn load_peers(&self) -> Result<Vec<RemotePeer>> {
        let path = self.remote_peers_file();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)?;
        if let Ok(file) = toml::from_str::<RemotePeersFile>(&raw) {
            return Ok(file.peers);
        }

        #[derive(Debug, Deserialize)]
        struct LegacyRemotePeersFile {
            remote_peers: Vec<RemotePeer>,
        }

        if let Ok(file) = toml::from_str::<LegacyRemotePeersFile>(&raw) {
            return Ok(file.remote_peers);
        }

        Err(anyhow::anyhow!("Failed to parse {}", path.display()))
    }

    fn save_peers(&self, peers: &[RemotePeer]) -> Result<()> {
        let path = self.remote_peers_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = RemotePeersFile {
            peers: peers.to_vec(),
        };
        fs::write(path, toml::to_string_pretty(&file)?)?;
        Ok(())
    }

    fn derive_selection_context(&self, prompt: &str) -> SelectionContext {
        let workspace_name = self
            .config
            .core
            .workspace
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .filter(|name| !name.trim().is_empty());
        let domain_tag = Some(infer_domain(&self.config.core.workspace).to_string());

        SelectionContext {
            workspace_name,
            domain_tag,
            policy_tags: Vec::new(),
            preferred_tools: Vec::new(),
            preferred_capabilities: Vec::new(),
            direct_executor_candidate: looks_like_direct_executor_task(prompt),
        }
    }

    async fn enrich_selection_context_with_policy(
        &self,
        prompt: &str,
        selection: &mut SelectionContext,
    ) {
        let Ok(report) = PolicyManager::new(self.config.clone(), None)
            .explain(prompt)
            .await
        else {
            return;
        };
        let (policy_tags, preferred_tools, preferred_capabilities) =
            policy_report_to_hints(Some(&report));
        selection.policy_tags = policy_tags;
        selection.preferred_tools = preferred_tools;
        selection.preferred_capabilities = preferred_capabilities;
    }

    pub async fn plan_execution_bundle(
        &self,
        prompt: &str,
        post_commands: &[String],
    ) -> Result<Option<RemoteExecutionPlan>> {
        let mut selection = self.derive_selection_context(prompt);
        self.enrich_selection_context_with_policy(prompt, &mut selection)
            .await;
        let mut plan = match self.build_executor_plan(prompt, &selection)? {
            Some(plan) => plan,
            None => return Ok(None),
        };

        if looks_like_mutating_executor_task(prompt) {
            for command in post_commands {
                let command = command.trim();
                if command.is_empty() {
                    continue;
                }
                plan.append_step(
                    format!("verify workspace state with `{}`", command),
                    "run_command",
                    serde_json::json!({ "command": command }),
                );
            }
        }

        Ok(Some(plan))
    }

    fn build_executor_plan(
        &self,
        prompt: &str,
        selection: &SelectionContext,
    ) -> Result<Option<RemoteExecutionPlan>> {
        let prompt_trimmed = prompt.trim();
        let prompt_lower = prompt_trimmed.to_ascii_lowercase();
        let extracted = extract_quoted_or_path_token(prompt_trimmed);
        let domain_hint = selection.domain_tag.clone();

        if prompt_lower.contains("screenshot") || prompt_lower.contains("capture screen") {
            return Ok(Some(RemoteExecutionPlan::direct(
                "capture screenshot",
                "capture_screen",
                serde_json::json!({"output_file":"remote-screenshot.png"}),
                domain_hint,
            )));
        }

        if contains_any(&prompt_lower, &["read ", "show ", "open ", "cat "]) {
            if let Some(path) = extracted.clone() {
                return Ok(Some(RemoteExecutionPlan::direct(
                    format!("read file {}", path),
                    "read_file",
                    serde_json::json!({ "path": path }),
                    domain_hint,
                )));
            }
        }

        if contains_any(&prompt_lower, &["does ", "exist", "file exists"]) {
            if let Some(path) = extracted.clone() {
                return Ok(Some(RemoteExecutionPlan::direct(
                    format!("check whether {} exists", path),
                    "file_exists",
                    serde_json::json!({ "path": path }),
                    domain_hint,
                )));
            }
        }

        if contains_any(&prompt_lower, &["list ", "directory", "folder", "files in"]) {
            let path = extracted.clone().unwrap_or_else(|| ".".to_string());
            return Ok(Some(RemoteExecutionPlan::direct(
                format!("list directory {}", path),
                "list_dir",
                serde_json::json!({ "path": path }),
                domain_hint,
            )));
        }

        if prompt_lower.contains("find ") || prompt_lower.contains("locate ") {
            if let Some(name) = extracted {
                let command = format!("fd -a {} .", shlex::try_quote(&name)?);
                return Ok(Some(RemoteExecutionPlan::direct(
                    format!("find {}", name),
                    "run_command",
                    serde_json::json!({ "command": command }),
                    domain_hint,
                )));
            }
        }

        if prompt_lower.contains("search ")
            && prompt_lower.contains(" in ")
            && selection
                .preferred_tools
                .iter()
                .any(|tool| tool.eq_ignore_ascii_case("run_command"))
        {
            if let Some(term) = extract_search_term(prompt_trimmed) {
                let command = format!("rg --line-number {} .", shlex::try_quote(&term)?);
                return Ok(Some(RemoteExecutionPlan::direct(
                    format!("search for {}", term),
                    "run_command",
                    serde_json::json!({ "command": command }),
                    domain_hint,
                )));
            }
        }

        Ok(None)
    }

    // ── PTY terminal ─────────────────────────────────────────────────────────

    /// Open an interactive PTY terminal on a remote node.
    ///
    /// Resolves the peer by name/id, then bridges local stdin/stdout to a
    /// remote PTY shell over a WebSocket connection. Falls back to HTTP-based
    /// WebSocket when iroh is unavailable.
    ///
    /// Handles terminal resize (SIGWINCH → `pty_resize` JSON message) and
    /// tears down cleanly on remote shell exit or local Ctrl-C.
    pub async fn open_terminal(&self, node: &str, shell: Option<&str>) -> Result<()> {
        if !self.config.ws_client.enabled {
            bail!("Remote service is disabled. Run `rove service enable remote` first.");
        }

        let peers = self.load_peers()?;
        let peer = peers
            .iter()
            .find(|p| {
                p.identity.node_name.eq_ignore_ascii_case(node)
                    || p.target.eq_ignore_ascii_case(node)
                    || p.identity.node_id == node
            })
            .ok_or_else(|| anyhow::anyhow!("Remote node '{}' is not paired", node))?;

        if !peer.trusted {
            bail!(
                "Remote node '{}' is paired but not trusted. Run `rove remote trust {}` first.",
                peer.identity.node_name,
                peer.identity.node_name
            );
        }

        let auth_token = self.optional_auth_token_for_peer(peer).await;
        let signed_headers =
            self.signed_request_headers(&peer.identity.node_id, "terminal", None)?;

        let mut ws_url = reqwest::Url::parse(
            &format!(
                "{}/v1/remote/terminal",
                self.peer_endpoint(peer).trim_end_matches('/')
            ),
        )
        .context("Invalid remote terminal URL")?;
        match ws_url.scheme() {
            "http" => ws_url
                .set_scheme("ws")
                .map_err(|_| anyhow::anyhow!("Failed to convert URL to ws://"))?,
            "https" => ws_url
                .set_scheme("wss")
                .map_err(|_| anyhow::anyhow!("Failed to convert URL to wss://"))?,
            _ => {}
        }
        if let Some(sh) = shell {
            ws_url
                .query_pairs_mut()
                .append_pair("shell", sh);
        }

        let mut request = ws_url
            .to_string()
            .into_client_request()
            .context("Failed to prepare terminal WebSocket request")?;
        if let Some(token) = auth_token.as_deref().filter(|t| !t.trim().is_empty()) {
            let header =
                HeaderValue::from_str(&format!("Bearer {}", token)).context("Invalid token")?;
            request.headers_mut().insert(AUTHORIZATION, header);
        }
        for (name, value) in signed_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .context("Invalid signed header name")?;
            let header_value =
                HeaderValue::from_str(&value).context("Invalid signed header value")?;
            request.headers_mut().insert(header_name, header_value);
        }

        let (mut ws, _) = connect_async(request)
            .await
            .context("Failed to open remote terminal stream")?;

        // Bridge local stdin → WS and WS → local stdout.
        use tokio::io::AsyncReadExt;
        let mut stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut stdin_buf = [0u8; 4096];

        loop {
            tokio::select! {
                n = stdin.read(&mut stdin_buf) => {
                    match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let data = base64_encode(&stdin_buf[..n]);
                            let msg = serde_json::json!({ "type": "stdin", "data": data });
                            if ws.send(WsMessage::Text(msg.to_string())).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                msg = ws.next() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                if val.get("type").and_then(|v| v.as_str()) == Some("stdout") {
                                    if let Some(data) = val.get("data").and_then(|v| v.as_str()) {
                                        if let Ok(bytes) = base64_decode(data) {
                                            use tokio::io::AsyncWriteExt;
                                            let _ = stdout.write_all(&bytes).await;
                                            let _ = stdout.flush().await;
                                        }
                                    }
                                } else if val.get("type").and_then(|v| v.as_str()) == Some("exit") {
                                    break;
                                }
                            }
                        }
                        Some(Ok(WsMessage::Close(_))) | None => break,
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }
}

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let _ = write!(out, "{}", CHARS[b0 >> 2] as char);
        let _ = write!(out, "{}", CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            let _ = write!(out, "{}", CHARS[((b1 & 15) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            let _ = write!(out, "{}", CHARS[b2 & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'=')
        .map(|b| match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 255,
        })
        .collect();
    if bytes.iter().any(|&b| b == 255) {
        bail!("Invalid base64 character");
    }
    for chunk in bytes.chunks(4) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let b3 = if chunk.len() > 3 { chunk[3] as usize } else { 0 };
        out.push(((b0 << 2) | (b1 >> 4)) as u8);
        if chunk.len() > 2 {
            out.push(((b1 & 15) << 4 | b2 >> 2) as u8);
        }
        if chunk.len() > 3 {
            out.push(((b2 & 3) << 6 | b3) as u8);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteNodeMetadata {
    identity: NodeIdentity,
    profile: NodeProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeProfileFile {
    profile: NodeProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemotePeersFile {
    peers: Vec<RemotePeer>,
}

#[derive(Debug, Clone, Serialize)]
struct RemoteExecuteRequest {
    task_id: Option<String>,
    input: Option<String>,
    task: Option<String>,
    origin_node: Option<String>,
    coordinator_node: Option<String>,
    workspace: Option<String>,
    team_id: Option<String>,
    wait_seconds: Option<u64>,
    plan: Option<RemoteExecutionPlan>,
}

#[derive(Debug, Clone, Deserialize)]
struct RemoteExecuteResponse {
    success: bool,
    task_id: Option<String>,
    status: String,
    answer: Option<String>,
    provider: Option<String>,
    duration_ms: Option<i64>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RemoteTaskStreamClientMessage {
    SubscribeTask { task_id: String },
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RemoteTaskStreamServerMessage {
    Connected {
        version: String,
    },
    Accepted {
        task_id: String,
    },
    Progress {
        message: String,
    },
    Event {
        task_id: String,
        event_type: String,
        payload: String,
        step_num: i64,
        domain: Option<String>,
        created_at: i64,
    },
    Result {
        answer: String,
        provider: Option<String>,
        duration_ms: i64,
        iterations: usize,
    },
    Error {
        message: String,
    },
    Pong,
}

async fn parse_remote_response(response: reqwest::Response) -> Result<RemoteExecuteResponse> {
    let status_code = response.status();
    let body = response.text().await.unwrap_or_default();
    let parsed: RemoteExecuteResponse = serde_json::from_str(&body).with_context(|| {
        format!(
            "Remote daemon returned non-JSON response (status {}): {}",
            status_code, body
        )
    })?;
    if !parsed.success && status_code.is_success() {
        bail!(
            "Remote daemon reported failure: {}",
            parsed
                .message
                .clone()
                .unwrap_or_else(|| parsed.status.clone())
        );
    }
    if !status_code.is_success() && status_code.as_u16() != 202 {
        bail!(
            "Remote daemon rejected request: {}",
            parsed.message.unwrap_or_else(|| format!(
                "status {} {}",
                status_code.as_u16(),
                parsed.status
            ))
        );
    }
    Ok(parsed)
}

async fn parse_remote_status_response(response: reqwest::Response) -> Result<RemoteStatus> {
    let status_code = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status_code.is_success() {
        bail!(
            "Remote status request failed (status {}): {}",
            status_code,
            body
        );
    }
    serde_json::from_str(&body).with_context(|| {
        format!(
            "Remote daemon returned invalid status payload (status {}): {}",
            status_code, body
        )
    })
}

async fn parse_json_response<T>(response: reqwest::Response, context: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let status_code = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status_code.is_success() {
        bail!("{} (status {}): {}", context, status_code, body);
    }
    serde_json::from_str(&body)
        .with_context(|| format!("{} (status {}): {}", context, status_code, body))
}

fn resolve_pair_target(target: &str, url: Option<&str>) -> Result<(String, String)> {
    match url {
        Some(url) => Ok((target.to_string(), normalize_base_url(url)?)),
        None if target.starts_with("http://") || target.starts_with("https://") => {
            Ok((derive_node_name(target), normalize_base_url(target)?))
        }
        None => bail!(
            "Pairing requires a daemon URL. Use `rove remote pair office-mac --url http://host:{} --token ...` or pass the URL directly as the target.",
            crate::info::DEFAULT_PORT
        ),
    }
}

fn normalize_base_url(url: &str) -> Result<String> {
    let parsed =
        reqwest::Url::parse(url).with_context(|| format!("Invalid remote daemon URL '{}'", url))?;
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

fn websocket_task_url(base_url: &str, remote_route: bool) -> Result<String> {
    let mut url = reqwest::Url::parse(base_url)
        .with_context(|| format!("Invalid remote daemon URL '{}'", base_url))?;
    match url.scheme() {
        "http" => {
            url.set_scheme("ws")
                .map_err(|_| anyhow::anyhow!("Failed to convert '{}' to ws://", base_url))?;
        }
        "https" => {
            url.set_scheme("wss")
                .map_err(|_| anyhow::anyhow!("Failed to convert '{}' to wss://", base_url))?;
        }
        "ws" | "wss" => {}
        other => bail!("Unsupported remote daemon scheme '{}'", other),
    }
    url.set_path(if remote_route {
        "/v1/remote/events/ws"
    } else {
        "/ws/task"
    });
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn derive_node_name(target: &str) -> String {
    reqwest::Url::parse(target)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.replace('.', "-")))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "remote-node".to_string())
}

fn guess_transports_for_endpoint(endpoint: &str) -> Vec<RemoteTransportRecord> {
    reqwest::Url::parse(endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .map(|address| {
            vec![RemoteTransportRecord {
                kind: "direct".to_string(),
                address: address.clone(),
                base_url: Some(endpoint.trim_end_matches('/').to_string()),
                network_id: None,
                reachable: true,
                latency_ms: None,
                last_checked_at: None,
                last_error: None,
                iroh_node_id: None,
            }]
        })
        .unwrap_or_default()
}

fn best_transport_record(records: &[RemoteTransportRecord]) -> Option<&RemoteTransportRecord> {
    records.iter().max_by(|left, right| {
        transport_score(left)
            .cmp(&transport_score(right))
            .then_with(|| {
                right
                    .latency_ms
                    .unwrap_or(u64::MAX)
                    .cmp(&left.latency_ms.unwrap_or(u64::MAX))
            })
            .then_with(|| {
                left.last_checked_at
                    .unwrap_or_default()
                    .cmp(&right.last_checked_at.unwrap_or_default())
            })
    })
}

fn transport_score(record: &RemoteTransportRecord) -> i64 {
    let mut score = 0_i64;
    if record.reachable {
        score += 100;
    }
    if record.kind.eq_ignore_ascii_case("zerotier") {
        score += 20;
    }
    if let Some(latency_ms) = record.latency_ms {
        score -= (latency_ms / 10).min(50) as i64;
    }
    if record.last_error.is_some() {
        score -= 15;
    }
    score
}

fn handshake_payload(challenge: &str) -> Vec<u8> {
    format!("rove-remote-handshake:{}", challenge).into_bytes()
}

fn secret_key_fragment(node_name: &str) -> String {
    node_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn remote_nonce_cache() -> &'static DashMap<String, u64> {
    static CACHE: OnceLock<DashMap<String, u64>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn prune_remote_nonce_cache(now: u64) {
    remote_nonce_cache().retain(|_, seen_at| now.saturating_sub(*seen_at) <= REMOTE_NONCE_TTL_SECS);
}

fn unix_timestamp_secs() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("System clock is before the unix epoch")?
        .as_secs())
}

fn signed_request_payload(
    origin_node_id: &str,
    target_node_id: &str,
    purpose: &str,
    timestamp: u64,
    nonce: &str,
    task_id: Option<&str>,
) -> Vec<u8> {
    serde_json::json!({
        "origin_node_id": origin_node_id,
        "target_node_id": target_node_id,
        "purpose": purpose,
        "timestamp": timestamp,
        "nonce": nonce,
        "task_id": task_id.unwrap_or_default(),
    })
    .to_string()
    .into_bytes()
}

fn header_value(headers: &HeaderMap, name: &str) -> Result<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("Missing required remote header '{}'", name))
}

fn default_local_profile(config: &Config) -> NodeProfile {
    NodeProfile {
        capabilities: normalize_capabilities(&[
            "task-routing".to_string(),
            "remote-execution".to_string(),
            "system-execution".to_string(),
        ]),
        tags: vec![std::env::consts::OS.to_string()],
        execution_role: if matches!(config.daemon.profile, crate::config::DaemonProfile::Edge) {
            NodeExecutionRole::ExecutorOnly
        } else {
            NodeExecutionRole::Full
        },
    }
}

fn merge_paired_profile(
    mut advertised: NodeProfile,
    executor_only: bool,
    tags: &[String],
    capabilities: &[String],
) -> NodeProfile {
    if executor_only {
        advertised.execution_role = NodeExecutionRole::ExecutorOnly;
    }
    if !tags.is_empty() {
        advertised.tags = tags.to_vec();
    }
    if !capabilities.is_empty() {
        advertised.capabilities = normalize_capabilities(capabilities);
    } else {
        advertised.capabilities = normalize_capabilities(&advertised.capabilities);
    }
    advertised
}

fn normalize_capabilities(capabilities: &[String]) -> Vec<String> {
    let mut values = capabilities
        .iter()
        .filter_map(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect::<Vec<_>>();
    if !values
        .iter()
        .any(|value| value.eq_ignore_ascii_case("remote-execution"))
    {
        values.push("remote-execution".to_string());
    }
    values.sort();
    values.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    values
}

fn policy_report_to_hints(
    report: Option<&PolicyExplainReport>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let Some(report) = report else {
        return (Vec::new(), Vec::new(), Vec::new());
    };

    let mut policy_tags = report.active_policies.clone();
    policy_tags.extend(report.memory_tags.clone());
    policy_tags.sort();
    policy_tags.dedup();

    let preferred_tools = report.preferred_tools.clone();
    let mut preferred_capabilities = Vec::new();
    for tool in &preferred_tools {
        for capability in tool_capabilities(tool) {
            if !preferred_capabilities
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(capability))
            {
                preferred_capabilities.push(capability.to_string());
            }
        }
    }

    (policy_tags, preferred_tools, preferred_capabilities)
}

fn tool_capabilities(tool: &str) -> &'static [&'static str] {
    match tool {
        "run_command" => &["shell-execution", "system-execution"],
        "read_file" | "write_file" | "list_dir" | "file_exists" => {
            &["filesystem-access", "system-execution"]
        }
        "capture_screen" => &["vision-capture", "system-execution"],
        _ => &[],
    }
}

fn looks_like_direct_executor_task(prompt: &str) -> bool {
    let prompt_lower = prompt.to_ascii_lowercase();
    contains_any(
        &prompt_lower,
        &[
            "find ",
            "locate ",
            "read ",
            "show ",
            "open ",
            "list ",
            "directory",
            "folder",
            "file exists",
            "does ",
            "screenshot",
            "capture screen",
            "search ",
        ],
    )
}

fn looks_like_mutating_executor_task(prompt: &str) -> bool {
    contains_any(
        &prompt.to_ascii_lowercase(),
        &[
            "write ",
            "update ",
            "edit ",
            "modify ",
            "change ",
            "create ",
            "delete ",
            "remove ",
            "rename ",
            "refactor ",
            "patch ",
        ],
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn extract_quoted_or_path_token(prompt: &str) -> Option<String> {
    for delimiter in ['`', '"', '\''] {
        let mut parts = prompt.split(delimiter);
        let _ = parts.next();
        if let Some(segment) = parts.next() {
            let trimmed = segment.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    prompt
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| ",.:;!?()[]{}".contains(ch)))
        .find(|token| {
            token.contains('/')
                || token.contains('\\')
                || token.contains('.')
                || token.ends_with(".txt")
                || token.ends_with(".md")
                || token.ends_with(".rs")
        })
        .map(|token| token.to_string())
}

fn extract_search_term(prompt: &str) -> Option<String> {
    if let Some(term) = extract_quoted_or_path_token(prompt) {
        return Some(term);
    }

    let lowered = prompt.to_ascii_lowercase();
    let start = lowered.find("search ")?;
    let tail = prompt[start + "search ".len()..].trim();
    let term = tail
        .split(" in ")
        .next()
        .unwrap_or(tail)
        .trim_matches(|ch: char| ",.:;!?".contains(ch))
        .trim();
    (!term.is_empty()).then(|| term.to_string())
}

pub fn local_execution_role_for_config(config: &Config) -> Result<NodeExecutionRole> {
    Ok(RemoteManager::new(config.clone())
        .load_or_init_node_metadata()?
        .profile
        .execution_role)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{
            ws::{Message, WebSocket, WebSocketUpgrade},
            Json,
        },
        response::IntoResponse,
        routing::{get, post},
        Router,
    };
    use futures::StreamExt;
    use tempfile::TempDir;
    use tokio::net::TcpListener;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config() -> (TempDir, Config) {
        let temp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.core.workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        config.core.data_dir = temp.path().join("data");
        *config.policy.policy_dir_mut() = temp.path().join("policies");
        std::fs::create_dir_all(config.policy.policy_dir()).expect("policy dir");
        config.ws_client.enabled = true;
        config.ws_client.auth_token = Some("remote-token".to_string());
        (temp, config)
    }

    fn secondary_config(temp: &TempDir, name: &str) -> Config {
        let mut config = Config::default();
        config.core.workspace = temp.path().join(format!("workspace-{}", name));
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        config.core.data_dir = temp.path().join(format!("data-{}", name));
        *config.policy.policy_dir_mut() = temp.path().join(format!("policies-{}", name));
        std::fs::create_dir_all(config.policy.policy_dir()).expect("policy dir");
        config.ws_client.enabled = true;
        config.ws_client.auth_token = Some("remote-token".to_string());
        config
    }

    #[test]
    fn signed_remote_request_verifies_for_trusted_peer() {
        let (temp, config) = test_config();
        let local = RemoteManager::new(config.clone());
        let remote = RemoteManager::new(secondary_config(&temp, "remote"));

        let local_status = local.status().expect("local status");
        let remote_status = remote.status().expect("remote status");
        local
            .save_peers(&[RemotePeer {
                identity: remote_status.node.clone(),
                profile: remote_status.profile,
                target: "http://remote-node".to_string(),
                trusted: true,
                load: None,
                last_status_error: None,
                auth_secret_key: None,
                transports: Vec::new(),
                iroh_node_id: None,
            }])
            .expect("save peer");

        let mut headers = HeaderMap::new();
        for (name, value) in remote
            .signed_request_headers(&local_status.node.node_id, "execute", Some("task-1"))
            .expect("sign")
        {
            headers.insert(
                name.parse::<axum::http::HeaderName>().expect("header name"),
                value.parse().expect("header value"),
            );
        }

        let verified = local
            .verify_signed_request(&headers, "execute", Some("task-1"))
            .expect("verify");
        assert_eq!(verified.identity.node_id, remote_status.node.node_id);
    }

    #[test]
    fn edge_profile_resolves_to_executor_only_role() {
        let (_temp, mut config) = test_config();
        config.daemon.profile = crate::config::DaemonProfile::Edge;

        let role = local_execution_role_for_config(&config).expect("execution role");

        assert_eq!(role, NodeExecutionRole::ExecutorOnly);
    }

    #[test]
    fn signed_remote_request_rejects_replay() {
        let (temp, config) = test_config();
        let local = RemoteManager::new(config.clone());
        let remote = RemoteManager::new(secondary_config(&temp, "remote-replay"));

        let local_status = local.status().expect("local status");
        let remote_status = remote.status().expect("remote status");
        local
            .save_peers(&[RemotePeer {
                identity: remote_status.node.clone(),
                profile: remote_status.profile,
                target: "http://remote-node".to_string(),
                trusted: true,
                load: None,
                last_status_error: None,
                auth_secret_key: None,
                transports: Vec::new(),
                iroh_node_id: None,
            }])
            .expect("save peer");

        let mut headers = HeaderMap::new();
        for (name, value) in remote
            .signed_request_headers(&local_status.node.node_id, "execute", Some("task-replay"))
            .expect("sign")
        {
            headers.insert(
                name.parse::<axum::http::HeaderName>().expect("header name"),
                value.parse().expect("header value"),
            );
        }

        local
            .verify_signed_request(&headers, "execute", Some("task-replay"))
            .expect("first verify");
        let error = local
            .verify_signed_request(&headers, "execute", Some("task-replay"))
            .expect_err("replay should fail");
        assert!(error.to_string().contains("replay"));
    }

    #[tokio::test]
    async fn send_to_trusted_node_polls_until_completion() {
        let (_temp, config) = test_config();
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/remote/execute"))
            .and(header("authorization", "Bearer remote-token"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "remote-task-1",
                "status": "running",
                "answer": null,
                "provider": null,
                "duration_ms": null,
                "message": "accepted"
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/remote/tasks/remote-task-1"))
            .and(header("authorization", "Bearer remote-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "remote-task-1",
                "status": "completed",
                "answer": "done",
                "provider": "ollama",
                "duration_ms": 12,
                "message": null
            })))
            .mount(&server)
            .await;

        let manager = RemoteManager::new(config);
        manager
            .pair("office-mac", Some(&server.uri()), None, false, &[], &[])
            .await
            .expect("pair");
        manager.trust("office-mac").expect("trust");

        let result = manager
            .send("office-mac", "find test.txt")
            .await
            .expect("send");

        assert_eq!(result.status, "completed");
        assert_eq!(result.answer.as_deref(), Some("done"));
        assert_eq!(result.provider.as_deref(), Some("ollama"));
    }

    #[tokio::test]
    async fn send_streams_remote_events_when_ws_task_stream_exists() {
        async fn execute_remote() -> impl IntoResponse {
            Json(serde_json::json!({
                "success": true,
                "task_id": "stream-task-1",
                "status": "running",
                "answer": null,
                "provider": null,
                "duration_ms": null,
                "message": "accepted"
            }))
        }

        async fn ws_task(ws: WebSocketUpgrade) -> impl IntoResponse {
            ws.on_upgrade(|socket| async move {
                handle_stream_test_socket(socket).await;
            })
        }

        async fn handle_stream_test_socket(mut socket: WebSocket) {
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "connected",
                        "version": "test"
                    })
                    .to_string()
                    .into(),
                ))
                .await;

            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let message: serde_json::Value =
                    serde_json::from_str(&text).expect("valid subscribe message");
                if message.get("type").and_then(|v| v.as_str()) == Some("subscribe_task") {
                    let _ = socket
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "event",
                                "task_id": "stream-task-1",
                                "event_type": "thought",
                                "payload": "{\"summary\":\"searching\"}",
                                "step_num": 1,
                                "domain": "general",
                                "created_at": 1
                            })
                            .to_string()
                            .into(),
                        ))
                        .await;
                    let _ = socket
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "result",
                                "answer": "done",
                                "provider": "ollama",
                                "duration_ms": 7,
                                "iterations": 1
                            })
                            .to_string()
                            .into(),
                        ))
                        .await;
                    break;
                }
            }
        }

        let (_temp, config) = test_config();
        let app = Router::new()
            .route("/v1/remote/execute", post(execute_remote))
            .route("/v1/remote/events/ws", get(ws_task));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let manager = RemoteManager::new(config);
        manager
            .pair(
                "office-mac",
                Some(&format!("http://{}", addr)),
                None,
                false,
                &[],
                &[],
            )
            .await
            .expect("pair");
        manager.trust("office-mac").expect("trust");

        let result = manager
            .send("office-mac", "find test.txt")
            .await
            .expect("send");

        assert_eq!(result.status, "completed");
        assert_eq!(result.answer.as_deref(), Some("done"));
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].event_type, "thought");

        server.abort();
    }

    #[tokio::test]
    async fn send_requires_trusted_peer() {
        let (_temp, config) = test_config();
        let server = MockServer::start().await;
        let manager = RemoteManager::new(config);
        manager
            .pair("office-mac", Some(&server.uri()), None, false, &[], &[])
            .await
            .expect("pair");

        let error = manager
            .send("office-mac", "find test.txt")
            .await
            .expect_err("send should fail");
        assert!(error.to_string().contains("not trusted"));
    }

    #[tokio::test]
    async fn pair_persists_peer_inventory_in_wrapped_toml() {
        let (_temp, config) = test_config();
        let server = MockServer::start().await;
        let manager = RemoteManager::new(config);

        manager
            .pair(
                "office-mac",
                Some(&server.uri()),
                None,
                true,
                &["office".to_string()],
                &["system-execution".to_string()],
            )
            .await
            .expect("pair");

        let nodes = manager.nodes().expect("nodes");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].identity.node_name, "office-mac");
        assert_eq!(
            nodes[0].profile.execution_role,
            NodeExecutionRole::ExecutorOnly
        );
        assert_eq!(nodes[0].profile.tags, vec!["office".to_string()]);
        assert!(nodes[0]
            .profile
            .capabilities
            .contains(&"system-execution".to_string()));
    }

    #[tokio::test]
    async fn auto_selection_prefers_matching_tagged_full_node() {
        let (_temp, config) = test_config();
        let office = MockServer::start().await;
        let lab = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "office-task",
                "status": "completed",
                "answer": "office result",
                "provider": "ollama",
                "duration_ms": 10,
                "message": null
            })))
            .mount(&office)
            .await;

        let manager = RemoteManager::new(config);
        manager
            .pair(
                "office-mac",
                Some(&office.uri()),
                None,
                false,
                &["office".to_string()],
                &["system-execution".to_string()],
            )
            .await
            .expect("pair office");
        manager.trust("office-mac").expect("trust office");
        manager
            .pair(
                "lab-mac",
                Some(&lab.uri()),
                None,
                true,
                &["lab".to_string()],
                &["system-execution".to_string()],
            )
            .await
            .expect("pair lab");
        manager.trust("lab-mac").expect("trust lab");

        let result = manager
            .send_with_options(
                "find test.txt on the office machine",
                RemoteSendOptions {
                    node: Some("auto".to_string()),
                    required_capabilities: vec!["system-execution".to_string()],
                    ..RemoteSendOptions::default()
                },
            )
            .await
            .expect("send");

        assert_eq!(result.envelope.target_node, "office-mac");
        assert_eq!(result.answer.as_deref(), Some("office result"));
    }

    #[tokio::test]
    async fn auto_selection_prefers_lower_load_when_capabilities_match() {
        let (_temp, config) = test_config();
        let quiet = MockServer::start().await;
        let busy = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v1/remote/status/public"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "enabled": true,
                "node": {
                    "node_id": "quiet-node",
                    "node_name": "quiet-mac",
                    "public_key": "quiet-public"
                },
                "profile": {
                    "capabilities": ["system-execution", "remote-execution"],
                    "tags": ["office"],
                    "execution_role": "full"
                },
                "paired_nodes": 0,
                "load": {
                    "pending_tasks": 0,
                    "running_tasks": 0,
                    "recent_failures": 0,
                    "recent_successes": 12,
                    "recent_avg_duration_ms": 1400
                }
            })))
            .mount(&quiet)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "quiet-task",
                "status": "completed",
                "answer": "quiet result",
                "provider": "ollama",
                "duration_ms": 8,
                "message": null
            })))
            .mount(&quiet)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/remote/status/public"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "enabled": true,
                "node": {
                    "node_id": "busy-node",
                    "node_name": "busy-mac",
                    "public_key": "busy-public"
                },
                "profile": {
                    "capabilities": ["system-execution", "remote-execution"],
                    "tags": ["office"],
                    "execution_role": "full"
                },
                "paired_nodes": 0,
                "load": {
                    "pending_tasks": 7,
                    "running_tasks": 4,
                    "recent_failures": 3,
                    "recent_successes": 1,
                    "recent_avg_duration_ms": 21000
                }
            })))
            .mount(&busy)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "busy-task",
                "status": "completed",
                "answer": "busy result",
                "provider": "ollama",
                "duration_ms": 8,
                "message": null
            })))
            .mount(&busy)
            .await;

        let manager = RemoteManager::new(config);
        manager
            .pair(
                "quiet-mac",
                Some(&quiet.uri()),
                None,
                false,
                &["office".to_string()],
                &["system-execution".to_string()],
            )
            .await
            .expect("pair quiet");
        manager.trust("quiet-mac").expect("trust quiet");
        manager
            .pair(
                "busy-mac",
                Some(&busy.uri()),
                None,
                false,
                &["office".to_string()],
                &["system-execution".to_string()],
            )
            .await
            .expect("pair busy");
        manager.trust("busy-mac").expect("trust busy");

        let result = manager
            .send_with_options(
                "find test.txt on the office machine",
                RemoteSendOptions {
                    node: Some("auto".to_string()),
                    required_capabilities: vec!["system-execution".to_string()],
                    ..RemoteSendOptions::default()
                },
            )
            .await
            .expect("send");

        assert_eq!(result.envelope.target_node, "quiet-mac");
        assert_eq!(result.answer.as_deref(), Some("quiet result"));
    }

    #[tokio::test]
    async fn auto_selection_uses_workspace_and_policy_hints() {
        let (temp, mut config) = test_config();
        config.core.workspace = temp.path().join("office-workspace");
        std::fs::create_dir_all(&config.core.workspace).expect("workspace");
        std::fs::write(
            config.policy.policy_dir().join("shell.toml"),
            r#"[meta]
id = "shell"
name = "shell"
version = "0.1.0"
description = "shell policy"
author = "test"
tags = []
domains = ["general"]

[activation]
manual = true
auto_when = []
conflicts_with = []
apply_only_to = []
auto_when_file_type = []

[directives]
system_prefix = ""
system_suffix = ""

[routing]
preferred_providers = []
avoid_providers = []
always_verify = false

[tools]
prefer = ["run_command"]
suggest_after_code = []

[memory]
auto_tag = ["office-workspace"]

[hints]
"#,
        )
        .expect("write policy");
        *config.policy.default_policies_mut() = vec!["shell".to_string()];

        let office = MockServer::start().await;
        let fallback = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "office-task",
                "status": "completed",
                "answer": "office result",
                "provider": "ollama",
                "duration_ms": 10,
                "message": null
            })))
            .mount(&office)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/remote/execute"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "task_id": "fallback-task",
                "status": "completed",
                "answer": "fallback result",
                "provider": "ollama",
                "duration_ms": 10,
                "message": null
            })))
            .mount(&fallback)
            .await;

        let manager = RemoteManager::new(config);
        manager
            .pair(
                "office-mac",
                Some(&office.uri()),
                None,
                false,
                &["office-workspace".to_string()],
                &["shell-execution".to_string()],
            )
            .await
            .expect("pair office");
        manager.trust("office-mac").expect("trust office");
        manager
            .pair("fallback-mac", Some(&fallback.uri()), None, false, &[], &[])
            .await
            .expect("pair fallback");
        manager.trust("fallback-mac").expect("trust fallback");

        let result = manager
            .send_with_options("search TODO in the repo", RemoteSendOptions::default())
            .await
            .expect("send");

        assert_eq!(result.envelope.target_node, "office-mac");
        assert_eq!(result.answer.as_deref(), Some("office result"));
    }

    #[tokio::test]
    async fn executor_only_send_attaches_direct_execution_plan() {
        use axum::{extract::State, response::IntoResponse};
        use serde_json::Value;
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct CaptureState(Arc<Mutex<Option<Value>>>);

        async fn execute_remote(
            State(state): State<CaptureState>,
            Json(payload): Json<Value>,
        ) -> impl IntoResponse {
            *state.0.lock().expect("capture lock") = Some(payload);
            Json(serde_json::json!({
                "success": true,
                "task_id": "direct-plan-task",
                "status": "completed",
                "answer": "found file",
                "provider": "executor-plan",
                "duration_ms": 4,
                "message": null
            }))
        }

        let (_temp, config) = test_config();
        let captured = Arc::new(Mutex::new(None));
        let state = CaptureState(Arc::clone(&captured));
        let app = Router::new()
            .route("/v1/remote/execute", post(execute_remote))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let manager = RemoteManager::new(config);
        manager
            .pair(
                "office-mac",
                Some(&format!("http://{}", addr)),
                None,
                true,
                &["office".to_string()],
                &[
                    "shell-execution".to_string(),
                    "system-execution".to_string(),
                ],
            )
            .await
            .expect("pair");
        manager.trust("office-mac").expect("trust");

        let result = manager
            .send_with_options(
                "find test.txt in the repo",
                RemoteSendOptions {
                    node: Some("office-mac".to_string()),
                    allow_executor_only: true,
                    ..RemoteSendOptions::default()
                },
            )
            .await
            .expect("send");

        let request = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured request");
        assert_eq!(
            request
                .get("plan")
                .and_then(|value| value.get("steps"))
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("tool_name"))
                .and_then(|value| value.as_str()),
            Some("run_command")
        );
        assert!(request
            .get("plan")
            .and_then(|value| value.get("steps"))
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("tool_args"))
            .and_then(|value| value.get("command"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .contains("fd -a"));
        assert!(result.envelope.execution_plan.is_some());
        assert_eq!(result.answer.as_deref(), Some("found file"));

        server.abort();
    }

    #[tokio::test]
    async fn plan_execution_bundle_appends_verification_steps_for_mutating_prompt() {
        let (_temp, config) = test_config();
        let manager = RemoteManager::new(config);

        let plan = manager
            .plan_execution_bundle(
                "read `Cargo.toml` and update the dependency version",
                &["cargo check".to_string(), "cargo test -q".to_string()],
            )
            .await
            .expect("bundle planning")
            .expect("bundle should be produced");

        let steps = plan.steps();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].tool_name, "read_file");
        assert_eq!(steps[1].tool_name, "run_command");
        assert_eq!(steps[2].tool_name, "run_command");
    }

    /// 2-node E2E: node A signs a remote execution request; node B's HTTP handler
    /// validates the signature with its own `verify_signed_request` before responding.
    /// This exercises the full sign → transmit → verify → respond path without a
    /// full daemon, using two in-process `RemoteManager` instances backed by real
    /// on-disk identity files.
    #[tokio::test]
    async fn two_node_signed_execution_e2e() {
        use axum::{extract::Json as AJson, extract::State, response::IntoResponse};
        use std::sync::{Arc, Mutex};

        // ── Build two separate node configs ─────────────────────────────────
        let temp = TempDir::new().expect("temp dir");
        let config_a = secondary_config(&temp, "node-a");
        let config_b = secondary_config(&temp, "node-b");

        let node_a = RemoteManager::new(config_a.clone());
        let node_b = Arc::new(RemoteManager::new(config_b.clone()));

        // ── Node B serves /v1/remote/handshake + /v1/remote/execute ─────────
        #[derive(Clone)]
        struct NodeBState {
            manager: Arc<RemoteManager>,
            received: Arc<Mutex<Vec<(bool, String)>>>,
        }

        async fn handshake_handler(
            State(state): State<NodeBState>,
            AJson(body): AJson<serde_json::Value>,
        ) -> impl IntoResponse {
            let challenge = body
                .get("challenge")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let proof = state.manager.sign_handshake(challenge).expect("sign");
            AJson(proof)
        }

        async fn execute_handler(
            State(state): State<NodeBState>,
            headers: axum::http::HeaderMap,
            AJson(payload): AJson<serde_json::Value>,
        ) -> impl IntoResponse {
            let task_id = payload
                .get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let verified = state
                .manager
                .verify_signed_request(&headers, "execute", Some(task_id))
                .is_ok();
            let input = payload
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            state
                .received
                .lock()
                .expect("lock")
                .push((verified, input));
            AJson(serde_json::json!({
                "success": true,
                "task_id": task_id,
                "status": "completed",
                "answer": "e2e-result",
                "provider": "test",
                "duration_ms": 1,
                "message": null
            }))
        }

        let received: Arc<Mutex<Vec<(bool, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let b_state = NodeBState {
            manager: Arc::clone(&node_b),
            received: Arc::clone(&received),
        };

        let app = axum::Router::new()
            .route(
                "/v1/remote/handshake",
                axum::routing::post(handshake_handler),
            )
            .route(
                "/v1/remote/execute",
                axum::routing::post(execute_handler),
            )
            .with_state(b_state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        // ── A pairs with B and trusts it ────────────────────────────────────
        // Register B's public key in A's peer list so signature verification works.
        let paired = node_a
            .pair(
                "node-b",
                Some(&format!("http://{}", addr)),
                None,
                false,
                &[],
                &[],
            )
            .await
            .expect("pair");
        let b_actual_name = paired.identity.node_name.clone();
        node_a.trust(&b_actual_name).expect("trust");

        // Also register A's public key in B's peer list so B can verify A's sig.
        let status_a = node_a.status().expect("status a");
        node_b
            .upsert_verified_peer(
                status_a.node.clone(),
                status_a.profile.clone(),
                &format!("http://{}", addr),
                vec![],
                true,
            )
            .expect("register a on b");

        // ── A sends a task to B ──────────────────────────────────────────────
        let result = node_a
            .send(&b_actual_name, "list files in workspace")
            .await
            .expect("send");

        // ── Assertions ───────────────────────────────────────────────────────
        assert_eq!(result.status, "completed");
        assert_eq!(result.answer.as_deref(), Some("e2e-result"));

        let calls = received.lock().expect("lock");
        assert_eq!(calls.len(), 1, "exactly one execute call reached node B");
        let (sig_ok, input) = &calls[0];
        assert!(sig_ok, "node B must verify node A's signature");
        assert_eq!(input, "list files in workspace");

        server.abort();
    }
}
