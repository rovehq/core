use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::cli::database_path::expand_data_dir;
use crate::config::Config;
use crate::runtime::{Manifest, PluginType};
use crate::security::crypto::CryptoModule;
use crate::storage::{Database, InstalledPlugin};

use super::package::{PluginPackage, MANIFEST_FILE, PACKAGE_FILE, RUNTIME_FILE};

pub(super) struct PreparedInstall {
    pub record: InstalledPlugin,
    pub verify_path: PathBuf,
}

fn artifact_sidecar_path(payload_source: &Path) -> PathBuf {
    payload_source.with_extension("capabilities.json")
}

pub fn install_directory(config: &Config, install_id: &str, plugin_type: &PluginType) -> PathBuf {
    expand_data_dir(&config.core.data_dir)
        .join(install_root(plugin_type))
        .join(install_id)
}

pub fn legacy_install_directory(config: &Config, install_id: &str) -> PathBuf {
    expand_data_dir(&config.core.data_dir)
        .join("plugins")
        .join(install_id)
}

fn install_root(_plugin_type: &PluginType) -> &'static str {
    "plugins"
}

pub fn perform_install(
    install_dir: &Path,
    manifest_raw: &str,
    runtime_raw: Option<&str>,
    payload_source: Option<&Path>,
    package: &PluginPackage,
    manifest: &Manifest,
    install_id: &str,
) -> Result<PreparedInstall> {
    fs::write(install_dir.join(MANIFEST_FILE), manifest_raw).with_context(|| {
        format!(
            "Failed to write installed manifest to '{}'",
            install_dir.join(MANIFEST_FILE).display()
        )
    })?;
    fs::write(
        install_dir.join(PACKAGE_FILE),
        serde_json::to_string_pretty(package)
            .context("Failed to serialize installed plugin package metadata")?,
    )
    .with_context(|| {
        format!(
            "Failed to write installed package metadata to '{}'",
            install_dir.join(PACKAGE_FILE).display()
        )
    })?;

    let config_path = install_dir.join(RUNTIME_FILE);
    let config_string = if let Some(runtime_raw) = runtime_raw {
        fs::write(&config_path, runtime_raw).with_context(|| {
            format!(
                "Failed to write runtime config to '{}'",
                config_path.display()
            )
        })?;
        Some(runtime_raw.to_string())
    } else {
        None
    };

    let (binary_path, verify_path) = if let Some(payload_source) = payload_source {
        let file_name = payload_source
            .file_name()
            .context("Plugin payload file name is missing")?;
        let target = install_dir.join(file_name);
        if target != config_path {
            fs::copy(payload_source, &target).with_context(|| {
                format!(
                    "Failed to copy plugin payload from '{}' to '{}'",
                    payload_source.display(),
                    target.display()
                )
            })?;
        }

        let sidecar = artifact_sidecar_path(payload_source);
        if sidecar.exists() {
            let target_sidecar = artifact_sidecar_path(&target);
            fs::copy(&sidecar, &target_sidecar).with_context(|| {
                format!(
                    "Failed to copy plugin capability sidecar from '{}' to '{}'",
                    sidecar.display(),
                    target_sidecar.display()
                )
            })?;
        }

        if matches!(manifest.plugin_type, PluginType::Mcp) {
            (None, target)
        } else {
            (Some(target.to_string_lossy().to_string()), target)
        }
    } else {
        (None, config_path.clone())
    };

    Ok(PreparedInstall {
        record: InstalledPlugin {
            id: install_id.to_string(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            plugin_type: manifest.plugin_type.as_str().to_string(),
            trust_tier: manifest.trust_tier.as_i64(),
            manifest: manifest_raw.to_string(),
            binary_path,
            binary_hash: package.payload_hash.clone(),
            signature: package.payload_signature.clone(),
            enabled: package.enabled,
            installed_at: unix_now()?,
            last_used: None,
            config: config_string,
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
        },
        verify_path,
    })
}

pub async fn verify_and_store(
    database: &Database,
    crypto: &CryptoModule,
    prepared: PreparedInstall,
) -> Result<InstalledPlugin> {
    let PreparedInstall {
        record,
        verify_path,
    } = prepared;

    crypto
        .verify_file(&verify_path, &record.binary_hash)
        .with_context(|| format!("Payload hash verification failed for '{}'", record.name))?;
    crypto
        .verify_file_signature(&verify_path, &record.signature)
        .with_context(|| {
            format!(
                "Payload signature verification failed for '{}'",
                record.name
            )
        })?;

    database
        .installed_plugins()
        .upsert_plugin(&record)
        .await
        .context("Failed to store installed plugin")?;

    Ok(record)
}

fn unix_now() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock before UNIX_EPOCH")?
        .as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::runtime::{DomainPattern, Manifest, PathPattern, Permissions, TrustTier};

    use super::{artifact_sidecar_path, perform_install, PluginPackage, PluginType};

    fn sample_manifest() -> Manifest {
        Manifest {
            name: "Echo Plugin".to_string(),
            version: "0.1.0".to_string(),
            sdk_version: "0.1.0".to_string(),
            plugin_type: PluginType::Plugin,
            permissions: Permissions {
                filesystem: Vec::<PathPattern>::new(),
                network: Vec::<DomainPattern>::new(),
                secrets: Vec::new(),
                host_patterns: Vec::new(),
                memory_read: false,
                memory_write: false,
                wasm_max_memory_mb: None,
                tools: Vec::new(),
                wasm_fuel_limit: None,
                max_execution_time: None,
            },
            trust_tier: TrustTier::Reviewed,
            min_model: None,
            description: "Echo plugin".to_string(),
        }
    }

    #[test]
    fn perform_install_copies_capability_sidecar_with_artifact() {
        let temp = TempDir::new().expect("temp");
        let source_dir = temp.path().join("source");
        let install_dir = temp.path().join("install");
        std::fs::create_dir_all(&source_dir).expect("source dir");
        std::fs::create_dir_all(&install_dir).expect("install dir");

        let artifact = source_dir.join("echo.wasm");
        std::fs::write(&artifact, b"wasm").expect("artifact");
        let sidecar = artifact_sidecar_path(&artifact);
        std::fs::write(&sidecar, r#"{"max_memory_mb":4}"#).expect("sidecar");

        let manifest = sample_manifest();
        let prepared = perform_install(
            &install_dir,
            "{}",
            Some("{}"),
            Some(&artifact),
            &PluginPackage {
                id: Some("echo-plugin".to_string()),
                artifact: Some("echo.wasm".to_string()),
                runtime_config: Some("runtime.json".to_string()),
                payload_hash: "hash".to_string(),
                payload_signature: "sig".to_string(),
                enabled: true,
            },
            &manifest,
            "echo-plugin",
        )
        .expect("perform install");

        let installed_artifact = install_dir.join("echo.wasm");
        assert_eq!(
            prepared.record.binary_path.as_deref(),
            Some(installed_artifact.to_string_lossy().as_ref())
        );
        assert!(installed_artifact.exists());
        assert!(artifact_sidecar_path(&installed_artifact).exists());
    }
}
