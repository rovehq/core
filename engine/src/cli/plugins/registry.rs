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

const REGISTRY_SCHEMA_VERSION: &str = "1";
pub(super) const REGISTRY_FILE: &str = "registry.json";
pub(super) const PLUGIN_INDEX_FILE: &str = "index.json";
const LOCAL_DEV_REGISTRY_SIGNATURE: &str = "LOCAL_DEV_REGISTRY_SIGNATURE";

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
    pub readme_path: Option<String>,
    pub release_path: String,
}

enum RegistryLocation {
    Local(PathBuf),
    Remote(String),
}

pub(crate) async fn load_registry_catalog(registry: &str) -> Result<RegistryCatalog> {
    let location = parse_registry_location(registry);
    enforce_remote_registry_policy(&location)?;
    let raw = match &location {
        RegistryLocation::Local(root) => fs::read_to_string(root.join(REGISTRY_FILE))
            .with_context(|| format!("Failed to read '{}'", root.join(REGISTRY_FILE).display()))?,
        RegistryLocation::Remote(base) => fetch_remote_text(&join_remote(base, REGISTRY_FILE)).await?,
    };

    if matches!(location, RegistryLocation::Remote(_)) {
        verify_signed_registry_json(&raw, "registry catalog")?;
    }

    serde_json::from_str(&raw).context("Invalid plugin registry catalog")
}

pub(crate) async fn load_registry_plugin_index(
    registry: &str,
    plugin_id: &str,
) -> Result<RegistryPluginIndex> {
    let location = parse_registry_location(registry);
    enforce_remote_registry_policy(&location)?;
    load_plugin_index(&location, plugin_id).await
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
    fs::create_dir_all(registry_dir)
        .with_context(|| format!("Failed to create '{}'", registry_dir.display()))?;

    let plugin_dir = registry_dir.join(plugin_id);
    fs::create_dir_all(&plugin_dir)
        .with_context(|| format!("Failed to create '{}'", plugin_dir.display()))?;

    let mut plugin_index = read_local_json::<RegistryPluginIndex>(
        &plugin_dir.join(PLUGIN_INDEX_FILE),
    )
    .unwrap_or_else(|_| RegistryPluginIndex {
        schema_version: REGISTRY_SCHEMA_VERSION.to_string(),
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
    plugin_index
        .versions
        .retain(|entry| entry.version != manifest.version);
    plugin_index.versions.push(RegistryVersionEntry {
        version: published.version.clone(),
        published_at: published.published_at,
        bundle_path: published.bundle_path.clone(),
        manifest_path: published.manifest_path.clone(),
        package_path: published.package_path.clone(),
        runtime_path: published.runtime_path.clone(),
        artifact_path: published.artifact_path.clone(),
        readme_path: published.readme_path.clone(),
        release_path: published.release_path.clone(),
    });
    sort_versions_desc(&mut plugin_index.versions);
    plugin_index.plugin.latest_version = plugin_index
        .versions
        .first()
        .map(|entry| entry.version.clone())
        .unwrap_or_else(|| manifest.version.clone());

    let mut plugin_index_json = serde_json::to_value(&plugin_index)?;
    sign_registry_json(&mut plugin_index_json, published.published_at)?;
    fs::write(
        plugin_dir.join(PLUGIN_INDEX_FILE),
        serde_json::to_string_pretty(&plugin_index_json)?,
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            plugin_dir.join(PLUGIN_INDEX_FILE).display()
        )
    })?;

    let mut registry = read_local_json::<RegistryCatalog>(&registry_dir.join(REGISTRY_FILE))
        .unwrap_or_else(|_| RegistryCatalog {
            schema_version: REGISTRY_SCHEMA_VERSION.to_string(),
            generated_at: published.published_at,
            signed_at: 0,
            signature: String::new(),
            plugins: Vec::new(),
        });
    registry.generated_at = published.published_at;
    registry.plugins.retain(|entry| entry.id != plugin_id);
    registry.plugins.push(plugin_index.plugin.clone());
    registry
        .plugins
        .sort_by(|left, right| left.name.cmp(&right.name));

    let mut registry_json = serde_json::to_value(&registry)?;
    sign_registry_json(&mut registry_json, published.published_at)?;
    fs::write(
        registry_dir.join(REGISTRY_FILE),
        serde_json::to_string_pretty(&registry_json)?,
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            registry_dir.join(REGISTRY_FILE).display()
        )
    })?;

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

    fetch_text_into(
        &location,
        &entry.manifest_path,
        &temp_dir.path().join(MANIFEST_FILE),
    )
    .await?;
    fetch_text_into(
        &location,
        &entry.package_path,
        &temp_dir.path().join(PACKAGE_FILE),
    )
    .await?;
    if let Some(runtime_path) = &entry.runtime_path {
        fetch_text_into(&location, runtime_path, &temp_dir.path().join(RUNTIME_FILE)).await?;
    }
    if let Some(artifact_path) = &entry.artifact_path {
        let destination = temp_dir
            .path()
            .join(file_name_from_relative(artifact_path)?);
        fetch_bytes_into(&location, artifact_path, &destination).await?;
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

    if base.starts_with("https://")
        || base.starts_with("http://localhost")
        || base.starts_with("http://127.0.0.1")
    {
        return Ok(());
    }

    bail!(
        "Remote plugin registries must use HTTPS or localhost. '{}' is not allowed",
        base
    )
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
    let raw = match location {
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

fn select_version<'a>(
    index: &'a RegistryPluginIndex,
    version: Option<&str>,
) -> Result<&'a RegistryVersionEntry> {
    if let Some(version) = version {
        return index
            .versions
            .iter()
            .find(|entry| entry.version == version)
            .with_context(|| {
                format!(
                    "Plugin '{}' does not publish version '{}'",
                    index.plugin.id, version
                )
            });
    }

    if let Some(entry) = index
        .versions
        .iter()
        .find(|entry| entry.version == index.plugin.latest_version)
    {
        return Ok(entry);
    }

    index
        .versions
        .first()
        .context("Plugin registry index does not contain any versions")
}

