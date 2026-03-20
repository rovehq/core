use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::runtime::Manifest;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const PACKAGE_FILE: &str = "plugin-package.json";
pub const RUNTIME_FILE: &str = "runtime.json";

#[derive(Debug, Deserialize)]
pub struct PluginPackage {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub artifact: Option<String>,
    #[serde(default)]
    pub runtime_config: Option<String>,
    #[serde(alias = "artifact_hash")]
    pub payload_hash: String,
    #[serde(alias = "artifact_signature")]
    pub payload_signature: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

pub fn resolve_package_root(source: &Path) -> Result<PathBuf> {
    if source.is_dir() {
        return Ok(source.to_path_buf());
    }

    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if matches!(file_name, MANIFEST_FILE | PACKAGE_FILE | RUNTIME_FILE) {
        return source
            .parent()
            .map(Path::to_path_buf)
            .context("Plugin package file has no parent directory");
    }

    bail!(
        "Plugin source '{}' must be a package directory or one of {} / {} / {}",
        source.display(),
        MANIFEST_FILE,
        PACKAGE_FILE,
        RUNTIME_FILE
    )
}

pub fn load_package(root: &Path) -> Result<PluginPackage> {
    let raw = read_required_file(&root.join(PACKAGE_FILE))?;
    serde_json::from_str(&raw).context("Invalid plugin-package.json")
}

pub fn load_runtime_config(root: &Path, relative: Option<&str>) -> Result<Option<String>> {
    match relative {
        Some(relative) => read_required_file(&root.join(relative)).map(Some),
        None => Ok(None),
    }
}

pub fn read_required_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("Failed to read '{}'", path.display()))
}

pub fn default_runtime_file(root: &Path) -> Option<String> {
    let path = root.join(RUNTIME_FILE);
    if path.exists() {
        Some(RUNTIME_FILE.to_string())
    } else {
        None
    }
}

pub fn default_plugin_id(name: &str) -> String {
    let mut id = String::new();
    let mut last_dash = false;

    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            id.push(lower);
            last_dash = false;
        } else if !last_dash {
            id.push('-');
            last_dash = true;
        }
    }

    let normalized = id.trim_matches('-').to_string();
    if normalized.is_empty() {
        default_plugin_id_from_manifest(name)
    } else {
        normalized
    }
}

fn default_plugin_id_from_manifest(name: &str) -> String {
    let fallback = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    if fallback.is_empty() {
        "plugin".to_string()
    } else {
        fallback
    }
}

fn default_enabled() -> bool {
    true
}

pub fn manifest_from_signed_json(raw: &str) -> Result<Manifest> {
    Ok(Manifest::from_json(raw)?)
}
