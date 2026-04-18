use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::runtime::{Manifest, McpServerConfig, PluginType, ToolCatalog, TrustTier};

use super::package::{PluginPackage, RUNTIME_FILE};

pub fn validate_plugin_shape(manifest: &Manifest, runtime_raw: Option<&str>) -> Result<()> {
    if matches!(
        manifest.plugin_type,
        PluginType::Brain | PluginType::Workspace
    ) && matches!(manifest.trust_tier, TrustTier::Community)
    {
        bail!(
            "Native plugins require trust tier Official or Reviewed. '{}' is Community.",
            manifest.name
        );
    }

    for path in &manifest.permissions.filesystem {
        if path.0.trim().is_empty() {
            bail!(
                "Plugin '{}' declares an empty filesystem permission",
                manifest.name
            );
        }
    }

    for domain in &manifest.permissions.network {
        if domain.0.trim().is_empty() {
            bail!(
                "Plugin '{}' declares an empty network permission",
                manifest.name
            );
        }
    }

    for secret in &manifest.permissions.secrets {
        if secret.trim().is_empty() {
            bail!(
                "Plugin '{}' declares an empty secret permission",
                manifest.name
            );
        }
    }

    for pattern in &manifest.permissions.host_patterns {
        if pattern.0.trim().is_empty() {
            bail!(
                "Plugin '{}' declares an empty secret host pattern",
                manifest.name
            );
        }
    }

    if !manifest.permissions.secrets.is_empty() && manifest.permissions.host_patterns.is_empty() {
        bail!(
            "Plugin '{}' declares secret permissions but no host_patterns allowlist",
            manifest.name
        );
    }

    if matches!(manifest.plugin_type, PluginType::Mcp) && !manifest.permissions.tools.is_empty() {
        bail!(
            "MCP plugin '{}' cannot request builtin tool access in manifest permissions",
            manifest.name
        );
    }

    let runtime_raw = runtime_raw.context(format!(
        "Plugin '{}' is missing {}",
        manifest.name, RUNTIME_FILE
    ))?;

    match manifest.plugin_type {
        PluginType::Skill | PluginType::Channel | PluginType::Brain | PluginType::Workspace => {
            ToolCatalog::from_json(Some(runtime_raw))?;
        }
        PluginType::Mcp => {
            let config: McpServerConfig =
                serde_json::from_str(runtime_raw).context("Invalid MCP runtime config")?;
            if config.name.trim().is_empty() {
                bail!("MCP runtime config is missing a server name");
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PermissionReview {
    pub summary_lines: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn review_manifest_permissions(manifest: &Manifest) -> PermissionReview {
    let filesystem = if manifest.permissions.filesystem.is_empty() {
        "none".to_string()
    } else {
        manifest
            .permissions
            .filesystem
            .iter()
            .map(|pattern| pattern.0.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let network = if manifest.permissions.network.is_empty() {
        "none".to_string()
    } else {
        manifest
            .permissions
            .network
            .iter()
            .map(|pattern| pattern.0.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let builtin_tools = if manifest.permissions.tools.is_empty() {
        "none".to_string()
    } else {
        manifest.permissions.tools.join(", ")
    };
    let secret_keys = if manifest.permissions.secrets.is_empty() {
        "none".to_string()
    } else {
        manifest.permissions.secrets.join(", ")
    };
    let secret_hosts = if manifest.permissions.host_patterns.is_empty() {
        "none".to_string()
    } else {
        manifest
            .permissions
            .host_patterns
            .iter()
            .map(|pattern| pattern.0.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let wasm_memory_limit = manifest
        .permissions
        .wasm_max_memory_mb
        .map(|value| format!("{value} MB"))
        .unwrap_or_else(|| "default".to_string());
    let wasm_fuel_limit = manifest
        .permissions
        .wasm_fuel_limit
        .map(|value| value.to_string())
        .unwrap_or_else(|| "default".to_string());
    let execution_timeout = manifest
        .permissions
        .max_execution_time
        .map(|value| format!("{value}s"))
        .unwrap_or_else(|| "default".to_string());

    let mut warnings = Vec::new();
    for path in &manifest.permissions.filesystem {
        if is_broad_filesystem_pattern(&path.0) {
            warnings.push(format!(
                "filesystem permission '{}' is broader than recommended",
                path.0
            ));
        }
    }
    for domain in &manifest.permissions.network {
        if is_broad_network_pattern(&domain.0) {
            warnings.push(format!(
                "network permission '{}' is broader than recommended",
                domain.0
            ));
        }
    }
    for pattern in &manifest.permissions.host_patterns {
        if is_broad_network_pattern(&pattern.0) {
            warnings.push(format!(
                "secret host pattern '{}' is broader than recommended",
                pattern.0
            ));
        }
    }
    if manifest
        .permissions
        .wasm_max_memory_mb
        .is_some_and(|value| value > 256)
    {
        warnings.push("WASM memory limit is higher than recommended (over 256 MB)".to_string());
    }
    if manifest
        .permissions
        .wasm_fuel_limit
        .is_some_and(|value| value > 100_000_000)
    {
        warnings.push("WASM fuel limit is higher than recommended (over 100000000)".to_string());
    }
    if manifest
        .permissions
        .max_execution_time
        .is_some_and(|value| value > 300)
    {
        warnings.push("WASM execution timeout is higher than recommended (over 300s)".to_string());
    }
    if matches!(
        manifest.plugin_type,
        PluginType::Brain | PluginType::Workspace
    ) && !matches!(
        manifest.trust_tier,
        TrustTier::Official | TrustTier::Reviewed
    ) {
        warnings.push(format!(
            "{} plugins should use Official or Reviewed trust tiers",
            manifest.plugin_type.as_str()
        ));
    }

    PermissionReview {
        summary_lines: vec![
            format!("type: {}", manifest.plugin_type.as_str()),
            format!("trust tier: {:?}", manifest.trust_tier),
            format!("filesystem: {}", filesystem),
            format!("network: {}", network),
            format!("secrets: {}", secret_keys),
            format!("secret hosts: {}", secret_hosts),
            format!(
                "memory: read={} write={}",
                manifest.permissions.memory_read, manifest.permissions.memory_write
            ),
            format!(
                "wasm limits: timeout={} memory={} fuel={}",
                execution_timeout, wasm_memory_limit, wasm_fuel_limit
            ),
            format!("builtin tools: {}", builtin_tools),
        ],
        warnings,
    }
}

pub fn print_permission_review(manifest: &Manifest) {
    let review = review_manifest_permissions(manifest);
    println!("permissions:");
    for line in review.summary_lines {
        println!("- {}", line);
    }
    if !review.warnings.is_empty() {
        println!("warnings:");
        for warning in review.warnings {
            println!("- {}", warning);
        }
    }
}

pub fn resolve_payload_source(
    root: &Path,
    manifest: &Manifest,
    package: &PluginPackage,
    runtime_rel: Option<&str>,
) -> Result<Option<PathBuf>> {
    match manifest.plugin_type {
        PluginType::Skill | PluginType::Channel => {
            let path = resolve_artifact(root, package.artifact.as_deref(), "wasm")?;
            Ok(Some(path))
        }
        PluginType::Brain | PluginType::Workspace => {
            let path = resolve_artifact(root, package.artifact.as_deref(), native_extension())?;
            Ok(Some(path))
        }
        PluginType::Mcp => {
            if package.artifact.is_some() {
                bail!("MCP plugin packages cannot declare a binary artifact");
            }
            let runtime_rel = runtime_rel.context("MCP plugin packages require runtime.json")?;
            Ok(Some(root.join(runtime_rel)))
        }
    }
}

fn resolve_artifact(
    root: &Path,
    relative: Option<&str>,
    required_extension: &str,
) -> Result<PathBuf> {
    let path = if let Some(relative) = relative {
        root.join(relative)
    } else {
        autodetect_artifact(root, required_extension)?
    };

    if !path.exists() {
        bail!("Plugin artifact '{}' does not exist", path.display());
    }
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if extension != required_extension {
        bail!(
            "Plugin artifact '{}' must use .{}",
            path.display(),
            required_extension
        );
    }

    Ok(path)
}

fn autodetect_artifact(root: &Path, extension: &str) -> Result<PathBuf> {
    let mut matches = fs::read_dir(root)
        .with_context(|| format!("Failed to list plugin package '{}'", root.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
        .collect::<Vec<_>>();

    matches.sort();
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => bail!(
            "Plugin package '{}' is missing a .{} artifact and plugin-package.json does not specify one",
            root.display(),
            extension
        ),
        _ => bail!(
            "Plugin package '{}' has multiple .{} artifacts; declare one in plugin-package.json",
            root.display(),
            extension
        ),
    }
}

fn is_broad_filesystem_pattern(pattern: &str) -> bool {
    matches!(
        pattern,
        "*" | "/" | "." | "./" | "workspace" | "workspace/**"
    ) || pattern.ends_with("/**")
}

fn is_broad_network_pattern(pattern: &str) -> bool {
    pattern == "*" || pattern.starts_with("*.") || pattern.contains("://") || pattern.contains('/')
}

#[cfg(target_os = "macos")]
fn native_extension() -> &'static str {
    "dylib"
}

#[cfg(target_os = "linux")]
fn native_extension() -> &'static str {
    "so"
}

#[cfg(target_os = "windows")]
fn native_extension() -> &'static str {
    "dll"
}

#[cfg(test)]
mod tests {
    use crate::runtime::Manifest;

    use super::{review_manifest_permissions, validate_plugin_shape};

    #[test]
    fn review_manifest_permissions_flags_broad_network_access() {
        let manifest = Manifest::from_json(
            r#"{
                "name": "Broad Network",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": ["/"],
                    "network": ["*"],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Broad network plugin",
                "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
            }"#,
        )
        .expect("manifest");

        let review = review_manifest_permissions(&manifest);
        assert!(review
            .warnings
            .iter()
            .any(|warning| warning.contains("network permission '*'")));
        assert!(review
            .warnings
            .iter()
            .any(|warning| warning.contains("filesystem permission '/'")));
    }

    #[test]
    fn validate_plugin_shape_rejects_mcp_builtin_tool_access() {
        let manifest = Manifest::from_json(
            r#"{
                "name": "Bad MCP",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Mcp",
                "permissions": {
                    "filesystem": [],
                    "network": ["api.example.com"],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": ["run_command"]
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Bad MCP plugin",
                "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
            }"#,
        )
        .expect("manifest");

        let error = validate_plugin_shape(
            &manifest,
            Some(
                r#"{
                    "name": "bad-mcp",
                    "command": "bad-mcp",
                    "args": ["stdio"],
                    "profile": {"allow_network": true, "read_paths": [], "write_paths": [], "allow_tmp": true},
                    "cached_tools": [],
                    "enabled": true
                }"#,
            ),
        )
        .expect_err("mcp tool access should fail");

        assert!(error
            .to_string()
            .contains("cannot request builtin tool access"));
    }

    #[test]
    fn validate_plugin_shape_requires_host_patterns_for_secret_permissions() {
        let manifest = Manifest::from_json(
            r#"{
                "name": "Bad Secret Plugin",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": ["api.openai.com"],
                    "secrets": ["OPENAI_API_KEY"],
                    "host_patterns": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Bad secret plugin",
                "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
            }"#,
        )
        .expect("manifest");

        let error = validate_plugin_shape(
            &manifest,
            Some(
                r#"{
                    "tools": [{
                        "name": "call_model",
                        "description": "Call model",
                        "parameters": {},
                        "domains": ["all"]
                    }]
                }"#,
            ),
        )
        .expect_err("secret permissions should require host patterns");

        assert!(error.to_string().contains("host_patterns allowlist"));
    }

    #[test]
    fn review_manifest_permissions_includes_secret_permissions() {
        let manifest = Manifest::from_json(
            r#"{
                "name": "Secret Plugin",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": ["api.openai.com"],
                    "secrets": ["OPENAI_API_KEY"],
                    "host_patterns": ["api.openai.com"],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Secret plugin",
                "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
            }"#,
        )
        .expect("manifest");

        let review = review_manifest_permissions(&manifest);
        assert!(review
            .summary_lines
            .iter()
            .any(|line| line == "secrets: OPENAI_API_KEY"));
        assert!(review
            .summary_lines
            .iter()
            .any(|line| line == "secret hosts: api.openai.com"));
    }

    #[test]
    fn review_manifest_permissions_includes_wasm_limits() {
        let manifest = Manifest::from_json(
            r#"{
                "name": "Limited Plugin",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "wasm_max_memory_mb": 32,
                    "wasm_fuel_limit": 75000000,
                    "max_execution_time": 45,
                    "tools": []
                },
                "trust_tier": "Reviewed",
                "min_model": null,
                "description": "Limited plugin",
                "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
            }"#,
        )
        .expect("manifest");

        let review = review_manifest_permissions(&manifest);
        assert!(review
            .summary_lines
            .iter()
            .any(|line| line == "wasm limits: timeout=45s memory=32 MB fuel=75000000"));
    }
}
