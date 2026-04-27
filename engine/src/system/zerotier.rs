use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::cli::database_path::database_path;
use crate::config::metadata::{DEFAULT_PORT, SERVICE_NAME};
use crate::config::Config;
use crate::remote::{RemoteHandshakeProof, RemoteManager};
use crate::secrets::SecretManager;
use crate::storage::Database;
use sdk::{RemoteDiscoveryCandidate, RemoteTransportRecord, ZeroTierStatus};

const DEFAULT_TOKEN_KEY: &str = "zerotier_api_token";
const SYNC_INTERVAL_SECS: u64 = 60;

pub struct ZeroTierManager {
    config: Config,
}

impl ZeroTierManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn status(&self) -> Result<ZeroTierStatus> {
        let settings = &self.config.remote.transports.zerotier;
        let token_key = self.token_key();
        let token = self.load_api_token().await;
        let (candidate_count, last_sync_at) = self.discovery_stats().await.unwrap_or((0, None));
        let installed = zerotier_installed();

        let mut status = ZeroTierStatus {
            enabled: settings.enabled,
            installed,
            configured: settings.network_id.is_some(),
            token_configured: token.is_some(),
            service_url: settings.service_url.clone(),
            network_id: settings.network_id.clone(),
            managed_name_sync: settings.managed_name_sync,
            service_online: false,
            joined: false,
            controller_access: false,
            node_id: None,
            network_name: None,
            network_status: None,
            assigned_addresses: Vec::new(),
            transport_records: Vec::new(),
            last_sync_at,
            candidate_count,
            sync_state: if settings.enabled {
                "idle".to_string()
            } else {
                "disabled".to_string()
            },
            message: None,
        };

        if !settings.enabled {
            status.message = Some("ZeroTier transport is disabled.".to_string());
            return Ok(status);
        }
        if !installed {
            status.sync_state = "not_installed".to_string();
            status.message = Some(
                "ZeroTier transport is enabled, but ZeroTierOne is not installed yet.".to_string(),
            );
            return Ok(status);
        }
        let Some(token) = token else {
            status.sync_state = "missing_token".to_string();
            status.message = Some(format!(
                "Missing ZeroTier API token secret '{}'.",
                token_key
            ));
            return Ok(status);
        };

        let client = self.client(Duration::from_secs(2))?;
        match self
            .request_json(&client, "/status", reqwest::Method::GET, None, &token)
            .await
        {
            Ok(value) => {
                status.service_online = true;
                status.node_id = extract_string(&value, &["address", "nodeId", "id"]);
            }
            Err(error) => {
                status.sync_state = "service_unreachable".to_string();
                status.message = Some(format!("ZeroTier service is unreachable: {}", error));
                return Ok(status);
            }
        }

        let Some(network_id) = settings.network_id.as_deref() else {
            status.sync_state = "unconfigured".to_string();
            status.message =
                Some("ZeroTier is enabled but no network id is configured.".to_string());
            return Ok(status);
        };

        match self
            .request_json(
                &client,
                &format!("/network/{}", network_id),
                reqwest::Method::GET,
                None,
                &token,
            )
            .await
        {
            Ok(value) => {
                status.joined = true;
                status.network_name = extract_string(&value, &["name"]);
                status.network_status = extract_string(&value, &["status"]);
                status.assigned_addresses =
                    extract_string_list(&value, &["assignedAddresses", "assigned_addresses"]);
                status.transport_records =
                    self.transport_records_from_addresses(network_id, &status.assigned_addresses);
                status.controller_access = self
                    .request_json(
                        &client,
                        &format!("/controller/network/{}/member", network_id),
                        reqwest::Method::GET,
                        None,
                        &token,
                    )
                    .await
                    .is_ok();
                status.sync_state = if status.controller_access {
                    "ready".to_string()
                } else {
                    "controller_unavailable".to_string()
                };
                if status.transport_records.is_empty() {
                    status.message = Some(
                        "ZeroTier network is joined, but the daemon is still bound to localhost only."
                            .to_string(),
                    );
                }
            }
            Err(error) => {
                status.sync_state = "not_joined".to_string();
                status.message = Some(format!(
                    "ZeroTier network '{}' is not joined yet: {}",
                    network_id, error
                ));
            }
        }

