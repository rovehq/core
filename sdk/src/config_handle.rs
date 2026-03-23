use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigMetadataSnapshot {
    pub schema_version: u32,
    pub written_by_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonConfigSnapshot {
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreConfigSnapshot {
    pub workspace: String,
    pub data_dir: String,
    pub log_level: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalConfigSnapshot {
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmConfigSnapshot {
    pub default_provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretConfigSnapshot {
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicesConfigSnapshot {
    pub webui_enabled: bool,
    pub remote_enabled: bool,
    pub connector_engine_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelsConfigSnapshot {
    pub telegram_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedConfigSnapshot {
    pub metadata: ConfigMetadataSnapshot,
    pub daemon: DaemonConfigSnapshot,
    pub core: CoreConfigSnapshot,
    pub approvals: ApprovalConfigSnapshot,
    pub llm: LlmConfigSnapshot,
    pub secrets: SecretConfigSnapshot,
    pub services: ServicesConfigSnapshot,
    pub channels: ChannelsConfigSnapshot,
}

#[derive(Clone)]
pub struct ConfigHandle {
    inner: Arc<dyn ConfigHandleImpl>,
}

impl ConfigHandle {
    pub fn new(inner: Arc<dyn ConfigHandleImpl>) -> Self {
        Self { inner }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.inner.get(key)
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str().map(String::from))
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_i64())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| v.as_bool())
    }

    pub fn schema_version(&self) -> u32 {
        self.inner.schema_version()
    }

    pub fn snapshot(&self) -> Option<VersionedConfigSnapshot> {
        self.inner.snapshot()
    }
}

pub trait ConfigHandleImpl: Send + Sync {
    fn get(&self, key: &str) -> Option<Value>;

    fn schema_version(&self) -> u32 {
        0
    }

    fn snapshot(&self) -> Option<VersionedConfigSnapshot> {
        None
    }
}

#[derive(Debug, Clone, Default)]
pub struct StaticConfigHandle {
    snapshot: Option<VersionedConfigSnapshot>,
    values: BTreeMap<String, Value>,
}

impl StaticConfigHandle {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_snapshot(snapshot: VersionedConfigSnapshot) -> Self {
        let mut handle = Self {
            snapshot: Some(snapshot.clone()),
            values: BTreeMap::new(),
        };
        handle.index_snapshot(&snapshot);
        handle
    }

    pub fn with_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.values.insert(key.into(), value);
        self
    }

    fn index_snapshot(&mut self, snapshot: &VersionedConfigSnapshot) {
        self.values.insert(
            "config.schema_version".to_string(),
            json!(snapshot.metadata.schema_version),
        );
        self.values.insert(
            "config.written_by_version".to_string(),
            json!(snapshot.metadata.written_by_version),
        );
        self.values
            .insert("daemon.profile".to_string(), json!(snapshot.daemon.profile));
        self.values
            .insert("core.workspace".to_string(), json!(snapshot.core.workspace));
        self.values
            .insert("core.data_dir".to_string(), json!(snapshot.core.data_dir));
        self.values
            .insert("core.log_level".to_string(), json!(snapshot.core.log_level));
        self.values
            .insert("approvals.mode".to_string(), json!(snapshot.approvals.mode));
        self.values.insert(
            "llm.default_provider".to_string(),
            json!(snapshot.llm.default_provider),
        );
        self.values.insert(
            "secrets.backend".to_string(),
            json!(snapshot.secrets.backend),
        );
        self.values.insert(
            "services.webui.enabled".to_string(),
            json!(snapshot.services.webui_enabled),
        );
        self.values.insert(
            "services.remote.enabled".to_string(),
            json!(snapshot.services.remote_enabled),
        );
        self.values.insert(
            "services.connector_engine.enabled".to_string(),
            json!(snapshot.services.connector_engine_enabled),
        );
        self.values.insert(
            "channels.telegram.enabled".to_string(),
            json!(snapshot.channels.telegram_enabled),
        );
    }
}

impl ConfigHandleImpl for StaticConfigHandle {
    fn get(&self, key: &str) -> Option<Value> {
        self.values.get(key).cloned()
    }

    fn schema_version(&self) -> u32 {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.metadata.schema_version)
            .unwrap_or_default()
    }

    fn snapshot(&self) -> Option<VersionedConfigSnapshot> {
        self.snapshot.clone()
    }
}
