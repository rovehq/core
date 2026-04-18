use extism::{Manifest as ExtismManifest, PluginBuilder, Wasm};
use extism_manifest::MemoryOptions;
use sdk::errors::EngineError;
use serde::Deserialize;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runtime::sdk_plugin_entry_from_installed_plugin;
use crate::storage::InstalledPlugin;

use super::{PluginMetadata, WasmRuntime};

const WASM_PAGE_SIZE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct WasmResourceLimits {
    pub timeout_secs: u64,
    pub max_memory_mb: u32,
    pub fuel_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct WasmLimitReport {
    pub timeout_secs: u64,
    pub max_memory_mb: u32,
    pub fuel_limit: u64,
    pub sidecar_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct WasmCapabilitiesSidecar {
    #[serde(default)]
    max_execution_time_secs: Option<u64>,
    #[serde(default)]
    max_memory_mb: Option<u32>,
    #[serde(default)]
    fuel_limit: Option<u64>,
}

pub(crate) fn wasm_capabilities_sidecar_path(plugin_path: &Path) -> PathBuf {
    plugin_path.with_extension("capabilities.json")
}

fn wasm_pages_from_mb(memory_mb: u32) -> u32 {
    let bytes = memory_mb as u64 * 1024 * 1024;
    bytes.div_ceil(WASM_PAGE_SIZE_BYTES) as u32
}

fn validate_positive_limit<T>(
    value: Option<T>,
    label: &str,
    plugin_path: &Path,
) -> Result<Option<T>, EngineError>
where
    T: Copy + PartialOrd + From<u8> + std::fmt::Display,
{
    if let Some(value) = value {
        if value < T::from(1) {
            return Err(EngineError::Config(format!(
                "Invalid {} in {}: value must be positive",
                label,
                plugin_path.display()
            )));
        }
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

pub(crate) fn resolve_wasm_resource_limits(
    plugin_entry: &sdk::manifest::PluginEntry,
    plugin_path: &Path,
) -> Result<WasmResourceLimits, EngineError> {
    let mut limits = WasmResourceLimits {
        timeout_secs: plugin_entry.wasm_timeout_secs(),
        max_memory_mb: plugin_entry.wasm_max_memory_mb(),
        fuel_limit: plugin_entry.wasm_fuel_limit(),
    };

    let sidecar_path = wasm_capabilities_sidecar_path(plugin_path);
    if !sidecar_path.exists() {
        return Ok(limits);
    }

    let raw = std::fs::read_to_string(&sidecar_path).map_err(|error| {
        EngineError::Config(format!(
            "Failed to read WASM capability sidecar {}: {}",
            sidecar_path.display(),
            error
        ))
    })?;
    let sidecar: WasmCapabilitiesSidecar = serde_json::from_str(&raw).map_err(|error| {
        EngineError::Config(format!(
            "Invalid WASM capability sidecar {}: {}",
            sidecar_path.display(),
            error
        ))
    })?;

    let timeout_secs = validate_positive_limit(
        sidecar.max_execution_time_secs,
        "max_execution_time_secs",
        &sidecar_path,
    )?;
    let max_memory_mb =
        validate_positive_limit(sidecar.max_memory_mb, "max_memory_mb", &sidecar_path)?;
    let fuel_limit = validate_positive_limit(sidecar.fuel_limit, "fuel_limit", &sidecar_path)?;

    if let Some(timeout_secs) = timeout_secs {
        limits.timeout_secs = limits.timeout_secs.min(timeout_secs);
    }
    if let Some(max_memory_mb) = max_memory_mb {
        limits.max_memory_mb = limits.max_memory_mb.min(max_memory_mb);
    }
    if let Some(fuel_limit) = fuel_limit {
        limits.fuel_limit = limits.fuel_limit.min(fuel_limit);
    }

    Ok(limits)
}

pub(crate) fn effective_wasm_limit_report(
    plugin_entry: &sdk::manifest::PluginEntry,
    plugin_path: &Path,
) -> Result<WasmLimitReport, EngineError> {
    let limits = resolve_wasm_resource_limits(plugin_entry, plugin_path)?;
    let sidecar_path = wasm_capabilities_sidecar_path(plugin_path);
    Ok(WasmLimitReport {
        timeout_secs: limits.timeout_secs,
        max_memory_mb: limits.max_memory_mb,
        fuel_limit: limits.fuel_limit,
        sidecar_path: sidecar_path
            .exists()
            .then(|| sidecar_path.display().to_string()),
    })
}

pub(crate) fn installed_plugin_wasm_limit_report(
    plugin: &InstalledPlugin,
) -> Result<Option<WasmLimitReport>, EngineError> {
    let Some(plugin_entry) = sdk_plugin_entry_from_installed_plugin(plugin) else {
        return Ok(None);
    };
    let Some(binary_path) = &plugin.binary_path else {
        return Ok(None);
    };
    let plugin_path = PathBuf::from(binary_path);
    effective_wasm_limit_report(&plugin_entry, &plugin_path).map(Some)
}

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
        let resource_limits = resolve_wasm_resource_limits(plugin_entry, &plugin_path)?;
        let extism_manifest = ExtismManifest::new([wasm])
            .with_memory_options(
                MemoryOptions::new()
                    .with_max_pages(wasm_pages_from_mb(resource_limits.max_memory_mb)),
            )
            .with_timeout(Duration::from_secs(resource_limits.timeout_secs));
        let host_functions = self.create_host_functions(plugin_entry);
        let plugin = PluginBuilder::new(extism_manifest)
            .with_functions(host_functions)
            .with_wasi(true)
            .with_fuel_limit(resource_limits.fuel_limit)
            .build()
            .map_err(|error| {
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

#[cfg(test)]
mod tests {
    use super::{
        effective_wasm_limit_report, resolve_wasm_resource_limits, wasm_capabilities_sidecar_path,
        wasm_pages_from_mb,
    };
    use sdk::manifest::PluginEntry;
    use tempfile::TempDir;

    #[test]
    fn resource_limits_use_signed_defaults_without_sidecar() {
        let temp = TempDir::new().expect("temp");
        let plugin_path = temp.path().join("echo.wasm");
        std::fs::write(&plugin_path, b"wasm").expect("write plugin");

        let plugin = PluginEntry::default();
        let limits = resolve_wasm_resource_limits(&plugin, &plugin_path).expect("limits");
        assert_eq!(limits.timeout_secs, 60);
        assert_eq!(limits.max_memory_mb, 10);
        assert_eq!(limits.fuel_limit, 50_000_000);
        assert_eq!(wasm_pages_from_mb(limits.max_memory_mb), 160);
    }

    #[test]
    fn sidecar_can_only_make_limits_stricter() {
        let temp = TempDir::new().expect("temp");
        let plugin_path = temp.path().join("echo.wasm");
        std::fs::write(&plugin_path, b"wasm").expect("write plugin");
        std::fs::write(
            wasm_capabilities_sidecar_path(&plugin_path),
            r#"{"max_execution_time_secs":15,"max_memory_mb":4,"fuel_limit":1000}"#,
        )
        .expect("write sidecar");

        let plugin = PluginEntry::default();
        let limits = resolve_wasm_resource_limits(&plugin, &plugin_path).expect("limits");
        assert_eq!(limits.timeout_secs, 15);
        assert_eq!(limits.max_memory_mb, 4);
        assert_eq!(limits.fuel_limit, 1000);
    }

    #[test]
    fn effective_limit_report_includes_sidecar_path_when_present() {
        let temp = TempDir::new().expect("temp");
        let plugin_path = temp.path().join("echo.wasm");
        std::fs::write(&plugin_path, b"wasm").expect("write plugin");
        let sidecar_path = wasm_capabilities_sidecar_path(&plugin_path);
        std::fs::write(&sidecar_path, r#"{"max_memory_mb":4}"#).expect("write sidecar");

        let report = effective_wasm_limit_report(&PluginEntry::default(), &plugin_path)
            .expect("effective report");
        assert_eq!(report.max_memory_mb, 4);
        assert_eq!(
            report.sidecar_path,
            Some(sidecar_path.display().to_string())
        );
    }

    #[test]
    fn sidecar_rejects_non_positive_limits() {
        let temp = TempDir::new().expect("temp");
        let plugin_path = temp.path().join("echo.wasm");
        std::fs::write(&plugin_path, b"wasm").expect("write plugin");
        std::fs::write(
            wasm_capabilities_sidecar_path(&plugin_path),
            r#"{"max_memory_mb":0}"#,
        )
        .expect("write sidecar");

        let error = resolve_wasm_resource_limits(&PluginEntry::default(), &plugin_path)
            .expect_err("zero max_memory_mb should fail");
        assert!(error.to_string().contains("max_memory_mb"));
    }
}