async fn fetch_text_into(
    location: &RegistryLocation,
    relative: &str,
    destination: &Path,
) -> Result<()> {
    match location {
        RegistryLocation::Local(root) => {
            let source = root.join(relative);
            let content = fs::read_to_string(&source)
                .with_context(|| format!("Failed to read '{}'", source.display()))?;
            fs::write(destination, content)
                .with_context(|| format!("Failed to write '{}'", destination.display()))?;
        }
        RegistryLocation::Remote(base) => {
            let content = fetch_remote_text(&join_remote(base, relative)).await?;
            fs::write(destination, content)
                .with_context(|| format!("Failed to write '{}'", destination.display()))?;
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
                format!(
                    "Failed to copy '{}' to '{}'",
                    source.display(),
                    destination.display()
                )
            })?;
        }
        RegistryLocation::Remote(base) => {
            let bytes = fetch_remote_bytes(&join_remote(base, relative)).await?;
            fs::write(destination, bytes)
                .with_context(|| format!("Failed to write '{}'", destination.display()))?;
        }
    }

    Ok(())
}

async fn fetch_remote_text(url: &str) -> Result<String> {
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to fetch '{}'", url))?;
    if response.status() == StatusCode::NOT_FOUND {
        bail!("Remote plugin registry entry '{}' was not found", url);
    }
    response
        .error_for_status()
        .with_context(|| format!("Failed to fetch '{}'", url))?
        .text()
        .await
        .with_context(|| format!("Failed to read response body from '{}'", url))
}

async fn fetch_remote_bytes(url: &str) -> Result<Vec<u8>> {
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to fetch '{}'", url))?;
    if response.status() == StatusCode::NOT_FOUND {
        bail!("Remote plugin artifact '{}' was not found", url);
    }
    response
        .error_for_status()
        .with_context(|| format!("Failed to fetch '{}'", url))?
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .with_context(|| format!("Failed to read response body from '{}'", url))
}

fn join_remote(base: &str, relative: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        relative.trim_start_matches('/')
    )
}

fn file_name_from_relative(relative: &str) -> Result<&str> {
    Path::new(relative)
        .file_name()
        .and_then(|name| name.to_str())
        .context("Registry entry path is missing a file name")
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
    object.insert(
        "signature".to_string(),
        serde_json::Value::String(signature),
    );
    Ok(())
}

fn resolve_registry_signature(value: &serde_json::Value) -> Result<String> {
    let Some(signing_key) = load_registry_signing_key()? else {
        return Ok(LOCAL_DEV_REGISTRY_SIGNATURE.to_string());
    };

    let canonical = CryptoModule::canonicalize_manifest(
        serde_json::to_vec(value)
            .context("Failed to serialize registry metadata for signing")?
            .as_slice(),
    )?;
    Ok(hex::encode(signing_key.sign(&canonical).to_bytes()))
}

fn load_registry_signing_key() -> Result<Option<SigningKey>> {
    let Some(raw) = std::env::var("ROVE_REGISTRY_PRIVATE_KEY")
        .ok()
        .or_else(|| std::env::var("ROVE_TEAM_PRIVATE_KEY").ok())
    else {
        return Ok(None);
    };

    let bytes = hex::decode(raw.trim()).context("Failed to decode registry signing key hex")?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Registry signing key must be 32 bytes"))?;
    Ok(Some(SigningKey::from_bytes(&bytes)))
}

pub(crate) fn verify_signed_registry_json(raw: &str, label: &str) -> Result<()> {
    let crypto = CryptoModule::new().context("Failed to initialize registry verifier")?;
    crypto
        .verify_manifest_file(raw.as_bytes())
        .with_context(|| format!("Unsigned or invalid {} metadata", label))?;
    Ok(())
}

