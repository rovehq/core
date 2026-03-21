use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::Serialize;

use crate::config::Config;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub name: String,
    pub enabled: bool,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedService {
    Logging,
    WebUi,
    Remote,
    ConnectorEngine,
}

impl ManagedService {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "logging" => Some(Self::Logging),
            "webui" => Some(Self::WebUi),
            "remote" => Some(Self::Remote),
            "connector-engine" | "connector_engine" | "connectors" => {
                Some(Self::ConnectorEngine)
            }
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Logging => "logging",
            Self::WebUi => "webui",
            Self::Remote => "remote",
            Self::ConnectorEngine => "connector-engine",
        }
    }
}

pub struct ServiceManager {
    config: Config,
}

impl ServiceManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn into_config(self) -> Config {
        self.config
    }

    pub fn list(&self) -> Vec<ServiceStatus> {
        [
            ManagedService::Logging,
            ManagedService::WebUi,
            ManagedService::Remote,
            ManagedService::ConnectorEngine,
        ]
        .into_iter()
        .map(|service| self.describe(service))
        .collect()
    }

    pub fn describe(&self, service: ManagedService) -> ServiceStatus {
        let mut details = BTreeMap::new();
        let enabled = match service {
            ManagedService::Logging => {
                details.insert("log_level".to_string(), self.config.core.log_level.clone());
                !self.config.core.log_level.eq_ignore_ascii_case("error")
            }
            ManagedService::WebUi => {
                details.insert("bind_addr".to_string(), self.config.webui.bind_addr.clone());
                self.config.webui.enabled
            }
            ManagedService::Remote => {
                details.insert("url".to_string(), self.config.ws_client.url.clone());
                self.config.ws_client.enabled
            }
            ManagedService::ConnectorEngine => {
                details.insert(
                    "configured_servers".to_string(),
                    self.config.mcp.servers.len().to_string(),
                );
                !self.config.mcp.servers.is_empty()
            }
        };

        ServiceStatus {
            name: service.name().to_string(),
            enabled,
            details,
        }
    }

    pub fn set_enabled(&mut self, service: ManagedService, enabled: bool) -> Result<ServiceStatus> {
        match service {
            ManagedService::Logging => {
                self.config.core.log_level = if enabled {
                    "info".to_string()
                } else {
                    "error".to_string()
                };
            }
            ManagedService::WebUi => {
                self.config.webui.enabled = enabled;
            }
            ManagedService::Remote => {
                self.config.ws_client.enabled = enabled;
            }
            ManagedService::ConnectorEngine => {
                if !enabled && !self.config.mcp.servers.is_empty() {
                    bail!("Cannot disable connector-engine while configured connectors still exist");
                }
            }
        }

        self.config.save()?;
        Ok(self.describe(service))
    }
}
