use extism::{Manifest as ExtismManifest, Plugin, Wasm};
use sdk::errors::EngineError;
use std::path::PathBuf;

use super::{PluginMetadata, WasmRuntime};

impl WasmRuntime {
    pub async fn load_plugin(&mut self, name: &str) -> Result<(), EngineError> {
        tracing::info!("Loading plugin: {}", name);

        let plugin_entry = self.manifest.get_plugin(name).ok_or_else(|| {
            tracing::error!("Gate 1 FAILED: Plugin '{}' not found in manifest", name);
            EngineError::PluginNotInManifest(name.to_string())
        })?;
        tracing::info!("Gate 1 PASSED: Plugin '{}' found in manifest", name);

        if plugin_entry.trust_tier == 2 {
            tracing::warn!(
                "Loading UNVERIFIED plugin '{}' (trust tier 2). This plugin has not been reviewed. All operations will require Tier 2 confirmation.",
                name
            );
        }

        let manifest_path = PathBuf::from(&plugin_entry.path);
        if manifest_path.is_absolute() {
            tracing::error!(
                "Plugin '{}' has absolute path in manifest: {}",
                name,
                plugin_entry.path
            );
            return Err(EngineError::Config(format!(
                "Plugin '{}' has absolute path in manifest (security violation)",
                name
            )));
        }

        let plugin_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rove/plugins")
            .join(manifest_path);

        self.crypto
            .verify_file(&plugin_path, &plugin_entry.hash)
            .map_err(|error| {
                tracing::error!(
                    "Gate 2 FAILED: Hash verification failed for '{}': {}",
                    name,
                    error
                );
                error
            })?;
        tracing::info!("Gate 2 PASSED: File hash verified for '{}'", name);

        tracing::info!("Gate 2.5: Checking WASM import allowlist for '{}'", name);
        let wasm_bytes_for_check = std::fs::read(&plugin_path).map_err(|error| {
            EngineError::Plugin(format!(
                "Failed to read WASM file for import check: {}",
                error
            ))
        })?;

        if let Err(error) = self.validate_wasm_imports(
            &wasm_bytes_for_check,
            &plugin_entry.allowed_imports,
            &plugin_path,
        ) {
            tracing::error!("Gate 2.5 FAILED for '{}': {}", name, error);
            let _ = std::fs::remove_file(&plugin_path);
            tracing::warn!(
                "Deleted plugin file due to Gate 2.5 failure: {}",
                plugin_path.display()
            );
            return Err(error);
        }
        tracing::info!("Gate 2.5 PASSED: Import allowlist verified for '{}'", name);

        tracing::info!("Both gates passed for '{}', loading WASM module...", name);
        let wasm_bytes = std::fs::read(&plugin_path).map_err(|error| {
            tracing::error!(
                "Failed to read WASM file {}: {}",
                plugin_path.display(),
                error
            );
            EngineError::Plugin(format!("Failed to read WASM file: {}", error))
        })?;

        let wasm = Wasm::data(wasm_bytes);
        let extism_manifest = ExtismManifest::new([wasm]);
        let host_functions = self.create_host_functions();

        let plugin = Plugin::new(&extism_manifest, host_functions, true).map_err(|error| {
            tracing::error!("Failed to create Extism plugin for '{}': {}", name, error);
            EngineError::Plugin(format!("Failed to create plugin: {}", error))
        })?;

        self.plugins.insert(
            name.to_string(),
            PluginMetadata {
                plugin,
                crash_count: 0,
            },
        );

        tracing::info!("Plugin '{}' loaded successfully", name);
        Ok(())
    }
}