        Ok(status)
    }

    pub async fn install(&self) -> Result<ZeroTierStatus> {
        install_zerotier_one()?;
        start_zerotier_service()?;
        self.ensure_default_api_token_secret().await?;
        let mut config = self.config.clone();
        config.remote.transports.zerotier.enabled = true;
        config.save()?;
        Self::new(config).status().await
    }

    pub async fn uninstall(&self) -> Result<ZeroTierStatus> {
        let mut config = self.config.clone();
        config.remote.transports.zerotier.enabled = false;
        config.save()?;

        let database = Database::new(&database_path(&config)).await?;
        database
            .remote_discovery()
            .prune_transport_candidates("zerotier", None, &[])
            .await?;

        let mut status = Self::new(config).status().await?;
        status.message = Some(
            "Disabled the Rove ZeroTier transport integration. The ZeroTierOne package remains installed."
                .to_string(),
        );
        Ok(status)
    }

    pub async fn setup(
        &self,
        network_id: &str,
        api_token_key: Option<&str>,
        managed_name_sync: bool,
    ) -> Result<ZeroTierStatus> {
        let network_id = network_id.trim();
        if network_id.is_empty() {
            bail!("ZeroTier setup requires a non-empty network id");
        }

        let mut config = self.config.clone();
        config.remote.transports.zerotier.enabled = true;
        config.remote.transports.zerotier.network_id = Some(network_id.to_string());
        config.remote.transports.zerotier.managed_name_sync = managed_name_sync;
        config.remote.transports.zerotier.api_token_key = Some(
            api_token_key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(DEFAULT_TOKEN_KEY)
                .to_string(),
        );
        config.save()?;

        Self::new(config).ensure_default_api_token_secret().await?;
        Self::new(Config::load_or_create()?).status().await
    }

    pub async fn join(&self, network_id_override: Option<&str>) -> Result<ZeroTierStatus> {
        let settings = &self.config.remote.transports.zerotier;
        if !settings.enabled {
            return Ok(ZeroTierStatus {
                enabled: false,
                sync_state: "disabled".to_string(),
                message: Some("ZeroTier transport is disabled.".to_string()),
                ..ZeroTierStatus::default()
            });
        }

        let network_id = network_id_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or(settings.network_id.as_deref())
            .ok_or_else(|| anyhow::anyhow!("No ZeroTier network id is configured"))?;
        let token = self.load_api_token().await.ok_or_else(|| {
            anyhow::anyhow!("Missing ZeroTier API token secret '{}'", self.token_key())
        })?;

        let client = self.client(Duration::from_secs(3))?;
        let body = serde_json::json!({
            "allowManaged": true,
            "allowGlobal": false,
            "allowDefault": false,
            "allowDNS": true,
        });
        let _ = self
            .request_json(
                &client,
                &format!("/network/{}", network_id),
                reqwest::Method::POST,
                Some(body),
                &token,
            )
            .await?;
        self.status().await
    }

    pub async fn refresh(&self) -> Result<ZeroTierStatus> {
        let mut status = self.status().await?;
        if !status.enabled || !status.installed || !status.service_online {
            return Ok(status);
        }
        if status.configured && !status.joined {
            status = self.join(status.network_id.as_deref()).await?;
        }
        if status.joined {
            self.sync_candidates(&mut status).await?;
        }
        Ok(status)
    }

    pub async fn transport_records(&self) -> Result<Vec<RemoteTransportRecord>> {
        Ok(self.status().await?.transport_records)
    }

    pub async fn list_candidates(&self) -> Result<Vec<RemoteDiscoveryCandidate>> {
        let database = Database::new(&database_path(&self.config)).await?;
        database
            .remote_discovery()
            .list_candidates("zerotier")
            .await
    }

    pub async fn trust_candidate(&self, candidate_id: &str) -> Result<RemoteDiscoveryCandidate> {
        let _ = self.refresh().await?;
        let database = Database::new(&database_path(&self.config)).await?;
        let Some(candidate) = database
            .remote_discovery()
            .get_candidate(candidate_id)
            .await?
        else {
            bail!(
                "ZeroTier discovery candidate '{}' was not found",
                candidate_id
            );
        };
        if candidate.trusted {
            return Ok(candidate);
        }
        let Some(identity) = candidate.identity.clone() else {
            bail!(
                "ZeroTier discovery candidate '{}' has not completed Rove identity validation yet",
                candidate_id
            );
        };
        let profile = candidate.profile.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "Candidate '{}' is missing a verified node profile",
                candidate_id
            )
        })?;
        let target = best_base_url(&candidate.transports).ok_or_else(|| {
            anyhow::anyhow!(
                "Candidate '{}' does not advertise a reachable daemon URL",
                candidate_id
            )
        })?;
        let peer = RemoteManager::new(self.config.clone()).upsert_verified_peer(
            identity,
            profile,
            &target,
            candidate.transports.clone(),
            true,
        )?;
        let updated = RemoteDiscoveryCandidate {
            paired_node_name: Some(peer.identity.node_name.clone()),
            trusted: true,
            ..candidate
        };
        database
            .remote_discovery()
            .upsert_candidate(&updated)
            .await?;
        Ok(updated)
    }

    pub fn sync_interval(&self) -> Duration {
        Duration::from_secs(SYNC_INTERVAL_SECS)
    }

    async fn sync_candidates(&self, status: &mut ZeroTierStatus) -> Result<()> {
        let Some(network_id) = status.network_id.clone() else {
            status.sync_state = "unconfigured".to_string();
            return Ok(());
        };
        let Some(token) = self.load_api_token().await else {
            status.sync_state = "missing_token".to_string();
            return Ok(());
        };

        let client = self.client(Duration::from_secs(3))?;
        let members = match self
            .request_json(
                &client,
                &format!("/controller/network/{}/member", network_id),
                reqwest::Method::GET,
                None,
                &token,
            )
            .await
        {
            Ok(value) => {
                status.controller_access = true;
                value
            }
            Err(error) => {
                status.controller_access = false;
                status.sync_state = "controller_unavailable".to_string();
                status.message = Some(format!(
                    "ZeroTier controller member discovery is unavailable: {}",
                    error
                ));
                return Ok(());
            }
        };

        let local_member_id = status.node_id.clone().unwrap_or_default();
        let database = Database::new(&database_path(&self.config)).await?;
        let repo = database.remote_discovery();
        let remote_manager = RemoteManager::new(self.config.clone());
        let now = now_ts()?;
        let mut keep_ids = Vec::new();

        for member_id in controller_member_ids(&members) {
            if member_id == local_member_id {
                continue;
            }
            let detail = match self
                .request_json(
                    &client,
                    &format!("/controller/network/{}/member/{}", network_id, member_id),
                    reqwest::Method::GET,
                    None,
                    &token,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!(member_id = %member_id, error = %error, "Failed to fetch ZeroTier member detail");
                    continue;
                }
            };

            let candidate_id = format!("zerotier:{}:{}", network_id, member_id);
            let member_name = extract_string(&detail, &["name", "description"]);
            let assigned_addresses = member_addresses(&detail);
            let transports =
                self.transport_records_from_addresses(&network_id, &assigned_addresses);
            let (transport_records, proof) = self.probe_candidate(&transports).await;

            let mut identity = None;
            let mut profile = None;
            let mut node_name_hint = member_name.clone();
            let mut paired_node_name = None;
            let mut trusted = false;

            if let Some(proof) = proof {
                node_name_hint = Some(proof.identity.node_name.clone());
                identity = Some(proof.identity.clone());
                profile = Some(proof.profile.clone());
                if let Some(base_url) = best_base_url(&transport_records) {
                    match remote_manager.upsert_verified_peer(
                        proof.identity.clone(),
                        proof.profile.clone(),
                        &base_url,
                        transport_records.clone(),
                        true,
                    ) {
                        Ok(peer) => {
                            paired_node_name = Some(peer.identity.node_name.clone());
                            trusted = peer.trusted;
                        }
                        Err(error) => {
                            tracing::warn!(
                                node = %proof.identity.node_name,
                                error = %error,
                                "Failed to auto-promote ZeroTier candidate into paired peer"
                            );
                        }
                    }
                }
            }

            let candidate = RemoteDiscoveryCandidate {
                candidate_id: candidate_id.clone(),
                transport_kind: "zerotier".to_string(),
                network_id: Some(network_id.clone()),
                member_id: member_id.clone(),
                member_name,
                node_name_hint,
                identity,
                profile,
                assigned_addresses,
                last_seen_at: now,
                controller_access: true,
                paired_node_name,
                trusted,
                transports: transport_records,
            };
            repo.upsert_candidate(&candidate).await?;
            keep_ids.push(candidate_id);
        }

        repo.prune_transport_candidates("zerotier", Some(&network_id), &keep_ids)
            .await?;
        status.last_sync_at = Some(now);
        status.candidate_count = keep_ids.len();

        if status.managed_name_sync {
            match self
                .try_sync_local_member_name(&client, &network_id, &token, status.node_id.as_deref())
                .await
            {
                Ok(true) => status.sync_state = "synced".to_string(),
                Ok(false) => status.sync_state = "name_sync_unavailable".to_string(),
                Err(error) => {
                    status.sync_state = "name_sync_degraded".to_string();
                    status.message =
                        Some(format!("ZeroTier managed-name sync is degraded: {}", error));
                }
            }
        } else {
            status.sync_state = "synced".to_string();
        }

        Ok(())
    }

    async fn probe_candidate(
        &self,
        transports: &[RemoteTransportRecord],
    ) -> (Vec<RemoteTransportRecord>, Option<RemoteHandshakeProof>) {
        let client = match self.client(Duration::from_secs(2)) {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(error = %error, "Failed to build ZeroTier discovery probe client");
                return (transports.to_vec(), None);
            }
        };

        let mut updated = Vec::with_capacity(transports.len());
        let mut proof = None;

        for record in transports {
            let mut record = record.clone();
            record.last_checked_at = Some(now_ts().unwrap_or_default());
            if let Some(base_url) = record.base_url.clone() {
                let started = Instant::now();
                let hello = client
                    .get(format!("{}/v1/hello", base_url.trim_end_matches('/')))
                    .send()
                    .await;
                match hello {
                    Ok(response) if response.status().is_success() => {
                        record.reachable = true;
                        record.latency_ms = Some(started.elapsed().as_millis() as u64);
                        record.last_error = None;
                        if proof.is_none() {
                            match RemoteManager::fetch_remote_handshake(&base_url).await {
                                Ok(value) => proof = Some(value),
                                Err(error) => {
                                    record.last_error = Some(error.to_string());
                                }
                            }
                        }
                    }
                    Ok(response) => {
                        record.reachable = false;
                        record.last_error = Some(format!("status {}", response.status()));
                    }
                    Err(error) => {
                        record.reachable = false;
                        record.last_error = Some(error.to_string());
                    }
                }
            } else {
                record.reachable = false;
                record.last_error = Some("No daemon base URL available".to_string());
            }
            updated.push(record);
        }

        (updated, proof)
    }

    async fn try_sync_local_member_name(
        &self,
        client: &reqwest::Client,
        network_id: &str,
        token: &str,
        member_id: Option<&str>,
    ) -> Result<bool> {
        let Some(member_id) = member_id.filter(|value| !value.trim().is_empty()) else {
            return Ok(false);
        };
        let node_name = RemoteManager::new(self.config.clone())
            .identity_status()?
            .identity
            .node_name;
        let result = self
            .request_json(
                client,
                &format!("/controller/network/{}/member/{}", network_id, member_id),
                reqwest::Method::POST,
                Some(serde_json::json!({ "name": node_name })),
                token,
            )
            .await;
        match result {
            Ok(_) => Ok(true),
            Err(error)
                if error.to_string().contains("404") || error.to_string().contains("405") =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    async fn discovery_stats(&self) -> Result<(usize, Option<i64>)> {
        let database = Database::new(&database_path(&self.config)).await?;
        let candidates = database
            .remote_discovery()
            .list_candidates("zerotier")
            .await?;
        let last_sync_at = candidates
            .iter()
            .map(|candidate| candidate.last_seen_at)
            .max();
        Ok((candidates.len(), last_sync_at))
    }

    async fn ensure_default_api_token_secret(&self) -> Result<()> {
        let token_key = self.token_key();
        let manager = SecretManager::new(SERVICE_NAME);
        if manager.lookup_secret(&token_key).await.is_some() {
            return Ok(());
        }
        if let Some(token) = read_local_zerotier_auth_token() {
            manager.set_secret(&token_key, token.trim()).await?;
        }
        Ok(())
    }

    async fn load_api_token(&self) -> Option<String> {
        let manager = SecretManager::new(SERVICE_NAME);
        if let Some((token, _)) = manager.lookup_secret(&self.token_key()).await {
            return Some(token);
        }
        read_local_zerotier_auth_token().map(|token| token.trim().to_string())
    }

    fn token_key(&self) -> String {
        self.config
            .remote
            .transports
            .zerotier
            .api_token_key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_TOKEN_KEY.to_string())
    }

    fn client(&self, timeout: Duration) -> Result<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("Failed to build ZeroTier client")
    }

    async fn request_json(
        &self,
        client: &reqwest::Client,
        path: &str,
        method: reqwest::Method,
        body: Option<Value>,
        token: &str,
    ) -> Result<Value> {
        let base = self
            .config
            .remote
            .transports
            .zerotier
            .service_url
            .trim_end_matches('/');
        let url = format!("{}{}", base, path);
        let mut request = client.request(method, &url).header("X-ZT1-Auth", token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to reach ZeroTier service at {}", url))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("status {}: {}", status, body);
        }
        serde_json::from_str(&body)
            .with_context(|| format!("Invalid JSON returned by ZeroTier service: {}", body))
    }

    fn transport_records_from_addresses(
        &self,
        network_id: &str,
        addresses: &[String],
    ) -> Vec<RemoteTransportRecord> {
        let port = daemon_port(&self.config);
        let daemon_reachable = daemon_reachable_over_network(&self.config);
        addresses
            .iter()
            .filter_map(|entry| entry.split('/').next())
            .filter(|entry| !entry.trim().is_empty())
            .map(|address| RemoteTransportRecord {
                kind: "zerotier".to_string(),
                address: address.to_string(),
                base_url: daemon_reachable.then(|| format!("http://{}:{}", address, port)),
                network_id: Some(network_id.to_string()),
                reachable: daemon_reachable,
                latency_ms: None,
                last_checked_at: None,
                last_error: None,
                iroh_node_id: None,
            })
            .collect()
    }
}