fn read_local_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read '{}'", path.display()))?;
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::runtime::{
        DomainPattern, Manifest, PathPattern, Permissions, PluginType, TrustTier,
    };

    use super::{
        enforce_remote_registry_policy, materialize_registry_bundle, update_registry_metadata,
        PublishedBundle, RegistryCatalog, RegistryLocation, RegistryPluginIndex, PLUGIN_INDEX_FILE,
        REGISTRY_FILE,
    };

    fn sample_manifest(version: &str) -> Manifest {
        Manifest {
            name: "Echo Skill".to_string(),
            version: version.to_string(),
            sdk_version: "0.1.0".to_string(),
            plugin_type: PluginType::Skill,
            permissions: Permissions {
                filesystem: Vec::<PathPattern>::new(),
                network: Vec::<DomainPattern>::new(),
                memory_read: false,
                memory_write: false,
                tools: Vec::new(),
            },
            trust_tier: TrustTier::Reviewed,
            min_model: None,
            description: "Echo skill".to_string(),
        }
    }

    #[test]
    fn update_registry_metadata_writes_catalog_and_plugin_index() {
        let temp_dir = TempDir::new().expect("temp dir");
        let manifest = sample_manifest("0.2.0");

        update_registry_metadata(
            temp_dir.path(),
            "echo-skill",
            &manifest,
            PublishedBundle {
                version: "0.2.0".to_string(),
                published_at: 123,
                bundle_path: "echo-skill/0.2.0".to_string(),
                manifest_path: "echo-skill/0.2.0/manifest.json".to_string(),
                package_path: "echo-skill/0.2.0/plugin-package.json".to_string(),
                runtime_path: Some("echo-skill/0.2.0/runtime.json".to_string()),
                artifact_path: Some("echo-skill/0.2.0/echo.wasm".to_string()),
                readme_path: Some("echo-skill/0.2.0/README.md".to_string()),
                release_path: "echo-skill/0.2.0/release.json".to_string(),
            },
        )
        .expect("write registry");

        let registry: RegistryCatalog = serde_json::from_str(
            &fs::read_to_string(temp_dir.path().join(REGISTRY_FILE)).expect("registry json"),
        )
        .expect("registry");
        assert_eq!(registry.plugins.len(), 1);
        assert_eq!(registry.plugins[0].latest_version, "0.2.0");
        assert!(!registry.signature.is_empty());
        assert!(registry.signed_at > 0);

        let plugin_index: RegistryPluginIndex = serde_json::from_str(
            &fs::read_to_string(temp_dir.path().join("echo-skill").join(PLUGIN_INDEX_FILE))
                .expect("plugin index"),
        )
        .expect("plugin index json");
        assert_eq!(plugin_index.versions.len(), 1);
        assert_eq!(plugin_index.versions[0].version, "0.2.0");
        assert!(!plugin_index.signature.is_empty());
        assert!(plugin_index.signed_at > 0);
    }

    #[tokio::test]
    async fn materialize_registry_bundle_copies_local_release() {
        let temp_dir = TempDir::new().expect("temp dir");
        let release_dir = temp_dir.path().join("echo-skill").join("0.1.0");
        fs::create_dir_all(&release_dir).expect("release dir");
        fs::write(release_dir.join("manifest.json"), "{}").expect("manifest");
        fs::write(release_dir.join("plugin-package.json"), "{}").expect("package");
        fs::write(release_dir.join("runtime.json"), "{}").expect("runtime");
        fs::write(release_dir.join("echo.wasm"), b"wasm").expect("artifact");

        update_registry_metadata(
            temp_dir.path(),
            "echo-skill",
            &sample_manifest("0.1.0"),
            PublishedBundle {
                version: "0.1.0".to_string(),
                published_at: 123,
                bundle_path: "echo-skill/0.1.0".to_string(),
                manifest_path: "echo-skill/0.1.0/manifest.json".to_string(),
                package_path: "echo-skill/0.1.0/plugin-package.json".to_string(),
                runtime_path: Some("echo-skill/0.1.0/runtime.json".to_string()),
                artifact_path: Some("echo-skill/0.1.0/echo.wasm".to_string()),
                readme_path: None,
                release_path: "echo-skill/0.1.0/release.json".to_string(),
            },
        )
        .expect("write registry");

        let bundle = materialize_registry_bundle(
            temp_dir.path().to_str().expect("registry path"),
            "echo-skill",
            Some("0.1.0"),
        )
        .await
        .expect("materialize bundle");

        assert!(bundle.path().join("manifest.json").exists());
        assert!(bundle.path().join("plugin-package.json").exists());
        assert!(bundle.path().join("runtime.json").exists());
        assert!(bundle.path().join("echo.wasm").exists());
    }

    #[test]
    fn remote_registry_policy_rejects_plain_http_hosts() {
        let error = enforce_remote_registry_policy(&RegistryLocation::Remote(
            "http://example.com/registry".to_string(),
        ))
        .expect_err("plain http should be rejected");

        assert!(error.to_string().contains("must use HTTPS or localhost"));
    }
}
