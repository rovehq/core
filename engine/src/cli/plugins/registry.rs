use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signer, SigningKey};
use reqwest::StatusCode;
use semver::Version;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::runtime::Manifest;
use crate::security::crypto::CryptoModule;

use super::package::{MANIFEST_FILE, PACKAGE_FILE, RUNTIME_FILE};

const REGISTRY_SCHEMA_VERSION: &str = "2";
const LOCAL_DEV_REGISTRY_SIGNATURE: &str = "LOCAL_DEV_REGISTRY_SIGNATURE";
pub(super) const PLUGIN_INDEX_FILE: &str = "index.json";
pub(super) const REGISTRY_FILE: &str = "registry.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryCatalog {
    pub schema_version: String,
    pub generated_at: i64,
    #[serde(default)]
    pub signed_at: i64,
    #[serde(default)]
    pub signature: String,
    pub plugins: Vec<RegistryCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryCatalogEntry {
    pub id: String,
    pub name: String,
    pub plugin_type: String,
    pub trust_tier: String,
    pub latest_version: String,
    pub index_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryPluginIndex {
    pub schema_version: String,
    pub generated_at: i64,
    #[serde(default)]
    pub signed_at: i64,
    #[serde(default)]
    pub signature: String,
    pub plugin: RegistryCatalogEntry,
    pub versions: Vec<RegistryVersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryVersionEntry {
    pub version: String,
    pub published_at: i64,
    pub bundle_path: String,
    pub manifest_path: String,
    pub package_path: String,
    pub runtime_path: Option<String>,
    pub artifact_path: Option<String>,
    #[serde(default)]
    pub artifact_sidecar_path: Option<String>,
    pub readme_path: Option<String>,
    pub release_path: String,
}

#[derive(Debug, Clone)]
pub(super) struct PublishedBundle {
    pub version: String,
    pub published_at: i64,
    pub bundle_path: String,
    pub manifest_path: String,
    pub package_path: String,
    pub runtime_path: Option<String>,
    pub artifact_path: Option<String>,
    pub artifact_sidecar_path: Option<String>,
    pub readme_path: Option<String>,
    pub release_path: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct V2Index {
    pub schema_version: u32,
    pub channels: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct V2Manifest {
    pub schema_version: u32,
    pub channel: String,
    pub artifact: String,
    pub issuer: String,
    pub published_at: i64,
    pub min_engine: Option<String>,
    pub entries: std::collections::BTreeMap<String, V2Entry>,
    #[serde(default)]
    pub signature: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct V2Entry {
    pub version: String,
    pub trust_tier: u8,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub plugin_type: Option<String>,
    #[serde(default)]
    pub bundle_path: Option<String>,
    #[serde(default)]
    pub manifest_path: Option<String>,
    #[serde(default)]
    pub package_path: Option<String>,
    #[serde(default)]
    pub runtime_path: Option<String>,
    #[serde(default)]
    pub artifact_path: Option<String>,
    #[serde(default)]
    pub artifact_sidecar_path: Option<String>,
    #[serde(default)]
    pub readme_path: Option<String>,
    #[serde(default)]
    pub release_path: Option<String>,
    #[serde(default)]
    pub platforms: std::collections::BTreeMap<String, V2Platform>,
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct V2Platform {
    pub url: String,
    pub fallback_url: Option<String>,
    pub blake3: String,
    pub size_bytes: u64,
}

enum RegistryLocation {
    Local(PathBuf),
    Remote(String),
}

pub(crate) async fn load_registry_catalog(registry: &str) -> Result<RegistryCatalog> {
    let location = parse_registry_location(registry);
    enforce_remote_registry_policy(&location)?;

    // Always try V2 first
    if let Ok(catalog) = load_v2_catalog(&location).await {
        return Ok(catalog);
    }

    // Fallback to V1
    let raw = match &location {
        RegistryLocation::Local(root) => fs::read_to_string(root.join(REGISTRY_FILE))
            .with_context(|| format!("Failed to read '{}'", root.join(REGISTRY_FILE).display()))?,
        RegistryLocation::Remote(base) => {
            fetch_remote_text(&join_remote(base, REGISTRY_FILE)).await?
        }
    };

    if matches!(location, RegistryLocation::Remote(_)) {
        verify_signed_registry_json(&raw, "registry catalog")?;
    }

    serde_json::from_str(&raw).context("Invalid plugin registry catalog")
}

async fn load_v2_catalog(location: &RegistryLocation) -> Result<RegistryCatalog> {
    let index_raw = match location {
        RegistryLocation::Local(root) => fs::read_to_string(root.join("index.json"))?,
        RegistryLocation::Remote(base) => fetch_remote_text(&join_remote(base, "index.json")).await?,
    };

    if matches!(location, RegistryLocation::Remote(_)) {
        verify_signed_registry_json(&index_raw, "registry index")?;
    }

    let index: V2Index = serde_json::from_str(&index_raw)?;
    if index.schema_version != 2 {
        bail!("Expected schema_version 2");
    }

    // Try to get dev or stable channel
    let channel_map = index.channels.get("dev").or_else(|| index.channels.get("stable")).context("No dev or stable channel in index.json")?;
    
    let mut catalog = RegistryCatalog {
        schema_version: REGISTRY_SCHEMA_VERSION.to_string(),
        generated_at: 0,
        signed_at: 0,
        signature: String::new(),
        plugins: Vec::new(),
    };

    for artifact_type in &["plugins", "drivers"] {
        if let Some(manifest_rel) = channel_map.get(*artifact_type) {
            let manifest_raw = match location {
                RegistryLocation::Local(root) => fs::read_to_string(root.join(manifest_rel)).unwrap_or_default(),
                RegistryLocation::Remote(base) => fetch_remote_text(&join_remote(base, manifest_rel)).await.unwrap_or_default(),
            };
            if manifest_raw.is_empty() { continue; }
            if matches!(location, RegistryLocation::Remote(_)) {
                let _ = verify_signed_registry_json(&manifest_raw, "registry manifest");
            }
            if let Ok(manifest) = serde_json::from_str::<V2Manifest>(&manifest_raw) {
                if catalog.generated_at == 0 {
                    catalog.generated_at = manifest.published_at;
                }
                for (id, entry) in manifest.entries {
                    let trust_tier = match entry.trust_tier {
                        0 => "Official",
                        1 => "Reviewed",
                        2 => "Community",
                        _ => "Unverified",
                    };
                    catalog.plugins.push(RegistryCatalogEntry {
                        id: id.clone(),
                        name: entry.name.clone().unwrap_or(id.clone()),
                        plugin_type: entry.plugin_type.clone().unwrap_or_else(|| "Plugin".to_string()),
                        trust_tier: trust_tier.to_string(),
                        latest_version: entry.version,
                        index_path: format!("{}/{}", id, PLUGIN_INDEX_FILE),
                    });
                }
            }
        }
    }

    Ok(catalog)
}

pub(crate) async fn load_registry_plugin_index(
    registry: &str,
    plugin_id: &str,
) -> Result<RegistryPluginIndex> {
    let location = parse_registry_location(registry);
    enforce_remote_registry_policy(&location)?;

    if let Ok(index) = load_v2_plugin_index(&location, plugin_id).await {
        return Ok(index);
    }

    let raw = match &location {
        RegistryLocation::Local(root) => {
            fs::read_to_string(root.join(plugin_id).join(PLUGIN_INDEX_FILE)).with_context(|| {
                format!(
                    "Failed to read plugin index '{}'",
                    root.join(plugin_id).join(PLUGIN_INDEX_FILE).display()
                )
            })?
        }
        RegistryLocation::Remote(base) => {
            fetch_remote_text(&join_remote(
                base,
                &format!("{plugin_id}/{PLUGIN_INDEX_FILE}"),
            ))
            .await?
        }
    };

    if matches!(location, RegistryLocation::Remote(_)) {
        verify_signed_registry_json(&raw, &format!("plugin index for {}", plugin_id))?;
    }

    serde_json::from_str(&raw).context("Invalid plugin registry index")
}

async fn load_v2_plugin_index(location: &RegistryLocation, plugin_id: &str) -> Result<RegistryPluginIndex> {
    let index_raw = match location {
        RegistryLocation::Local(root) => fs::read_to_string(root.join("index.json"))?,
        RegistryLocation::Remote(base) => fetch_remote_text(&join_remote(base, "index.json")).await?,
    };
    let index: V2Index = serde_json::from_str(&index_raw)?;
    let channel_map = index.channels.get("dev").or_else(|| index.channels.get("stable")).context("No channel map")?;
    
    for artifact_type in &["plugins", "drivers"] {
        if let Some(manifest_rel) = channel_map.get(*artifact_type) {
            let manifest_raw = match location {
                RegistryLocation::Local(root) => fs::read_to_string(root.join(manifest_rel)).unwrap_or_default(),
                RegistryLocation::Remote(base) => fetch_remote_text(&join_remote(base, manifest_rel)).await.unwrap_or_default(),
            };
            if let Ok(manifest) = serde_json::from_str::<V2Manifest>(&manifest_raw) {
                if let Some(entry) = manifest.entries.get(plugin_id) {
                    let trust_tier = match entry.trust_tier {
                        0 => "Official", 1 => "Reviewed", 2 => "Community", _ => "Unverified",
                    };
                    return Ok(RegistryPluginIndex {
                        schema_version: "2".to_string(),
                        generated_at: manifest.published_at,
                        signed_at: manifest.published_at,
                        signature: manifest.signature.clone(),
                        plugin: RegistryCatalogEntry {
                            id: plugin_id.to_string(),
                            name: entry.name.clone().unwrap_or(plugin_id.to_string()),
                            plugin_type: entry.plugin_type.clone().unwrap_or_else(|| "Plugin".to_string()),
                            trust_tier: trust_tier.to_string(),
                            latest_version: entry.version.clone(),
                            index_path: format!("{}/{}", plugin_id, PLUGIN_INDEX_FILE),
                        },
                        versions: vec![RegistryVersionEntry {
                            version: entry.version.clone(),
                            published_at: manifest.published_at,
                            bundle_path: entry.bundle_path.clone().unwrap_or_default(),
                            manifest_path: entry.manifest_path.clone().unwrap_or_default(),
                            package_path: entry.package_path.clone().unwrap_or_default(),
                            runtime_path: entry.runtime_path.clone(),
                            artifact_path: entry.artifact_path.clone(),
                            artifact_sidecar_path: entry.artifact_sidecar_path.clone(),
                            readme_path: entry.readme_path.clone(),
                            release_path: entry.release_path.clone().unwrap_or_default(),
                        }],
                    });
                }
            }
        }
    }
    bail!("Plugin not found in V2 registry");
}

pub(crate) async fn read_registry_text(registry: &str, relative: &str) -> Result<String> {
    let location = parse_registry_location(registry);
    enforce_remote_registry_policy(&location)?;

    match &location {
        RegistryLocation::Local(root) => {
            let source = root.join(relative);
            fs::read_to_string(&source)
                .with_context(|| format!("Failed to read '{}'", source.display()))
        }
        RegistryLocation::Remote(base) => fetch_remote_text(&join_remote(base, relative)).await,
    }
}

pub(crate) fn select_registry_version<'a>(
    index: &'a RegistryPluginIndex,
    version: Option<&str>,
) -> Result<&'a RegistryVersionEntry> {
    select_version(index, version)
}

pub(super) fn update_registry_metadata(
    registry_dir: &Path,
    plugin_id: &str,
    manifest: &Manifest,
    published: PublishedBundle,
) -> Result<()> {
    // Write V1 metadata (for compatibility with existing local setups and tests)
    write_v1_registry_metadata(registry_dir, plugin_id, manifest, &published)?;
    
    // Write V2 metadata
    write_v2_registry_metadata(registry_dir, plugin_id, manifest, &published)?;

    Ok(())
}

fn write_v1_registry_metadata(
    registry_dir: &Path,
    plugin_id: &str,
    manifest: &Manifest,
    published: &PublishedBundle,
) -> Result<()> {
    fs::create_dir_all(registry_dir)?;
    let plugin_dir = registry_dir.join(plugin_id);
    fs::create_dir_all(&plugin_dir)?;

    let mut plugin_index = read_local_json::<RegistryPluginIndex>(
        &plugin_dir.join(PLUGIN_INDEX_FILE),
    )
    .unwrap_or_else(|_| RegistryPluginIndex {
        schema_version: "1".to_string(),
        generated_at: published.published_at,
        signed_at: 0,
        signature: String::new(),
        plugin: RegistryCatalogEntry {
            id: plugin_id.to_string(),
            name: manifest.name.clone(),
            plugin_type: manifest.plugin_type.as_str().to_string(),
            trust_tier: format!("{:?}", manifest.trust_tier),
            latest_version: manifest.version.clone(),
            index_path: format!("{plugin_id}/{PLUGIN_INDEX_FILE}"),
        },
        versions: Vec::new(),
    });

    plugin_index.generated_at = published.published_at;
    plugin_index.plugin.name = manifest.name.clone();
    plugin_index.plugin.plugin_type = manifest.plugin_type.as_str().to_string();
    plugin_index.plugin.trust_tier = format!("{:?}", manifest.trust_tier);
    plugin_index.versions.retain(|entry| entry.version != manifest.version);
    plugin_index.versions.push(RegistryVersionEntry {
        version: published.version.clone(),
        published_at: published.published_at,
        bundle_path: published.bundle_path.clone(),
        manifest_path: published.manifest_path.clone(),
        package_path: published.package_path.clone(),
        runtime_path: published.runtime_path.clone(),
        artifact_path: published.artifact_path.clone(),
        artifact_sidecar_path: published.artifact_sidecar_path.clone(),
        readme_path: published.readme_path.clone(),
        release_path: published.release_path.clone(),
    });
    sort_versions_desc(&mut plugin_index.versions);
    plugin_index.plugin.latest_version = plugin_index.versions.first().map(|e| e.version.clone()).unwrap_or_else(|| manifest.version.clone());

    let mut plugin_index_json = serde_json::to_value(&plugin_index)?;
    sign_registry_json(&mut plugin_index_json, published.published_at)?;
    fs::write(
        plugin_dir.join(PLUGIN_INDEX_FILE),
        serde_json::to_string_pretty(&plugin_index_json)?,
    )?;

    let mut registry = read_local_json::<RegistryCatalog>(&registry_dir.join(REGISTRY_FILE))
        .unwrap_or_else(|_| RegistryCatalog {
            schema_version: "1".to_string(),
            generated_at: published.published_at,
            signed_at: 0,
            signature: String::new(),
            plugins: Vec::new(),
        });
    registry.generated_at = published.published_at;
    registry.plugins.retain(|entry| entry.id != plugin_id);
    registry.plugins.push(plugin_index.plugin.clone());
    registry.plugins.sort_by(|left, right| left.name.cmp(&right.name));

    let mut registry_json = serde_json::to_value(&registry)?;
    sign_registry_json(&mut registry_json, published.published_at)?;
    fs::write(
        registry_dir.join(REGISTRY_FILE),
        serde_json::to_string_pretty(&registry_json)?,
    )?;

    Ok(())
}

fn write_v2_registry_metadata(
    registry_dir: &Path,
    plugin_id: &str,
    manifest: &Manifest,
    published: &PublishedBundle,
) -> Result<()> {
    // Generate index.json
    let index_path = registry_dir.join("index.json");
    let index = read_local_json::<V2Index>(&index_path).unwrap_or_else(|_| {
        let mut channels = std::collections::BTreeMap::new();
        let mut dev_channel = std::collections::BTreeMap::new();
        dev_channel.insert("plugins".to_string(), "dev/plugins/manifest.json".to_string());
        dev_channel.insert("drivers".to_string(), "dev/drivers/manifest.json".to_string());
        channels.insert("dev".to_string(), dev_channel);
        V2Index {
            schema_version: 2,
            channels,
        }
    });
    fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;

    // Generate manifest.json for dev/plugins or dev/drivers depending on plugin_type
    let artifact_type = if manifest.plugin_type.as_str() == "Workspace" { "drivers" } else { "plugins" };
    let v2_manifest_dir = registry_dir.join("dev").join(artifact_type);
    fs::create_dir_all(&v2_manifest_dir)?;
    
    let manifest_path = v2_manifest_dir.join("manifest.json");
    let mut v2_manifest = read_local_json::<V2Manifest>(&manifest_path).unwrap_or_else(|_| V2Manifest {
        schema_version: 2,
        channel: "dev".to_string(),
        artifact: artifact_type.to_string(),
        issuer: "rove-team".to_string(),
        published_at: published.published_at,
        min_engine: None,
        entries: std::collections::BTreeMap::new(),
        signature: String::new(),
    });

    let trust_tier = match manifest.trust_tier {
        crate::runtime::TrustTier::Official => 0,
        crate::runtime::TrustTier::Reviewed => 1,
        crate::runtime::TrustTier::Community => 2,
    };

    v2_manifest.published_at = published.published_at;
    v2_manifest.entries.insert(plugin_id.to_string(), V2Entry {
        version: manifest.version.clone(),
        trust_tier,
        name: Some(manifest.name.clone()),
        plugin_type: Some(manifest.plugin_type.as_str().to_string()),
        bundle_path: Some(published.bundle_path.clone()),
        manifest_path: Some(published.manifest_path.clone()),
        package_path: Some(published.package_path.clone()),
        runtime_path: published.runtime_path.clone(),
        artifact_path: published.artifact_path.clone(),
        artifact_sidecar_path: published.artifact_sidecar_path.clone(),
        readme_path: published.readme_path.clone(),
        release_path: Some(published.release_path.clone()),
        platforms: std::collections::BTreeMap::new(),
    });

    let mut v2_manifest_json = serde_json::to_value(&v2_manifest)?;
    sign_registry_json(&mut v2_manifest_json, published.published_at)?;
    fs::write(&manifest_path, serde_json::to_string_pretty(&v2_manifest_json)?)?;

    Ok(())
}

pub(super) async fn materialize_registry_bundle(
    registry: &str,
    plugin_id: &str,
    version: Option<&str>,
) -> Result<TempDir> {
    let location = parse_registry_location(registry);
    enforce_remote_registry_policy(&location)?;
    let index = load_plugin_index(&location, plugin_id).await?;
    let entry = select_version(&index, version)?;
    let temp_dir = TempDir::new().context("Failed to create temporary plugin bundle directory")?;

    if matches!(location, RegistryLocation::Remote(_)) {
        if !entry.release_path.is_empty() {
            let release_raw = fetch_remote_text(&join_remote(
                match &location {
                    RegistryLocation::Remote(base) => base,
                    RegistryLocation::Local(_) => unreachable!("guarded above"),
                },
                &entry.release_path,
            ))
            .await?;
            verify_signed_registry_json(&release_raw, &format!("release metadata for {}", plugin_id))?;
        }
    }

    if !entry.manifest_path.is_empty() {
        fetch_text_into(&location, &entry.manifest_path, &temp_dir.path().join(MANIFEST_FILE)).await?;
    }
    if !entry.package_path.is_empty() {
        fetch_text_into(&location, &entry.package_path, &temp_dir.path().join(PACKAGE_FILE)).await?;
    }
    if let Some(runtime_path) = &entry.runtime_path {
        fetch_text_into(&location, runtime_path, &temp_dir.path().join(RUNTIME_FILE)).await?;
    }
    if let Some(artifact_path) = &entry.artifact_path {
        let destination = temp_dir.path().join(file_name_from_relative(artifact_path)?);
        fetch_bytes_into(&location, artifact_path, &destination).await?;
    }
    if let Some(artifact_sidecar_path) = &entry.artifact_sidecar_path {
        let destination = temp_dir.path().join(file_name_from_relative(artifact_sidecar_path)?);
        fetch_text_into(&location, artifact_sidecar_path, &destination).await?;
    }
    if let Some(readme_path) = &entry.readme_path {
        let destination = temp_dir.path().join(file_name_from_relative(readme_path)?);
        fetch_text_into(&location, readme_path, &destination).await?;
    }

    Ok(temp_dir)
}

fn enforce_remote_registry_policy(location: &RegistryLocation) -> Result<()> {
    let RegistryLocation::Remote(base) = location else {
        return Ok(());
    };
    if base.starts_with("https://") || base.starts_with("http://localhost") || base.starts_with("http://127.0.0.1") {
        return Ok(());
    }
    bail!("Remote plugin registries must use HTTPS or localhost. '{}' is not allowed", base)
}

fn parse_registry_location(registry: &str) -> RegistryLocation {
    if registry.starts_with("https://") || registry.starts_with("http://") {
        RegistryLocation::Remote(registry.trim_end_matches('/').to_string())
    } else {
        RegistryLocation::Local(PathBuf::from(registry))
    }
}

async fn load_plugin_index(
    location: &RegistryLocation,
    plugin_id: &str,
) -> Result<RegistryPluginIndex> {
    if let Ok(index) = load_v2_plugin_index(location, plugin_id).await {
        return Ok(index);
    }

    let raw = match location {
        RegistryLocation::Local(root) => {
            fs::read_to_string(root.join(plugin_id).join(PLUGIN_INDEX_FILE)).with_context(|| {
                format!("Failed to read plugin index '{}'", root.join(plugin_id).join(PLUGIN_INDEX_FILE).display())
            })?
        }
        RegistryLocation::Remote(base) => {
            fetch_remote_text(&join_remote(base, &format!("{plugin_id}/{PLUGIN_INDEX_FILE}"))).await?
        }
    };

    if matches!(location, RegistryLocation::Remote(_)) {
        verify_signed_registry_json(&raw, &format!("plugin index for {}", plugin_id))?;
    }

    serde_json::from_str(&raw).context("Invalid plugin registry index")
}

fn select_version<'a>(
    index: &'a RegistryPluginIndex,
    version: Option<&str>,
) -> Result<&'a RegistryVersionEntry> {
    if let Some(version) = version {
        return index.versions.iter().find(|entry| entry.version == version).with_context(|| {
            format!("Plugin '{}' does not publish version '{}'", index.plugin.id, version)
        });
    }
    if let Some(entry) = index.versions.iter().find(|entry| entry.version == index.plugin.latest_version) {
        return Ok(entry);
    }
    index.versions.first().context("Plugin registry index does not contain any versions")
}

async fn fetch_text_into(
    location: &RegistryLocation,
    relative: &str,
    destination: &Path,
) -> Result<()> {
    match location {
        RegistryLocation::Local(root) => {
            let source = root.join(relative);
            let content = fs::read_to_string(&source).with_context(|| format!("Failed to read '{}'", source.display()))?;
            fs::write(destination, content).with_context(|| format!("Failed to write '{}'", destination.display()))?;
        }
        RegistryLocation::Remote(base) => {
            let content = fetch_remote_text(&join_remote(base, relative)).await?;
            fs::write(destination, content).with_context(|| format!("Failed to write '{}'", destination.display()))?;
        }
    }
    Ok(())
}

async fn fetch_bytes_into(
    location: &RegistryLocation,
    relative: &str,
    destination: &Path,
) -> Result<()> {
    match location {
        RegistryLocation::Local(root) => {
            let source = root.join(relative);
            fs::copy(&source, destination).with_context(|| {
                format!("Failed to copy '{}' to '{}'", source.display(), destination.display())
            })?;
        }
        RegistryLocation::Remote(base) => {
            let bytes = fetch_remote_bytes(&join_remote(base, relative)).await?;
            fs::write(destination, bytes).with_context(|| format!("Failed to write '{}'", destination.display()))?;
        }
    }
    Ok(())
}

async fn fetch_remote_text(url: &str) -> Result<String> {
    let response = reqwest::get(url).await.with_context(|| format!("Failed to fetch '{}'", url))?;
    if response.status() == StatusCode::NOT_FOUND {
        bail!("Remote plugin registry entry '{}' was not found", url);
    }
    response.error_for_status().with_context(|| format!("Failed to fetch '{}'", url))?.text().await.with_context(|| format!("Failed to read response body from '{}'", url))
}

async fn fetch_remote_bytes(url: &str) -> Result<Vec<u8>> {
    let response = reqwest::get(url).await.with_context(|| format!("Failed to fetch '{}'", url))?;
    if response.status() == StatusCode::NOT_FOUND {
        bail!("Remote plugin artifact '{}' was not found", url);
    }
    response.error_for_status().with_context(|| format!("Failed to fetch '{}'", url))?.bytes().await.map(|bytes| bytes.to_vec()).with_context(|| format!("Failed to read response body from '{}'", url))
}

fn join_remote(base: &str, relative: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), relative.trim_start_matches('/'))
}

