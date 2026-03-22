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
use crate::policy::{infer_domain, PolicyExplainReport, PolicyManager};
use crate::secrets::SecretManager;
use sdk::{
    NodeExecutionRole, NodeIdentity, NodeLoadSnapshot, NodeProfile, RemoteEnvelope,
    RemoteExecutionPlan,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePeer {
    pub identity: NodeIdentity,
    pub profile: NodeProfile,
    pub target: String,
    pub trusted: bool,
    pub auth_secret_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteStatus {
    pub enabled: bool,
    pub node: NodeIdentity,
    pub profile: NodeProfile,
    pub paired_nodes: usize,
    pub load: Option<NodeLoadSnapshot>,
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
            load: None,
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
        let execution_plan = if matches!(peer.profile.execution_role, NodeExecutionRole::ExecutorOnly)
        {
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

        let auth_token = self.auth_token_for_peer(&peer).await?;
        let client = Client::new();
        let endpoint = format!("{}/api/v1/remote/execute", peer.target.trim_end_matches('/'));
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

    async fn select_peer(
        &self,
        peers: &[RemotePeer],
        prompt: &str,
        selection: &SelectionContext,
        options: &RemoteSendOptions,
    ) -> Result<RemotePeer> {
        let prompt_lower = prompt.to_ascii_lowercase();
        let live_candidates = self
            .load_peer_selection_candidates(peers)
            .await;
        let mut candidates = live_candidates
            .into_iter()
            .filter(|peer| peer.peer.trusted)
            .filter(|peer| {
                options.allow_executor_only
                    || !matches!(peer.peer.profile.execution_role, NodeExecutionRole::ExecutorOnly)
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
                    if peer.peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(workspace_name))
                    {
                        score += 35;
                    }
                }
                if let Some(domain_tag) = &selection.domain_tag {
                    if peer.peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(domain_tag))
                    {
                        score += 15;
                    }
                }
                for tag in &selection.policy_tags {
                    if peer.peer
                        .profile
                        .tags
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(tag))
                    {
                        score += 10;
                    }
                }
                for capability in &selection.preferred_capabilities {
                    if peer.peer
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
            .ok_or_else(|| anyhow::anyhow!("No trusted remote node matches the requested selection."))
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
        let auth_token = self.auth_token_for_peer(peer).await?;
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .context("Failed to build remote status client")?;
        let endpoint = format!("{}/api/v1/remote/status", peer.target.trim_end_matches('/'));
        let response = client
            .get(endpoint)
            .bearer_auth(auth_token)
            .send()
            .await
            .context("Failed to fetch remote node status")?;
        parse_remote_status_response(response).await
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
        let Ok(report) = PolicyManager::new(self.config.clone(), None).explain(prompt).await else {
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

    #[tokio::test]
    async fn auto_selection_prefers_lower_load_when_capabilities_match() {
        let (_temp, config) = test_config();
        let quiet = MockServer::start().await;
        let busy = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/remote/status"))
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
            .and(path("/api/v1/remote/execute"))
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
            .and(path("/api/v1/remote/status"))
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
            .and(path("/api/v1/remote/execute"))
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

        Mock::given(method("POST"))
            .and(path("/api/v1/remote/execute"))
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
            .route("/api/v1/remote/execute", post(execute_remote))
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
                &["shell-execution".to_string(), "system-execution".to_string()],
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
}
