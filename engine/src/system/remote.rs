use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Config;
use sdk::{NodeExecutionRole, NodeIdentity, NodeProfile, RemoteEnvelope};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePeer {
    pub identity: NodeIdentity,
    pub profile: NodeProfile,
    pub target: String,
    pub trusted: bool,
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

    pub fn pair(&self, target: &str) -> Result<RemotePeer> {
        let mut peers = self.load_peers()?;
        if peers.iter().any(|peer| {
            peer.identity.node_name == target || peer.target == target || peer.identity.node_id == target
        }) {
            bail!("Remote node '{}' is already paired", target);
        }

        let peer = RemotePeer {
            identity: NodeIdentity {
                node_id: Uuid::new_v4().to_string(),
                node_name: target.to_string(),
                public_key: Uuid::new_v4().simple().to_string(),
            },
            profile: NodeProfile {
                capabilities: vec!["remote-execution".to_string()],
                tags: vec![],
                execution_role: NodeExecutionRole::Full,
            },
            target: target.to_string(),
            trusted: false,
        };
        peers.push(peer.clone());
        self.save_peers(&peers)?;
        Ok(peer)
    }

    pub fn unpair(&self, name: &str) -> Result<()> {
        let mut peers = self.load_peers()?;
        let original_len = peers.len();
        peers.retain(|peer| {
            peer.identity.node_name != name && peer.target != name && peer.identity.node_id != name
        });
        if peers.len() == original_len {
            bail!("Remote node '{}' is not paired", name);
        }
        self.save_peers(&peers)?;
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
        let peers = self.load_peers()?;
        let Some(peer) = peers.iter().find(|peer| {
            peer.identity.node_name == node || peer.target == node || peer.identity.node_id == node
        }) else {
            bail!("Remote node '{}' is not paired", node);
        };

        if !self.config.ws_client.enabled {
            bail!("Remote service is disabled. Run `rove service enable remote` first.");
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

        Ok(RemoteSendPreview {
            envelope,
            trusted: peer.trusted,
            message: "Remote task execution transport is not wired yet; this preview validates routing prerequisites and the envelope shape.".to_string(),
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
                capabilities: vec!["task-routing".to_string(), "remote-execution".to_string()],
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
        let peers: Vec<RemotePeer> =
            toml::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(peers)
    }

    fn save_peers(&self, peers: &[RemotePeer]) -> Result<()> {
        let path = self.remote_peers_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(peers)?)?;
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
