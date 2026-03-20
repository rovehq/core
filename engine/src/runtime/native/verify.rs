use std::path::PathBuf;

use sdk::errors::EngineError;

use super::NativeRuntime;

impl NativeRuntime {
    pub(super) fn verified_tool_path(&self, name: &str) -> Result<PathBuf, EngineError> {
        let tool_entry = self.manifest.get_core_tool(name).ok_or_else(|| {
            tracing::error!("Gate 1 FAILED: Tool '{}' not found in manifest", name);
            EngineError::ToolNotInManifest(name.to_string())
        })?;
        tracing::info!("Gate 1 PASSED: Tool '{}' found in manifest", name);

        let tool_path = PathBuf::from(&tool_entry.path);

        self.crypto
            .verify_file(&tool_path, &tool_entry.hash)
            .map_err(|error| {
                tracing::error!(
                    "Gate 2 FAILED: Hash verification failed for '{}': {}",
                    name,
                    error
                );
                error
            })?;
        tracing::info!("Gate 2 PASSED: File hash verified for '{}'", name);

        let manifest_bytes = serde_json::to_vec(&self.manifest).map_err(|error| {
            EngineError::Config(format!("Failed to serialize manifest: {}", error))
        })?;
        self.crypto
            .verify_manifest(&manifest_bytes, &self.manifest.signature)
            .map_err(|error| {
                tracing::error!(
                    "Gate 3 FAILED: Manifest signature verification failed: {}",
                    error
                );
                self.delete_compromised_file(&tool_path);
                error
            })?;
        tracing::info!("Gate 3 PASSED: Manifest signature verified");

        self.crypto
            .verify_file_signature(&tool_path, &tool_entry.signature)
            .map_err(|error| {
                tracing::error!(
                    "Gate 4 FAILED: Tool signature verification failed for '{}': {}",
                    name,
                    error
                );
                self.delete_compromised_file(&tool_path);
                error
            })?;
        tracing::info!("Gate 4 PASSED: Tool signature verified for '{}'", name);

        Ok(tool_path)
    }

    pub(super) fn verify_registered_library_path(
        &self,
        lib_path: &str,
    ) -> Result<PathBuf, EngineError> {
        let registration = self.registered_libraries.get(lib_path).ok_or_else(|| {
            tracing::error!(
                "Native library '{}' was called before registration metadata was loaded",
                lib_path
            );
            EngineError::ToolNotInManifest(lib_path.to_string())
        })?;

        let tool_path = PathBuf::from(lib_path);

        self.crypto
            .verify_file(&tool_path, &registration.hash)
            .map_err(|error| {
                tracing::error!(
                    "Native hash verification failed for '{}': {}",
                    tool_path.display(),
                    error
                );
                error
            })?;

        self.crypto
            .verify_file_signature(&tool_path, &registration.signature)
            .map_err(|error| {
                tracing::error!(
                    "Native signature verification failed for '{}': {}",
                    tool_path.display(),
                    error
                );
                self.delete_compromised_file(&tool_path);
                error
            })?;

        Ok(tool_path)
    }

    fn delete_compromised_file(&self, tool_path: &PathBuf) {
        if let Err(error) = std::fs::remove_file(tool_path) {
            tracing::error!(
                "Failed to delete compromised file {}: {}",
                tool_path.display(),
                error
            );
        }
    }
}
