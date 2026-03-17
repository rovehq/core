//! WASM runtime for loading and managing plugins.

mod call;
mod host;
mod inspect;
mod load;
mod restart;
#[cfg(test)]
mod tests;

use crate::crypto::CryptoModule;
use crate::fs_guard::FileSystemGuard;
use crate::message_bus::MessageBus;
use extism::Plugin;
use sdk::{errors::EngineError, manifest::Manifest};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_CRASH_RESTARTS: u32 = 3;

struct PluginMetadata {
    plugin: Plugin,
    crash_count: u32,
}

pub struct WasmRuntime {
    plugins: HashMap<String, PluginMetadata>,
    manifest: Manifest,
    crypto: Arc<CryptoModule>,
    #[allow(dead_code)]
    fs_guard: Arc<FileSystemGuard>,
    message_bus: Option<Arc<MessageBus>>,
}

impl WasmRuntime {
    pub fn new(
        manifest: Manifest,
        crypto: Arc<CryptoModule>,
        fs_guard: Arc<FileSystemGuard>,
    ) -> Self {
        tracing::info!("Initializing WasmRuntime");
        Self {
            plugins: HashMap::new(),
            manifest,
            crypto,
            fs_guard,
            message_bus: None,
        }
    }

    pub fn load_from_directory(
        plugin_dir: &std::path::Path,
        crypto: Arc<CryptoModule>,
        fs_guard: Arc<FileSystemGuard>,
    ) -> Result<Self, EngineError> {
        let manifest_path = plugin_dir.join("manifest.json");
        let manifest_bytes = std::fs::read(&manifest_path).map_err(|error| {
            tracing::error!(
                "Failed to read manifest.json at {}: {}",
                manifest_path.display(),
                error
            );
            EngineError::Io(error)
        })?;

        tracing::info!(
            "Gate 0: Verifying manifest signature at {}",
            manifest_path.display()
        );
        crypto.verify_manifest_file(&manifest_bytes)?;
        tracing::info!("Gate 0 PASSED: Manifest signature verified");

        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|error| EngineError::Config(format!("Invalid manifest JSON: {}", error)))?;

        Ok(Self::new(manifest, crypto, fs_guard))
    }

    pub fn set_message_bus(&mut self, bus: Arc<MessageBus>) {
        self.message_bus = Some(bus);
    }
}
