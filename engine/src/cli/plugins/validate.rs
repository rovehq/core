use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

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
