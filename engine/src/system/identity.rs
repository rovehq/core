use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use blake3::Hasher;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::config::{Config, DaemonProfile};
use sdk::NodeIdentity;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodePublicRecord {
    node_id: String,
    node_name: String,
    public_key: String,
}

pub struct IdentityManager {
    config: Config,
}

impl IdentityManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn load_or_init(&self) -> Result<NodeIdentity> {
        if self.public_identity_path().exists() && self.private_key_path().exists() {
            return self.load();
        }

        self.initialize()
    }

    pub fn rename(&self, node_name: &str) -> Result<NodeIdentity> {
        let mut record = self.load_public_record()?;
        record.node_name = node_name.trim().to_string();
        self.write_public_record(&record)?;
        Ok(NodeIdentity {
            node_id: record.node_id,
            node_name: record.node_name,
            public_key: record.public_key,
        })
    }

    pub fn sign_message(&self, message: &[u8]) -> Result<String> {
        let signing_key = self.load_signing_key()?;
        Ok(hex::encode(signing_key.sign(message).to_bytes()))
    }

    pub fn verify_message(public_key_hex: &str, message: &[u8], signature_hex: &str) -> bool {
        let Ok(public_key_bytes) = hex::decode(public_key_hex) else {
            return false;
        };
        let Ok(signature_bytes) = hex::decode(signature_hex) else {
            return false;
        };
        let Ok(public_key_array) = <[u8; 32]>::try_from(public_key_bytes.as_slice()) else {
            return false;
        };
        let Ok(signature_array) = <[u8; 64]>::try_from(signature_bytes.as_slice()) else {
            return false;
        };
        let Ok(verifying_key) = VerifyingKey::from_bytes(&public_key_array) else {
            return false;
        };
        let signature = Signature::from_bytes(&signature_array);
        verifying_key.verify(message, &signature).is_ok()
    }

    pub fn identity_dir(&self) -> PathBuf {
        identity_root(&self.config).join("identity")
    }

    fn public_identity_path(&self) -> PathBuf {
        self.identity_dir().join("node-public.toml")
    }

    fn private_key_path(&self) -> PathBuf {
        self.identity_dir().join("node-private.key")
    }

    fn initialize(&self) -> Result<NodeIdentity> {
        fs::create_dir_all(self.identity_dir())
            .with_context(|| format!("Failed to create {}", self.identity_dir().display()))?;

        let mut secret = [0u8; 32];
        OsRng.fill_bytes(&mut secret);
        let signing_key = SigningKey::from_bytes(&secret);
        let verifying_key = signing_key.verifying_key();
        let public_key = hex::encode(verifying_key.to_bytes());
        let node_id = derive_node_id(&verifying_key);
        let node_name = default_node_name();

        let record = NodePublicRecord {
            node_id: node_id.clone(),
            node_name: node_name.clone(),
            public_key: public_key.clone(),
        };
        self.write_public_record(&record)?;
        self.write_private_key(&signing_key)?;

        Ok(NodeIdentity {
            node_id,
            node_name,
            public_key,
        })
    }

    fn load(&self) -> Result<NodeIdentity> {
        let record = self.load_public_record()?;
        Ok(NodeIdentity {
            node_id: record.node_id,
            node_name: record.node_name,
            public_key: record.public_key,
        })
    }

    fn load_public_record(&self) -> Result<NodePublicRecord> {
        let raw = fs::read_to_string(self.public_identity_path())
            .with_context(|| format!("Failed to read {}", self.public_identity_path().display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", self.public_identity_path().display()))
    }

    fn write_public_record(&self, record: &NodePublicRecord) -> Result<()> {
        let serialized = toml::to_string_pretty(record)?;
        fs::write(self.public_identity_path(), serialized).with_context(|| {
            format!("Failed to write {}", self.public_identity_path().display())
        })?;
        lock_down_path(&self.public_identity_path())?;
        Ok(())
    }

    fn write_private_key(&self, signing_key: &SigningKey) -> Result<()> {
        fs::write(self.private_key_path(), hex::encode(signing_key.to_bytes()))
            .with_context(|| format!("Failed to write {}", self.private_key_path().display()))?;
        lock_down_path(&self.private_key_path())?;
        Ok(())
    }

    fn load_signing_key(&self) -> Result<SigningKey> {
        let raw = fs::read_to_string(self.private_key_path())
            .with_context(|| format!("Failed to read {}", self.private_key_path().display()))?;
        let bytes = hex::decode(raw.trim()).with_context(|| {
            format!("Invalid key data in {}", self.private_key_path().display())
        })?;
        let array = <[u8; 32]>::try_from(bytes.as_slice())
            .context("Expected 32-byte Ed25519 signing key")?;
        Ok(SigningKey::from_bytes(&array))
    }
}

fn derive_node_id(verifying_key: &VerifyingKey) -> String {
    let mut hasher = Hasher::new();
    hasher.update(verifying_key.as_bytes());
    let hash = hasher.finalize();
    format!("node_{}", &hash.to_hex()[..20])
}

fn default_node_name() -> String {
    std::env::var("ROVE_NODE_NAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "rove-node-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            )
        })
}

fn identity_root(config: &Config) -> PathBuf {
    if matches!(
        config.daemon.profile,
        DaemonProfile::Headless | DaemonProfile::Edge
    ) && config.core.data_dir == crate::config::default_data_dir()
    {
        return PathBuf::from("/var/lib/rove");
    }

    config
        .core
        .data_dir
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| config.core.data_dir.clone())
}

fn lock_down_path(path: &PathBuf) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to chmod {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn identity_is_stable_once_created() {
        let temp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.core.data_dir = temp.path().join("data");
        config.daemon.profile = DaemonProfile::Desktop;

        let manager = IdentityManager::new(config);
        let first = manager.load_or_init().expect("first");
        let second = manager.load_or_init().expect("second");
        assert_eq!(first.node_id, second.node_id);
        assert_eq!(first.public_key, second.public_key);
    }

    #[test]
    fn rename_changes_name_only() {
        let temp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.core.data_dir = temp.path().join("data");
        let manager = IdentityManager::new(config);
        let first = manager.load_or_init().expect("identity");
        let renamed = manager.rename("office-mac").expect("rename");
        assert_eq!(first.node_id, renamed.node_id);
        assert_eq!(first.public_key, renamed.public_key);
        assert_eq!(renamed.node_name, "office-mac");
    }
}