pub async fn maybe_start_sync_loop(config: Config) {
    if !config.remote.transports.zerotier.enabled {
        return;
    }

    tokio::spawn(async move {
        let manager = ZeroTierManager::new(config.clone());
        loop {
            if let Err(error) = manager.refresh().await {
                tracing::warn!(error = %error, "ZeroTier background sync failed");
            }
            tokio::time::sleep(manager.sync_interval()).await;
        }
    });
}

fn now_ts() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock before UNIX_EPOCH")?
        .as_secs() as i64)
}

fn daemon_port(config: &Config) -> u16 {
    parse_bind_addr(&config.webui.bind_addr)
        .map(|addr| addr.port())
        .unwrap_or(DEFAULT_PORT)
}

fn daemon_reachable_over_network(config: &Config) -> bool {
    parse_bind_addr(&config.webui.bind_addr)
        .map(|addr| !addr.ip().is_loopback())
        .unwrap_or(false)
}

fn parse_bind_addr(bind_addr: &str) -> Option<SocketAddr> {
    bind_addr.parse().ok()
}

fn extract_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match value.get(*key) {
        Some(entry) if entry.is_string() => entry.as_str().map(ToOwned::to_owned),
        Some(entry) if entry.is_number() => Some(entry.to_string()),
        _ => None,
    })
}

