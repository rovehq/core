use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::runtime::{Manifest, PluginType};

use super::package::{
    default_plugin_id, default_runtime_file, load_package, load_runtime_config,
    manifest_from_signed_json, read_required_file, resolve_package_root, MANIFEST_FILE,
    PACKAGE_FILE, RUNTIME_FILE,
};
use super::registry::{sign_registry_json, update_registry_metadata, PublishedBundle};
use super::test::{ensure_wasm_target_installed, run_cargo};
use super::validate::{resolve_payload_source, review_manifest_permissions, validate_plugin_shape};

#[derive(Debug, Serialize)]
struct ReleaseManifest {
    id: String,
    name: String,
    version: String,
    plugin_type: String,
    trust_tier: String,
    generated_at: i64,
    signed_at: i64,
    signature: String,
    bundled_from: String,
    artifact: Option<String>,
    runtime_config: Option<String>,
    permission_review: super::validate::PermissionReview,
}

pub(super) struct BundleOutput {
    bundle_dir: PathBuf,
    plugin_id: String,
    manifest: Manifest,
}

pub(crate) struct PublishedBundleOutput {
    pub destination: PathBuf,
    pub plugin_id: String,
    pub version: String,
}

pub async fn handle_pack(source: Option<&str>, out: Option<&Path>, no_build: bool) -> Result<()> {
    let bundle = prepare_distribution_bundle(source, out, no_build).await?;

    println!("Packed plugin bundle:");
    println!("id: {}", bundle.plugin_id);
    println!("name: {}", bundle.manifest.name);
    println!("version: {}", bundle.manifest.version);
    println!("bundle: {}", bundle.bundle_dir.display());
    println!(
        "Install with: rove plugin install {}",
        bundle.bundle_dir.display()
    );

    Ok(())
}

pub async fn handle_publish(
    source: Option<&str>,
    registry_dir: &Path,
    no_build: bool,
) -> Result<()> {
    let bundle = prepare_distribution_bundle(source, None, no_build).await?;
    let published = publish_prepared_bundle(bundle, registry_dir)?;

    println!("Published plugin bundle:");
    println!("id: {}", published.plugin_id);
    println!("version: {}", published.version);
    println!("registry path: {}", published.destination.display());
    println!(
        "Install with: rove plugin install {} --registry {} --version {}",
        published.plugin_id,
        registry_dir.display(),
        published.version
    );

    Ok(())
}

pub(crate) async fn publish_source_to_registry(
    source: &Path,
    registry_dir: &Path,
    no_build: bool,
) -> Result<PublishedBundleOutput> {
    let source_owned = source.to_string_lossy().to_string();
    let bundle = prepare_distribution_bundle(Some(source_owned.as_str()), None, no_build).await?;
    publish_prepared_bundle(bundle, registry_dir)
}