fn file_name_from_relative(relative: &str) -> Result<&str> {
    Path::new(relative).file_name().and_then(|name| name.to_str()).context("Registry entry path is missing a file name")
}

pub(super) fn sign_registry_json(value: &mut serde_json::Value, signed_at: i64) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        bail!("Registry metadata must be a JSON object");
    };
    object.insert("signed_at".to_string(), serde_json::json!(signed_at));
    let signature = resolve_registry_signature(value)?;
    let Some(object) = value.as_object_mut() else {
        bail!("Registry metadata must be a JSON object");
    };
    object.insert("signature".to_string(), serde_json::Value::String(signature));
    Ok(())
}

fn resolve_registry_signature(value: &serde_json::Value) -> Result<String> {
    let Some(signing_key) = load_registry_signing_key()? else {
        return Ok(LOCAL_DEV_REGISTRY_SIGNATURE.to_string());
    };
    let canonical = CryptoModule::canonicalize_manifest(
        serde_json::to_vec(value).context("Failed to serialize registry metadata for signing")?.as_slice(),
    )?;
    Ok(hex::encode(signing_key.sign(&canonical).to_bytes()))
}

pub(super) fn load_registry_signing_key() -> Result<Option<SigningKey>> {
    let Some(raw) = std::env::var("ROVE_REGISTRY_PRIVATE_KEY")
        .ok()
        .or_else(|| std::env::var("ROVE_TEAM_PRIVATE_KEY").ok())
    else {
        return Ok(None);
    };
    let bytes = hex::decode(raw.trim()).context("Failed to decode registry signing key hex")?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("Registry signing key must be 32 bytes"))?;
    Ok(Some(SigningKey::from_bytes(&bytes)))
}

pub(crate) fn verify_signed_registry_json(raw: &str, label: &str) -> Result<()> {
    let crypto = CryptoModule::new().context("Failed to initialize registry verifier")?;
    crypto.verify_manifest_file(raw.as_bytes()).with_context(|| format!("Unsigned or invalid {} metadata", label))?;
    Ok(())
}

fn read_local_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let raw = fs::read_to_string(path).with_context(|| format!("Failed to read '{}'", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("Invalid JSON in '{}'", path.display()))
}

fn sort_versions_desc(entries: &mut [RegistryVersionEntry]) {
    entries.sort_by(|left, right| compare_versions_desc(&left.version, &right.version));
}

fn compare_versions_desc(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => right.cmp(&left),
        _ => right.cmp(left),
    }
}