fn extract_string_list(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| {
            value.get(*key).and_then(|entry| {
                entry.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| match item {
                            Value::String(text) => Some(text.to_string()),
                            Value::Number(number) => Some(number.to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                })
            })
        })
        .unwrap_or_default()
}

fn controller_member_ids(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| extract_string(item, &["nodeId", "node_id", "id", "address"]))
                .collect()
        })
        .unwrap_or_default()
}

fn member_addresses(value: &Value) -> Vec<String> {
    let mut addresses = extract_string_list(
        value,
        &["assignedAddresses", "assigned_addresses", "ipAssignments"],
    );
    if addresses.is_empty() {
        if let Some(config) = value.get("config") {
            addresses = extract_string_list(config, &["assignedAddresses", "ipAssignments"]);
        }
    }
    addresses
}

fn best_base_url(records: &[RemoteTransportRecord]) -> Option<String> {
    records
        .iter()
        .filter(|record| record.reachable)
        .max_by(|left, right| {
            left.reachable
                .cmp(&right.reachable)
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
        .and_then(|record| record.base_url.clone())
}

fn zerotier_installed() -> bool {
    command_exists("zerotier-cli")
        || path_exists("/Applications/ZeroTier One.app")
        || path_exists("/Library/Application Support/ZeroTier/One/zerotier-cli")
        || path_exists("/usr/sbin/zerotier-cli")
        || path_exists("/usr/local/sbin/zerotier-cli")
}

fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn command_exists(command: &str) -> bool {
    #[cfg(target_os = "windows")]
    let checker = ("where", vec![command]);
    #[cfg(not(target_os = "windows"))]
    let checker = ("which", vec![command]);

    Command::new(checker.0)
        .args(&checker.1)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn install_zerotier_one() -> Result<()> {
    if command_exists("brew") {
        run_command("brew", &["install", "--cask", "zerotier-one"])?;
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "ZeroTierOne install requires Homebrew on macOS in this phase"
    ))
}

#[cfg(target_os = "linux")]
fn install_zerotier_one() -> Result<()> {
    run_shell("curl -fsSL https://install.zerotier.com | sudo bash")
}

#[cfg(target_os = "windows")]
fn install_zerotier_one() -> Result<()> {
    run_command(
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            "winget install --id ZeroTier.ZeroTierOne -e --accept-package-agreements --accept-source-agreements",
        ],
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn install_zerotier_one() -> Result<()> {
    bail!("ZeroTierOne install is not implemented on this platform")
}

#[cfg(target_os = "macos")]
fn start_zerotier_service() -> Result<()> {
    let _ = run_command(
        "/bin/launchctl",
        &["kickstart", "-k", "system/com.zerotier.one"],
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn start_zerotier_service() -> Result<()> {
    run_command("systemctl", &["enable", "--now", "zerotier-one"])
}

#[cfg(target_os = "windows")]
fn start_zerotier_service() -> Result<()> {
    run_command("sc", &["start", "ZeroTierOneService"])
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn start_zerotier_service() -> Result<()> {
    Ok(())
}

fn run_command(binary: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run `{}`", binary))?;
    if !output.status.success() {
        bail!(
            "`{}` failed: {}",
            binary,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_shell(command: &str) -> Result<()> {
    let output = Command::new("/bin/sh")
        .args(["-lc", command])
        .output()
        .with_context(|| format!("Failed to run `{}`", command))?;

    if !output.status.success() {
        bail!(
            "Shell command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn read_local_zerotier_auth_token() -> Option<String> {
    zerotier_auth_token_paths()
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn zerotier_auth_token_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Library/Application Support/ZeroTier/One/authtoken.secret",
        ));
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/var/lib/zerotier-one/authtoken.secret"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(program_data) = std::env::var_os("ProgramData") {
            paths.push(
                PathBuf::from(program_data)
                    .join("ZeroTier")
                    .join("One")
                    .join("authtoken.secret"),
            );
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_bind_addr_is_not_remote_reachable() {
        let mut config = Config::default();
        config.webui.bind_addr = format!("127.0.0.1:{}", DEFAULT_PORT);
        assert!(!daemon_reachable_over_network(&config));
    }

    #[test]
    fn zerotier_transport_records_use_daemon_port() {
        let mut config = Config::default();
        config.webui.bind_addr = format!("0.0.0.0:{}", DEFAULT_PORT);
        let manager = ZeroTierManager::new(config);
        let records = manager
            .transport_records_from_addresses("8056c2e21c000001", &["10.10.10.8/24".to_string()]);
        assert_eq!(records.len(), 1);
        let expected = format!("http://10.10.10.8:{}", DEFAULT_PORT);
        assert_eq!(records[0].base_url.as_deref(), Some(expected.as_str()));
        assert!(records[0].reachable);
    }

    #[test]
    fn best_base_url_prefers_low_latency_reachable_record() {
        let url = best_base_url(&[
            RemoteTransportRecord {
                kind: "direct".to_string(),
                address: "example.com".to_string(),
                base_url: Some(format!("http://example.com:{}", DEFAULT_PORT)),
                network_id: None,
                reachable: true,
                latency_ms: Some(40),
                last_checked_at: Some(2),
                last_error: None,
                iroh_node_id: None,
            },
            RemoteTransportRecord {
                kind: "zerotier".to_string(),
                address: "10.10.10.8".to_string(),
                base_url: Some(format!("http://10.10.10.8:{}", DEFAULT_PORT)),
                network_id: Some("net".to_string()),
                reachable: true,
                latency_ms: Some(12),
                last_checked_at: Some(3),
                last_error: None,
                iroh_node_id: None,
            },
        ]);

        let expected = format!("http://10.10.10.8:{}", DEFAULT_PORT);
        assert_eq!(url.as_deref(), Some(expected.as_str()));
    }
}
