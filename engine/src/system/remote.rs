use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use crate::config::metadata::SERVICE_NAME;
use crate::config::Config;
use crate::secrets::SecretManager;
use sdk::{NodeExecutionRole, NodeIdentity, NodeProfile, RemoteEnvelope};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePeer {
    pub identity: NodeIdentity,
    pub profile: NodeProfile,
    pub target: String,
    pub trusted: bool,
    pub auth_secret_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteStatus {
    pub enabled: bool,
    pub node: NodeIdentity,
    pub profile: NodeProfile,
    pub paired_nodes: usize,
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

pub struct RemoteManager {
    config: Config,
}

impl RemoteManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn status(&self) -> Result<RemoteStatus> {
        let node = self.load_or_init_node_metadata()?;
        let peers = self.load_peers()?;
        Ok(RemoteStatus {
            enabled: self.config.ws_client.enabled,
            node: node.identity,
            profile: node.profile,
            paired_nodes: peers.len(),
        })
    }

    pub fn nodes(&self) -> Result<Vec<RemotePeer>> {
        self.load_peers()
    }

    pub fn rename(&self, name: &str) -> Result<NodeIdentity> {
        let path = self.remote_node_file();
        let mut metadata = self.load_or_init_node_metadata()?;
        metadata.identity.node_name = name.to_string();
        self.save_node_metadata(&path, &metadata)?;
        Ok(metadata.identity)
    }

    pub fn local_profile(&self) -> Result<NodeProfile> {
        Ok(self.load_or_init_node_metadata()?.profile)
    }

    pub fn set_execution_role(&self, execution_role: NodeExecutionRole) -> Result<NodeProfile> {
        let path = self.remote_node_file();
        let mut metadata = self.load_or_init_node_metadata()?;
        metadata.profile.execution_role = execution_role;
        self.save_node_metadata(&path, &metadata)?;
        Ok(metadata.profile)
    }

    pub fn replace_tags(&self, tags: &[String]) -> Result<NodeProfile> {
        let path = self.remote_node_file();
        let mut metadata = self.load_or_init_node_metadata()?;
        metadata.profile.tags = tags.to_vec();
        self.save_node_metadata(&path, &metadata)?;
        Ok(metadata.profile)
    }

    pub fn replace_capabilities(&self, capabilities: &[String]) -> Result<NodeProfile> {
        let path = self.remote_node_file();
        let mut metadata = self.load_or_init_node_metadata()?;
        metadata.profile.capabilities = normalize_capabilities(capabilities);
        self.save_node_metadata(&path, &metadata)?;
        Ok(metadata.profile)
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
        let (node_name, endpoint) = resolve_pair_target(target, url)?;

        if peers.iter().any(|peer| {
            peer.identity.node_name.eq_ignore_ascii_case(&node_name)
                || peer.target.eq_ignore_ascii_case(&endpoint)
                || peer.identity.node_id == target
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

        let peer = RemotePeer {
            identity: NodeIdentity {
                node_id: Uuid::new_v4().to_string(),
                node_name: node_name.clone(),
                public_key: Uuid::new_v4().simple().to_string(),
            },
            profile: NodeProfile {
                capabilities: normalize_capabilities(capabilities),
                tags: tags.to_vec(),
                execution_role: if executor_only {
                    NodeExecutionRole::ExecutorOnly
                } else {
                    NodeExecutionRole::Full
                },
            },
            target: endpoint,
            trusted: false,
            auth_secret_key,
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
            if peer.identity.node_name == name || peer.target == name || peer.identity.node_id == name
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
        let peer = self.resolve_peer(
            &peers,
            prompt,
            &RemoteSendOptions {
                node: Some(node.to_string()),
                ..RemoteSendOptions::default()
            },
        )?;
        let coordinator = self.load_or_init_node_metadata()?;
        let envelope = RemoteEnvelope {
            origin_node: coordinator.identity.node_name.clone(),
            target_node: peer.identity.node_name.clone(),
            coordinator_node: coordinator.identity.node_name.clone(),
            task_id: Uuid::new_v4().to_string(),
            task_input: prompt.to_string(),
            stream_policy: "events+result".to_string(),
        };

        Ok(RemoteSendPreview {
            envelope,
            trusted: peer.trusted,
            message: "Remote transport ready. The coordinator will submit to the target daemon and stream task events before the final result.".to_string(),
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
        let peer = self.resolve_peer(&peers, prompt, &options)?;
        if !peer.trusted {
            bail!(
                "Remote node '{}' is paired but not trusted. Run `rove remote trust {}` first.",
                peer.identity.node_name,
                peer.identity.node_name
            );
        }

        let coordinator = self.load_or_init_node_metadata()?;
        let envelope = RemoteEnvelope {
            origin_node: coordinator.identity.node_name.clone(),
            target_node: peer.identity.node_name.clone(),
            coordinator_node: coordinator.identity.node_name.clone(),
            task_id: Uuid::new_v4().to_string(),
            task_input: prompt.to_string(),
            stream_policy: "events+result".to_string(),
        };

        let auth_token = self.auth_token_for_peer(&peer).await?;
        let client = Client::new();
        let endpoint = format!("{}/api/v1/remote/execute", peer.target.trim_end_matches('/'));
        let request = RemoteExecuteRequest {
            input: Some(prompt.to_string()),
            task: None,
            origin_node: Some(envelope.origin_node.clone()),
            coordinator_node: Some(envelope.coordinator_node.clone()),
            workspace: None,
            team_id: None,
            wait_seconds: Some(1),
        };

        let execute = client
            .post(endpoint)
            .bearer_auth(auth_token.clone())
            .json(&request)
            .send()
            .await
            .context("Failed to reach remote daemon")?;

        let execute = parse_remote_response(execute).await?;
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
            .stream_remote_events(&peer, &auth_token, &remote_task_id)
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
                    self.poll_remote_completion(&client, &peer, &auth_token, &remote_task_id)
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

    fn resolve_peer(
        &self,
        peers: &[RemotePeer],
        prompt: &str,
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

        self.select_peer(peers, prompt, options)
    }

    fn validate_peer_selection(&self, peer: &RemotePeer, options: &RemoteSendOptions) -> Result<()> {
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
            if !peer.profile.tags.iter().any(|value| value.eq_ignore_ascii_case(tag)) {
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

    fn select_peer(
        &self,
        peers: &[RemotePeer],
        prompt: &str,
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
                if options.prefer_executor_only {
                    score += match peer.profile.execution_role {
                        NodeExecutionRole::ExecutorOnly => 25,
                        NodeExecutionRole::Full => 5,
                    };
                } else {
                    score += match peer.profile.execution_role {
                        NodeExecutionRole::Full => 25,
                        NodeExecutionRole::ExecutorOnly => 5,
                    };
                }
                score += peer.profile.capabilities.len() as i64;
                (score, peer.clone())
            })
            .collect::<Vec<_>>();

        candidates.sort_by(|left, right| right.0.cmp(&left.0));

        candidates
            .into_iter()
            .map(|(_, peer)| peer)
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

    async fn stream_remote_events(
        &self,
        peer: &RemotePeer,
        auth_token: &str,
        task_id: &str,
    ) -> Result<(Vec<RemoteTaskEvent>, RemoteExecuteResponse)> {
        let mut request = websocket_task_url(&peer.target)?
            .into_client_request()
            .context("Failed to prepare remote WebSocket request")?;
        let header = HeaderValue::from_str(&format!("Bearer {}", auth_token))
            .context("Invalid remote bearer token")?;
        request.headers_mut().insert(AUTHORIZATION, header);

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
        auth_token: &str,
        task_id: &str,
    ) -> Result<RemoteExecuteResponse> {
        let deadline = Instant::now() + Duration::from_secs(120);
        let status_url = format!(
            "{}/api/v1/tasks/{}",
            peer.target.trim_end_matches('/'),
            task_id
        );

        loop {
            let response = client
                .get(&status_url)
                .bearer_auth(auth_token)
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
                        message: Some("Remote task is still running; polling timed out after 120s".to_string()),
                    });
                }
                _ => return Ok(completion),
            }
        }
    }

    async fn auth_token_for_peer(&self, peer: &RemotePeer) -> Result<String> {
        if let Some(secret_key) = &peer.auth_secret_key {
            return SecretManager::new(SERVICE_NAME)
                .get_secret(secret_key)
                .await
                .with_context(|| {
                    format!(
                        "Missing auth token for remote node '{}'. Pair again with `--token` or restore secret '{}'.",
                        peer.identity.node_name, secret_key
                    )
                });
        }

        self.config
            .ws_client
            .auth_token
            .clone()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Remote node '{}' has no stored auth token. Pair again with `--token`.",
                    peer.identity.node_name
                )
            })
    }

    fn remote_node_file(&self) -> PathBuf {
        self.config.core.data_dir.join("remote-node.toml")
    }

    fn remote_peers_file(&self) -> PathBuf {
        self.config.core.data_dir.join("remote-peers.toml")
    }

    fn load_or_init_node_metadata(&self) -> Result<RemoteNodeMetadata> {
        let path = self.remote_node_file();
        if path.exists() {
            let raw = fs::read_to_string(&path)?;
            return Ok(toml::from_str(&raw)?);
        }

        let metadata = RemoteNodeMetadata {
            identity: NodeIdentity {
                node_id: Uuid::new_v4().to_string(),
                node_name: self.default_node_name(),
                public_key: Uuid::new_v4().simple().to_string(),
            },
            profile: NodeProfile {
                capabilities: vec![
                    "task-routing".to_string(),
                    "remote-execution".to_string(),
                    "system-execution".to_string(),
                ],
                tags: vec![std::env::consts::OS.to_string()],
                execution_role: NodeExecutionRole::Full,
            },
        };
        self.save_node_metadata(&path, &metadata)?;
        Ok(metadata)
    }

    fn save_node_metadata(&self, path: &Path, metadata: &RemoteNodeMetadata) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(metadata)?)?;
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

    fn default_node_name(&self) -> String {
        self.config
            .core
            .workspace
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("local-node")
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteNodeMetadata {
    identity: NodeIdentity,
    profile: NodeProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemotePeersFile {
    peers: Vec<RemotePeer>,
}

#[derive(Debug, Clone, Serialize)]
struct RemoteExecuteRequest {
    input: Option<String>,
    task: Option<String>,
    origin_node: Option<String>,
    coordinator_node: Option<String>,
    workspace: Option<String>,
    team_id: Option<String>,
    wait_seconds: Option<u64>,
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
    Connected { version: String },
    Accepted { task_id: String },
    Progress { message: String },
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
    Error { message: String },
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
            parsed
                .message
                .unwrap_or_else(|| format!("status {} {}", status_code.as_u16(), parsed.status))
        );
    }
    Ok(parsed)
}

fn resolve_pair_target(target: &str, url: Option<&str>) -> Result<(String, String)> {
    match url {
        Some(url) => Ok((target.to_string(), normalize_base_url(url)?)),
        None if target.starts_with("http://") || target.starts_with("https://") => {
            Ok((derive_node_name(target), normalize_base_url(target)?))
        }
        None => bail!(
            "Pairing requires a daemon URL. Use `rove remote pair office-mac --url http://host:3727 --token ...` or pass the URL directly as the target."
        ),
    }
}

fn normalize_base_url(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url)
        .with_context(|| format!("Invalid remote daemon URL '{}'", url))?;
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

fn websocket_task_url(base_url: &str) -> Result<String> {
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
    url.set_path("/ws/task");
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

fn secret_key_fragment(node_name: &str) -> String {
    node_name
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
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
        config.ws_client.enabled = true;
        config.ws_client.auth_token = Some("remote-token".to_string());
        (temp, config)
    }

    #[tokio::test]
    async fn send_to_trusted_node_polls_until_completion() {
        let (_temp, config) = test_config();
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/remote/execute"))
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
            .and(path("/api/v1/tasks/remote-task-1"))
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
            .route("/api/v1/remote/execute", post(execute_remote))
            .route("/ws/task", get(ws_task));
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
        assert_eq!(nodes[0].profile.execution_role, NodeExecutionRole::ExecutorOnly);
        assert_eq!(nodes[0].profile.tags, vec!["office".to_string()]);
        assert!(
            nodes[0]
                .profile
                .capabilities
                .contains(&"system-execution".to_string())
        );
    }

    #[tokio::test]
    async fn auto_selection_prefers_matching_tagged_full_node() {
        let (_temp, config) = test_config();
        let office = MockServer::start().await;
        let lab = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/remote/execute"))
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
}
