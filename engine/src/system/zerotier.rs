use std::net::SocketAddr;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{metadata::DEFAULT_PORT, Config};
use crate::config::metadata::SERVICE_NAME;
use crate::secrets::SecretManager;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RemoteTransportRecord {
    pub kind: String,
    pub address: String,
    pub base_url: Option<String>,
    pub network_id: Option<String>,
    pub reachable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZeroTierStatus {
    pub enabled: bool,
    pub configured: bool,
    pub token_configured: bool,
    pub service_url: String,
    pub network_id: Option<String>,
    pub managed_name_sync: bool,
    pub service_online: bool,
    pub joined: bool,
    pub node_id: Option<String>,
    pub network_name: Option<String>,
    pub network_status: Option<String>,
    pub assigned_addresses: Vec<String>,
    #[serde(default)]
    pub transport_records: Vec<RemoteTransportRecord>,
    pub message: Option<String>,
}

pub struct ZeroTierManager {
    config: Config,
}

impl ZeroTierManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn status(&self) -> Result<ZeroTierStatus> {
        let settings = &self.config.remote.transports.zerotier;
        let token_key = settings
            .api_token_key
            .clone()
            .unwrap_or_else(|| "zerotier_api_token".to_string());
        let token = SecretManager::new(SERVICE_NAME)
            .lookup_secret(&token_key)
            .await
            .map(|(value, _)| value);

        let mut status = ZeroTierStatus {
            enabled: settings.enabled,
            configured: settings.network_id.is_some(),
            token_configured: token.is_some(),
            service_url: settings.service_url.clone(),
            network_id: settings.network_id.clone(),
            managed_name_sync: settings.managed_name_sync,
            service_online: false,
            joined: false,
            node_id: None,
            network_name: None,
            network_status: None,
            assigned_addresses: Vec::new(),
            transport_records: Vec::new(),
            message: None,
        };

        if !settings.enabled {
            status.message = Some("ZeroTier transport is disabled.".to_string());
            return Ok(status);
        }
        let Some(token) = token else {
            status.message = Some(format!(
                "Missing ZeroTier API token secret '{}'.",
                token_key
            ));
            return Ok(status);
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .context("Failed to build ZeroTier client")?;

        match self
            .request_json(&client, "/status", reqwest::Method::GET, None, &token)
            .await
        {
            Ok(value) => {
                status.service_online = true;
                status.node_id = extract_string(&value, &["address", "nodeId", "id"]);
            }
            Err(error) => {
                status.message = Some(format!("ZeroTier service is unreachable: {}", error));
                return Ok(status);
            }
        }

        let Some(network_id) = settings.network_id.as_deref() else {
            status.message = Some("ZeroTier is enabled but no network id is configured.".to_string());
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
                status.assigned_addresses = extract_string_list(&value, &["assignedAddresses", "assigned_addresses"]);
                status.transport_records = self.transport_records_from_addresses(
                    network_id,
                    &status.assigned_addresses,
                );
                if status.transport_records.is_empty() {
                    status.message = Some(
                        "ZeroTier network is joined, but the daemon is still bound to localhost only."
                            .to_string(),
                    );
                }
            }
            Err(error) => {
                status.message = Some(format!(
                    "ZeroTier network '{}' is not joined yet: {}",
                    network_id, error
                ));
            }
        }

        Ok(status)
    }

    pub async fn join(&self, network_id_override: Option<&str>) -> Result<ZeroTierStatus> {
        let settings = &self.config.remote.transports.zerotier;
        if !settings.enabled {
            return Ok(ZeroTierStatus {
                enabled: false,
                message: Some("ZeroTier transport is disabled.".to_string()),
                ..ZeroTierStatus::default()
            });
        }

        let network_id = network_id_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or(settings.network_id.as_deref())
            .ok_or_else(|| anyhow::anyhow!("No ZeroTier network id is configured"))?;
        let token_key = settings
            .api_token_key
            .clone()
            .unwrap_or_else(|| "zerotier_api_token".to_string());
        let token = SecretManager::new(SERVICE_NAME)
            .lookup_secret(&token_key)
            .await
            .map(|(value, _)| value)
            .ok_or_else(|| anyhow::anyhow!("Missing ZeroTier API token secret '{}'", token_key))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .context("Failed to build ZeroTier client")?;
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

    pub async fn transport_records(&self) -> Result<Vec<RemoteTransportRecord>> {
        Ok(self.status().await?.transport_records)
    }

    async fn request_json(
        &self,
        client: &reqwest::Client,
        path: &str,
        method: reqwest::Method,
        body: Option<Value>,
        token: &str,
    ) -> Result<Value> {
        let base = self.config.remote.transports.zerotier.service_url.trim_end_matches('/');
        let url = format!("{}{}", base, path);
        let mut request = client
            .request(method, &url)
            .header("X-ZT1-Auth", token);
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
            anyhow::bail!("status {}: {}", status, body);
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
            })
            .collect()
    }
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
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|entry| entry.as_str()).map(ToOwned::to_owned)
    })
}

fn extract_string_list(value: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| {
            value.get(*key).and_then(|entry| {
                entry.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect::<Vec<_>>()
                })
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_bind_addr_is_not_remote_reachable() {
        let mut config = Config::default();
        config.webui.bind_addr = "127.0.0.1:47630".to_string();
        assert!(!daemon_reachable_over_network(&config));
    }

    #[test]
    fn zerotier_transport_records_use_daemon_port() {
        let mut config = Config::default();
        config.webui.bind_addr = "0.0.0.0:47630".to_string();
        let manager = ZeroTierManager::new(config);
        let records = manager.transport_records_from_addresses(
            "8056c2e21c000001",
            &["10.10.10.8/24".to_string()],
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].base_url.as_deref(), Some("http://10.10.10.8:47630"));
        assert!(records[0].reachable);
    }
}
