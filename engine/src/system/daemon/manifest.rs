use crate::crypto::CryptoModule;

use super::DaemonManager;

impl DaemonManager {
    pub(super) fn verify_manifest_at_startup() -> std::result::Result<(), String> {
        let manifest_paths = [
            std::path::PathBuf::from("manifest/manifest.json"),
            dirs::home_dir()
                .map(|home| home.join(".rove/manifest.json"))
                .unwrap_or_default(),
        ];

        let Some(manifest_path) = manifest_paths.iter().find(|path| path.exists()) else {
            tracing::debug!("No manifest.json found — skipping verification");
            return Ok(());
        };

        tracing::info!("Verifying manifest at {}", manifest_path.display());
        let manifest_bytes =
            std::fs::read(manifest_path).map_err(|error| format!("Failed to read manifest: {}", error))?;
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)
            .map_err(|error| format!("Failed to parse manifest JSON: {}", error))?;

        let crypto =
            CryptoModule::new().map_err(|error| format!("Failed to initialize crypto: {}", error))?;

        if let Some(signature) = manifest.get("signature").and_then(|value| value.as_str()) {
            let mut manifest_for_verify = manifest.clone();
            if let Some(object) = manifest_for_verify.as_object_mut() {
                object.remove("signature");
            }
            let verify_bytes = serde_json::to_vec(&manifest_for_verify)
                .map_err(|error| format!("Failed to serialize manifest for verification: {}", error))?;

            crypto
                .verify_manifest(&verify_bytes, signature)
                .map_err(|error| format!("Manifest signature verification failed: {}", error))?;
            tracing::info!("Manifest signature verified successfully");
        } else {
            tracing::debug!("No signature in manifest — skipping signature verification");
        }

        if let Some(entries) = manifest.get("entries").and_then(|value| value.as_array()) {
            for entry in entries {
                let path_str = entry.get("path").and_then(|value| value.as_str()).unwrap_or("");
                let hash = entry.get("hash").and_then(|value| value.as_str()).unwrap_or("");
                if path_str.is_empty() || hash.is_empty() {
                    continue;
                }

                let file_path = std::path::Path::new(path_str);
                if file_path.exists() {
                    if let Err(error) = crypto.verify_file(file_path, hash) {
                        tracing::error!("File verification failed for {}: {}", path_str, error);
                        return Err(format!("File verification failed for {}: {}", path_str, error));
                    }
                    tracing::debug!("Verified: {}", path_str);
                } else {
                    tracing::debug!("Skipping missing file: {}", path_str);
                }
            }
        }

        tracing::info!("Manifest verification completed successfully");
        Ok(())
    }
}
