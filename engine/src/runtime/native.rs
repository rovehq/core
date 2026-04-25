//! Native runtime for loading and managing core tools.

mod call;
mod load;
mod verify;

use crate::crypto::CryptoModule;
use sdk::agent_handle::{AgentHandle, AgentHandleImpl};
use sdk::bus_handle::{BusHandle, BusHandleImpl};
use sdk::config_handle::{ConfigHandle, ConfigHandleImpl};
use sdk::core_tool::{CoreContext, CoreTool};
use sdk::crypto_handle::{CryptoHandle, CryptoHandleImpl};
use sdk::db_handle::{DbHandle, DbHandleImpl};
use sdk::errors::EngineError;
use sdk::manifest::Manifest;
use sdk::network_handle::{NetworkHandle, NetworkHandleImpl};
use std::collections::HashMap;
use std::sync::Arc;

struct RegisteredLibrary {
    hash: String,
    signature: String,
}

// Keep the tool before the library so the tool drops before the shared object unloads.
struct LoadedLibrary {
    tool: Box<dyn CoreTool>,
    #[allow(dead_code)]
    library: libloading::Library,
}

pub struct NativeRuntime {
    manifest: Manifest,
    crypto: Arc<CryptoModule>,
    loaded_libraries: HashMap<String, LoadedLibrary>,
    loaded_tools: HashMap<String, String>,
    registered_libraries: HashMap<String, RegisteredLibrary>,
    ctx: CoreContext,
}

impl NativeRuntime {
    pub fn new(manifest: Manifest, crypto: Arc<CryptoModule>) -> Self {
        tracing::info!("Initializing NativeRuntime");
        Self {
            manifest,
            crypto,
            loaded_libraries: HashMap::new(),
            loaded_tools: HashMap::new(),
            registered_libraries: HashMap::new(),
            ctx: default_core_context(),
        }
    }

    pub fn set_context(&mut self, ctx: CoreContext) {
        self.ctx = ctx;
    }

    pub fn register_library(
        &mut self,
        lib_path: impl Into<String>,
        hash: impl Into<String>,
        signature: impl Into<String>,
    ) {
        self.registered_libraries.insert(
            lib_path.into(),
            RegisteredLibrary {
                hash: hash.into(),
                signature: signature.into(),
            },
        );
    }

    #[doc(hidden)]
    pub fn is_library_registered(&self, lib_path: &str) -> bool {
        self.registered_libraries.contains_key(lib_path)
    }
}

fn default_core_context() -> CoreContext {
    CoreContext::new(
        AgentHandle::new(Arc::new(NoopAgentHandle)),
        DbHandle::new(Arc::new(NoopDbHandle)),
        ConfigHandle::new(Arc::new(NoopConfigHandle)),
        CryptoHandle::new(Arc::new(NoopCryptoHandle)),
        NetworkHandle::new(Arc::new(NoopNetworkHandle)),
        BusHandle::new(Arc::new(NoopBusHandle)),
    )
}

struct NoopAgentHandle;

impl AgentHandleImpl for NoopAgentHandle {
    fn submit_task(&self, _task_input: String) -> Result<String, EngineError> {
        Err(EngineError::ToolError(
            "native runtime agent handle is not configured".to_string(),
        ))
    }

    fn get_task_status(&self, _task_id: &str) -> Result<String, EngineError> {
        Err(EngineError::ToolError(
            "native runtime agent handle is not configured".to_string(),
        ))
    }
}

struct NoopDbHandle;

impl DbHandleImpl for NoopDbHandle {
    fn query(
        &self,
        _sql: &str,
        _params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, EngineError> {
        Err(EngineError::ToolError(
            "native runtime database handle is not configured".to_string(),
        ))
    }
}

struct NoopConfigHandle;

impl ConfigHandleImpl for NoopConfigHandle {
    fn get(&self, _key: &str) -> Option<serde_json::Value> {
        None
    }
}

struct NoopCryptoHandle;

impl CryptoHandleImpl for NoopCryptoHandle {
    fn sign_data(&self, _data: &[u8]) -> Result<Vec<u8>, EngineError> {
        Err(EngineError::ToolError(
            "native runtime crypto handle is not configured".to_string(),
        ))
    }

    fn verify_signature(&self, _data: &[u8], _signature: &[u8]) -> Result<(), EngineError> {
        Err(EngineError::ToolError(
            "native runtime crypto handle is not configured".to_string(),
        ))
    }

    fn get_secret(&self, key: &str) -> Result<String, EngineError> {
        Err(EngineError::KeyringError(format!(
            "secret '{}' is not available in native runtime fallback context",
            key
        )))
    }

    fn scrub_secrets(&self, text: &str) -> String {
        text.to_string()
    }
}

struct NoopNetworkHandle;

impl NetworkHandleImpl for NoopNetworkHandle {
    fn http_get(&self, _url: &str) -> Result<Vec<u8>, EngineError> {
        Err(EngineError::Network(
            "native runtime network handle is not configured".to_string(),
        ))
    }

    fn http_post(&self, _url: &str, _body: Vec<u8>) -> Result<Vec<u8>, EngineError> {
        Err(EngineError::Network(
            "native runtime network handle is not configured".to_string(),
        ))
    }
}

struct NoopBusHandle;

impl BusHandleImpl for NoopBusHandle {
    fn subscribe(&self, _event_type: &str) -> Result<(), EngineError> {
        Ok(())
    }

    fn publish(&self, _event_type: &str, _payload: serde_json::Value) -> Result<(), EngineError> {
        Ok(())
    }
}