pub(super) async fn prepare_distribution_bundle(
    source: Option<&str>,
    out: Option<&Path>,
    no_build: bool,
) -> Result<BundleOutput> {
    let source_path = match source {
        Some(source) => PathBuf::from(source),
        None => std::env::current_dir().context("Failed to resolve current directory")?,
    };
    let package_root = resolve_package_root(&source_path)?;
    let manifest_raw = read_required_file(&package_root.join(MANIFEST_FILE))?;
    let manifest = manifest_from_signed_json(&manifest_raw)?;
    let package = load_package(&package_root)?;
    let runtime_rel = package
        .runtime_config
        .clone()
        .or_else(|| default_runtime_file(&package_root));
    let runtime_raw = load_runtime_config(&package_root, runtime_rel.as_deref())?;
    validate_plugin_shape(&manifest, runtime_raw.as_deref())?;

    if !no_build
        && matches!(
            manifest.plugin_type,
            PluginType::Skill | PluginType::Channel
        )
    {
        run_cargo(&package_root, &["test"])?;
        ensure_wasm_target_installed()?;
        run_cargo(
            &package_root,
            &["build", "--target", "wasm32-wasip1", "--release"],
        )?;
    }

    let plugin_id = package
        .id
        .clone()
        .unwrap_or_else(|| default_plugin_id(&manifest.name));
    let bundle_dir = match out {
        Some(out) => out.to_path_buf(),
        None => package_root
            .join("dist")
            .join(format!("{}-{}", plugin_id, manifest.version)),
    };

    if bundle_dir.exists() {
        fs::remove_dir_all(&bundle_dir).with_context(|| {
            format!(
                "Failed to clear existing bundle directory '{}'",
                bundle_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("Failed to create '{}'", bundle_dir.display()))?;

    let mut normalized_package = serde_json::from_str::<serde_json::Value>(&read_required_file(
        &package_root.join(PACKAGE_FILE),
    )?)
    .context("Invalid plugin-package.json")?;
    normalized_package["id"] = serde_json::Value::String(plugin_id.clone());
    normalized_package["runtime_config"] = serde_json::Value::String(RUNTIME_FILE.to_string());

    fs::write(bundle_dir.join(MANIFEST_FILE), manifest_raw).with_context(|| {
        format!(
            "Failed to write '{}'",
            bundle_dir.join(MANIFEST_FILE).display()
        )
    })?;

    let artifact_name = if let Some(payload_source) =
        resolve_payload_source(&package_root, &manifest, &package, runtime_rel.as_deref())?
    {
        let file_name = payload_source
            .file_name()
            .context("Plugin payload file name is missing")?
            .to_string_lossy()
            .to_string();
        fs::copy(&payload_source, bundle_dir.join(&file_name)).with_context(|| {
            format!(
                "Failed to copy '{}' into bundle '{}'",
                payload_source.display(),
                bundle_dir.display()
            )
        })?;
        if matches!(manifest.plugin_type, PluginType::Mcp) {
            None
        } else {
            Some(file_name)
        }
    } else {
        None
    };

    if let Some(runtime_raw) = runtime_raw.as_deref() {
        fs::write(bundle_dir.join(RUNTIME_FILE), runtime_raw).with_context(|| {
            format!(
                "Failed to write '{}'",
                bundle_dir.join(RUNTIME_FILE).display()
            )
        })?;
    }

    if let Some(artifact_name) = artifact_name {
        normalized_package["artifact"] = serde_json::Value::String(artifact_name);
    }

    fs::write(
        bundle_dir.join(PACKAGE_FILE),
        serde_json::to_string_pretty(&normalized_package)?,
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            bundle_dir.join(PACKAGE_FILE).display()
        )
    })?;

    let readme = package_root.join("README.md");
    if readme.exists() {
        fs::copy(&readme, bundle_dir.join("README.md")).with_context(|| {
            format!(
                "Failed to copy '{}' into bundle '{}'",
                readme.display(),
                bundle_dir.display()
            )
        })?;
    }

    let release_manifest = ReleaseManifest {
        id: plugin_id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        plugin_type: manifest.plugin_type.as_str().to_string(),
        trust_tier: format!("{:?}", manifest.trust_tier),
        generated_at: unix_now()?,
        signed_at: 0,
        signature: String::new(),
        bundled_from: package_root.display().to_string(),
        artifact: normalized_package
            .get("artifact")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        runtime_config: normalized_package
            .get("runtime_config")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        permission_review: review_manifest_permissions(&manifest),
    };
    let mut release_json = serde_json::to_value(&release_manifest)?;
    sign_registry_json(&mut release_json, release_manifest.generated_at)?;
    fs::write(
        bundle_dir.join("release.json"),
        serde_json::to_string_pretty(&release_json)?,
    )
    .with_context(|| {
        format!(
            "Failed to write '{}'",
            bundle_dir.join("release.json").display()
        )
    })?;

    Ok(BundleOutput {
        bundle_dir,
        plugin_id,
        manifest,
    })
}

fn publish_prepared_bundle(
    bundle: BundleOutput,
    registry_dir: &Path,
) -> Result<PublishedBundleOutput> {
    let destination = registry_dir
        .join(&bundle.plugin_id)
        .join(&bundle.manifest.version);

    if destination.exists() {
        bail!(
            "Registry destination '{}' already exists",
            destination.display()
        );
    }

    fs::create_dir_all(
        destination
            .parent()
            .context("Registry destination has no parent")?,
    )
    .with_context(|| format!("Failed to create '{}'", registry_dir.display()))?;
    copy_tree(&bundle.bundle_dir, &destination)?;
    let published_at = unix_now()?;
    let bundle_rel = PathBuf::from(&bundle.plugin_id).join(&bundle.manifest.version);
    let runtime_path = destination
        .join(RUNTIME_FILE)
        .exists()
        .then(|| bundle_rel.join(RUNTIME_FILE).display().to_string());
    let artifact_path =
        release_artifact_name(&destination).map(|name| bundle_rel.join(name).display().to_string());
    let readme_path = destination
        .join("README.md")
        .exists()
        .then(|| bundle_rel.join("README.md").display().to_string());
    update_registry_metadata(
        registry_dir,
        &bundle.plugin_id,
        &bundle.manifest,
        PublishedBundle {
            version: bundle.manifest.version.clone(),
            published_at,
            bundle_path: bundle_rel.display().to_string(),
            manifest_path: bundle_rel.join(MANIFEST_FILE).display().to_string(),
            package_path: bundle_rel.join(PACKAGE_FILE).display().to_string(),
            runtime_path,
            artifact_path,
            readme_path,
            release_path: bundle_rel.join("release.json").display().to_string(),
        },
    )?;

    Ok(PublishedBundleOutput {
        destination,
        plugin_id: bundle.plugin_id,
        version: bundle.manifest.version,
    })
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("Failed to create '{}'", destination.display()))?;

    for entry in
        fs::read_dir(source).with_context(|| format!("Failed to read '{}'", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "Failed to copy '{}' to '{}'",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn unix_now() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock before UNIX_EPOCH")?
        .as_secs() as i64)
}

fn release_artifact_name(bundle_dir: &Path) -> Option<String> {
    let package_json = fs::read_to_string(bundle_dir.join(PACKAGE_FILE)).ok()?;
    let package = serde_json::from_str::<serde_json::Value>(&package_json).ok()?;
    package
        .get("artifact")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;

    use serde_json::Value;
    use tempfile::TempDir;

    use super::{copy_tree, prepare_distribution_bundle, publish_source_to_registry};

    fn write_sample_package(root: &Path) {
        fs::create_dir_all(root).expect("package root");
        fs::write(
            root.join("manifest.json"),
            serde_json::json!({
                "name": "Sample Skill",
                "version": "0.1.0",
                "sdk_version": "0.1.0",
                "plugin_type": "Skill",
                "permissions": {
                    "filesystem": [],
                    "network": [],
                    "memory_read": false,
                    "memory_write": false,
                    "tools": []
                },
                "trust_tier": "Community",
                "min_model": null,
                "description": "Sample plugin",
                "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
            })
            .to_string(),
        )
        .expect("manifest");
        fs::write(
            root.join("plugin-package.json"),
            serde_json::json!({
                "id": "sample-skill",
                "artifact": "sample.wasm",
                "runtime_config": "runtime.json",
                "payload_hash": "LOCAL_DEV_PAYLOAD_HASH",
                "payload_signature": "LOCAL_DEV_PAYLOAD_SIGNATURE",
                "enabled": true
            })
            .to_string(),
        )
        .expect("package");
        fs::write(
            root.join("runtime.json"),
            serde_json::json!({
                "tools": [
                    {
                        "name": "run",
                        "description": "Run the plugin",
                        "parameters": { "type": "object" },
                        "domains": ["all"]
                    }
                ]
            })
            .to_string(),
        )
        .expect("runtime");
        fs::write(root.join("sample.wasm"), b"dummy-wasm").expect("artifact");
        fs::write(root.join("README.md"), "# Sample\n").expect("readme");
    }

    #[tokio::test]
    async fn pack_creates_normalized_bundle_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let package_dir = temp_dir.path().join("sample");
        write_sample_package(&package_dir);

        let bundle =
            prepare_distribution_bundle(Some(package_dir.to_str().expect("path")), None, true)
                .await
                .expect("bundle");

        assert!(bundle.bundle_dir.join("manifest.json").exists());
        assert!(bundle.bundle_dir.join("plugin-package.json").exists());
        assert!(bundle.bundle_dir.join("runtime.json").exists());
        assert!(bundle.bundle_dir.join("sample.wasm").exists());
        assert!(bundle.bundle_dir.join("release.json").exists());

        let package_json: Value = serde_json::from_str(
            &fs::read_to_string(bundle.bundle_dir.join("plugin-package.json"))
                .expect("read package"),
        )
        .expect("package json");
        assert_eq!(package_json["artifact"], "sample.wasm");
        let release_json: Value = serde_json::from_str(
            &fs::read_to_string(bundle.bundle_dir.join("release.json")).expect("read release"),
        )
        .expect("release json");
        assert!(release_json["signature"].is_string());
        assert!(release_json["signed_at"].is_number());
    }

    #[tokio::test]
    async fn fixture_example_package_packs() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins/echo-skill");
        let temp_dir = TempDir::new().expect("temp dir");
        let bundle_dir = temp_dir.path().join("bundle");

        let bundle = prepare_distribution_bundle(
            Some(fixture.to_str().expect("fixture path")),
            Some(&bundle_dir),
            true,
        )
        .await
        .expect("bundle fixture package");

        assert!(bundle.bundle_dir.join("manifest.json").exists());
        assert!(bundle.bundle_dir.join("plugin-package.json").exists());
        assert!(bundle.bundle_dir.join("runtime.json").exists());
        assert!(bundle.bundle_dir.join("echo_skill.wasm").exists());
    }

    #[tokio::test]
    async fn fixture_channel_package_packs() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins/echo-channel");
        let temp_dir = TempDir::new().expect("temp dir");
        let bundle_dir = temp_dir.path().join("bundle");

        let bundle = prepare_distribution_bundle(
            Some(fixture.to_str().expect("fixture path")),
            Some(&bundle_dir),
            true,
        )
        .await
        .expect("bundle channel package");

        assert!(bundle.bundle_dir.join("manifest.json").exists());
        assert!(bundle.bundle_dir.join("plugin-package.json").exists());
        assert!(bundle.bundle_dir.join("runtime.json").exists());
        assert!(bundle.bundle_dir.join("echo_channel.wasm").exists());
    }

    #[tokio::test]
    async fn fixture_mcp_package_packs() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins/github-mcp");
        let temp_dir = TempDir::new().expect("temp dir");
        let bundle_dir = temp_dir.path().join("bundle");

        let bundle = prepare_distribution_bundle(
            Some(fixture.to_str().expect("fixture path")),
            Some(&bundle_dir),
            true,
        )
        .await
        .expect("bundle mcp package");

        assert!(bundle.bundle_dir.join("manifest.json").exists());
        assert!(bundle.bundle_dir.join("plugin-package.json").exists());
        assert!(bundle.bundle_dir.join("runtime.json").exists());
        assert!(!bundle.bundle_dir.join("github-mcp").exists());
    }

    #[tokio::test]
    async fn publish_source_to_registry_writes_catalog_metadata() {
        let temp_dir = TempDir::new().expect("temp dir");
        let package_dir = temp_dir.path().join("sample");
        let registry_dir = temp_dir.path().join("registry");
        write_sample_package(&package_dir);

        let published = publish_source_to_registry(&package_dir, &registry_dir, true)
            .await
            .expect("publish source");

        assert!(published.destination.exists());
        assert!(registry_dir.join("registry.json").exists());
        assert!(registry_dir
            .join("sample-skill")
            .join("index.json")
            .exists());
    }

    #[test]
    fn publish_copy_tree_copies_nested_bundle() {
        let temp_dir = TempDir::new().expect("temp dir");
        let source = temp_dir.path().join("source");
        let destination = temp_dir.path().join("destination");
        fs::create_dir_all(source.join("nested")).expect("nested source");
        fs::write(source.join("root.txt"), "root").expect("root");
        fs::write(source.join("nested").join("child.txt"), "child").expect("child");

        copy_tree(&source, &destination).expect("copy tree");

        assert_eq!(
            fs::read_to_string(destination.join("root.txt")).expect("read root"),
            "root"
        );
        assert_eq!(
            fs::read_to_string(destination.join("nested").join("child.txt")).expect("read child"),
            "child"
        );
    }
}
