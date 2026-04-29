use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::runtime::PluginType;
use crate::security::crypto::CryptoModule;
use crate::storage::{Database, InstalledPlugin};

use super::inventory::open_database;
use super::package::{
    default_plugin_id, default_runtime_file, load_package, load_runtime_config,
    manifest_from_signed_json, read_required_file, resolve_package_root, MANIFEST_FILE,
};
use super::registry::materialize_registry_bundle;
use super::stage::{
    install_directory, legacy_install_directory, perform_install, verify_and_store,
};
use super::validate::{print_permission_review, resolve_payload_source, validate_plugin_shape};

pub async fn handle_install(
    config: &Config,
    source: &str,
    registry: Option<&str>,
    version: Option<&str>,
) -> Result<()> {
    let installed = install_checked(config, source, registry, version, None).await?;

    println!(
        "Installed plugin '{}' [{}] type={} version={}",
        installed.name, installed.id, installed.plugin_type, installed.version
    );
    if let Some(path) = &installed.binary_path {
        println!("artifact: {}", path);
    }
    println!("Enable with: rove plugin enable {}", installed.name);

    Ok(())
}

pub async fn handle_upgrade(
    config: &Config,
    source: &str,
    registry: Option<&str>,
    version: Option<&str>,
) -> Result<()> {
    let installed = upgrade_checked(config, source, registry, version, None).await?;

    println!(
        "Upgraded plugin '{}' [{}] to version {}",
        installed.name, installed.id, installed.version
    );

    Ok(())
}

pub(crate) async fn install_checked(
    config: &Config,
    source: &str,
    registry: Option<&str>,
    version: Option<&str>,
    expected_type: Option<PluginType>,
) -> Result<InstalledPlugin> {
    let database = open_database(config).await?;
    let crypto = CryptoModule::new().context("Failed to initialize plugin verifier")?;
    if let Some(registry) = registry {
        return install_from_registry(
            config,
            &database,
            &crypto,
            registry,
            source,
            version,
            expected_type.as_ref(),
        )
        .await;
    }

    validate_expected_type(Path::new(source), &crypto, expected_type.as_ref())?;
    install_from_directory(config, &database, &crypto, Path::new(source)).await
}

pub(crate) async fn upgrade_checked(
    config: &Config,
    source: &str,
    registry: Option<&str>,
    version: Option<&str>,
    expected_type: Option<PluginType>,
) -> Result<InstalledPlugin> {
    let database = open_database(config).await?;
    let crypto = CryptoModule::new().context("Failed to initialize plugin verifier")?;
    if let Some(registry) = registry {
        return upgrade_from_registry(
            config,
            &database,
            &crypto,
            registry,
            source,
            version,
            expected_type.as_ref(),
        )
        .await;
    }

    validate_expected_type(Path::new(source), &crypto, expected_type.as_ref())?;
    upgrade_from_directory(config, &database, &crypto, Path::new(source)).await
}

pub(super) async fn install_from_directory(
    config: &Config,
    database: &Database,
    crypto: &CryptoModule,
    source: &Path,
) -> Result<crate::storage::InstalledPlugin> {
    install_with_mode(config, database, crypto, source, false).await
}

async fn upgrade_from_directory(
    config: &Config,
    database: &Database,
    crypto: &CryptoModule,
    source: &Path,
) -> Result<crate::storage::InstalledPlugin> {
    install_with_mode(config, database, crypto, source, true).await
}

async fn install_from_registry(
    config: &Config,
    database: &Database,
    crypto: &CryptoModule,
    registry: &str,
    plugin_id: &str,
    version: Option<&str>,
    expected_type: Option<&PluginType>,
) -> Result<crate::storage::InstalledPlugin> {
    let bundle = materialize_registry_bundle(registry, plugin_id, version).await?;
    validate_expected_type(bundle.path(), crypto, expected_type)?;
    install_from_directory(config, database, crypto, bundle.path()).await
}

async fn upgrade_from_registry(
    config: &Config,
    database: &Database,
    crypto: &CryptoModule,
    registry: &str,
    plugin_id: &str,
    version: Option<&str>,
    expected_type: Option<&PluginType>,
) -> Result<crate::storage::InstalledPlugin> {
    let bundle = materialize_registry_bundle(registry, plugin_id, version).await?;
    validate_expected_type(bundle.path(), crypto, expected_type)?;
    upgrade_from_directory(config, database, crypto, bundle.path()).await
}

async fn install_with_mode(
    config: &Config,
    database: &Database,
    crypto: &CryptoModule,
    source: &Path,
    replace_existing: bool,
) -> Result<crate::storage::InstalledPlugin> {
    let package_root = resolve_package_root(source)?;
    let manifest_raw = read_required_file(&package_root.join(MANIFEST_FILE))?;
    crypto
        .verify_manifest_file(manifest_raw.as_bytes())
        .context("Manifest signature verification failed")?;
    let manifest = manifest_from_signed_json(&manifest_raw)?;

    let package = load_package(&package_root)?;
    let runtime_rel = package
        .runtime_config
        .clone()
        .or_else(|| default_runtime_file(&package_root));
    let runtime_raw = load_runtime_config(&package_root, runtime_rel.as_deref())?;
    validate_plugin_shape(&manifest, runtime_raw.as_deref())?;
    print_permission_review(&manifest);

    let install_id = package
        .id
        .clone()
        .unwrap_or_else(|| default_plugin_id(&manifest.name));

    if replace_existing {
        remove_existing_plugin_artifacts(config, database, &install_id, &manifest.name).await?;
    } else {
        ensure_not_installed(database, &install_id, &manifest.name).await?;
    }

    let payload_source =
        resolve_payload_source(&package_root, &manifest, &package, runtime_rel.as_deref())?;
    let install_dir = install_directory(config, &install_id, &manifest.plugin_type);
    if install_dir.exists() {
        bail!(
            "Install directory '{}' already exists. Remove the plugin first.",
            install_dir.display()
        );
    }

    fs::create_dir_all(&install_dir).with_context(|| {
        format!(
            "Failed to create install directory '{}'",
            install_dir.display()
        )
    })?;

    let install_result = match perform_install(
        &install_dir,
        &manifest_raw,
        runtime_raw.as_deref(),
        payload_source.as_deref(),
        &package,
        &manifest,
        &install_id,
    ) {
        Ok(prepared) => verify_and_store(database, crypto, prepared).await,
        Err(error) => Err(error),
    };

    if install_result.is_err() {
        let _ = fs::remove_dir_all(&install_dir);
    }

    install_result
}

async fn ensure_not_installed(
    database: &Database,
    install_id: &str,
    manifest_name: &str,
) -> Result<()> {
    let existing_id = database
        .installed_plugins()
        .get_plugin(install_id)
        .await
        .context("Failed to check installed plugin ids")?;
    if existing_id.is_some() {
        bail!(
            "Plugin id '{}' is already installed. Remove it before reinstalling.",
            install_id
        );
    }

    let existing_name = database
        .installed_plugins()
        .get_plugin_by_name(manifest_name)
        .await
        .context("Failed to check installed plugin names")?;
    if existing_name.is_some() {
        bail!(
            "Plugin '{}' is already installed. Remove it before reinstalling.",
            manifest_name
        );
    }

    Ok(())
}

fn validate_expected_type(
    source: &Path,
    crypto: &CryptoModule,
    expected_type: Option<&PluginType>,
) -> Result<()> {
    let Some(expected_type) = expected_type else {
        return Ok(());
    };

    let package_root = resolve_package_root(source)?;
    let manifest_raw = read_required_file(&package_root.join(MANIFEST_FILE))?;
    crypto
        .verify_manifest_file(manifest_raw.as_bytes())
        .context("Manifest signature verification failed")?;
    let manifest = manifest_from_signed_json(&manifest_raw)?;

    if &manifest.plugin_type != expected_type {
        bail!(
            "Plugin '{}' is type '{}' but this command expects '{}'",
            manifest.name,
            manifest.plugin_type.as_str(),
            expected_type.as_str()
        );
    }

    Ok(())
}

async fn remove_existing_plugin_artifacts(
    config: &Config,
    database: &Database,
    install_id: &str,
    manifest_name: &str,
) -> Result<()> {
    let existing = database
        .installed_plugins()
        .get_plugin(install_id)
        .await
        .context("Failed to check installed plugin ids")?
        .or(database
            .installed_plugins()
            .get_plugin_by_name(manifest_name)
            .await
            .context("Failed to check installed plugin names")?);

    let Some(existing) = existing else {
        return Ok(());
    };

    database
        .installed_plugins()
        .delete_plugin(&existing.id)
        .await
        .context("Failed to remove existing plugin before upgrade")?;

    let plugin_type = PluginType::parse(&existing.plugin_type).unwrap_or(PluginType::Plugin);
    let install_dir = install_directory(config, &existing.id, &plugin_type);
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).with_context(|| {
            format!(
                "Failed to remove previous install directory '{}'",
                install_dir.display()
            )
        })?;
    } else if false {
        // Legacy Workspace fallback path removed (Workspace merged into Plugin)
        let legacy_dir = legacy_install_directory(config, &existing.id);
        if legacy_dir.exists() {
            fs::remove_dir_all(&legacy_dir).with_context(|| {
                format!(
                    "Failed to remove legacy driver install directory '{}'",
                    legacy_dir.display()
                )
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::TempDir;

    use crate::config::Config;
    use crate::runtime::PluginType;
    use crate::security::crypto::CryptoModule;
    use crate::storage::Database;

    use super::{
        install_from_directory, install_from_registry, upgrade_from_directory,
        validate_expected_type,
    };
    use crate::cli::plugins::handle_publish;
    use crate::cli::plugins::package::{
        default_plugin_id, MANIFEST_FILE, PACKAGE_FILE, RUNTIME_FILE,
    };

    fn write_signed_manifest(
        dir: &Path,
        signing_key: &SigningKey,
        name: &str,
        plugin_type: &str,
        trust_tier: &str,
        sdk_version: &str,
    ) {
        let unsigned = serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "sdk_version": sdk_version,
            "plugin_type": plugin_type,
            "permissions": {
                "filesystem": [],
                "network": [],
                "memory_read": false,
                "memory_write": false,
                "tools": []
            },
            "trust_tier": trust_tier,
            "min_model": null,
            "description": format!("{} plugin", name),
        });
        let mut signed = unsigned.clone();
        let canonical = CryptoModule::canonicalize_manifest(
            serde_json::to_string(&unsigned)
                .expect("manifest json")
                .as_bytes(),
        )
        .expect("canonical manifest");
        let signature = signing_key.sign(&canonical);
        signed["signature"] = serde_json::Value::String(hex::encode(signature.to_bytes()));

        fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::to_string_pretty(&signed).expect("signed manifest"),
        )
        .expect("write manifest");
    }

    fn write_package(dir: &Path, payload_file: &str, payload_hash: &str, payload_signature: &str) {
        let package = serde_json::json!({
            "artifact": payload_file,
            "runtime_config": RUNTIME_FILE,
            "payload_hash": payload_hash,
            "payload_signature": payload_signature,
            "enabled": true
        });
        fs::write(
            dir.join(PACKAGE_FILE),
            serde_json::to_string_pretty(&package).expect("package json"),
        )
        .expect("write package");
    }

    async fn test_database() -> (TempDir, Database) {
        let temp_dir = TempDir::new().expect("temp dir");
        let database = Database::new(&temp_dir.path().join("plugins.db"))
            .await
            .expect("database");
        (temp_dir, database)
    }

    #[tokio::test]
    async fn installs_signed_wasm_plugin_package() {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let crypto = CryptoModule::with_key(signing_key.verifying_key());
        let package_dir = TempDir::new().expect("package dir");
        let (_db_dir, database) = test_database().await;
        let data_dir = TempDir::new().expect("data dir");
        let workspace = TempDir::new().expect("workspace dir");

        let artifact_bytes = b"fake wasm bytes";
        fs::write(package_dir.path().join("echo.wasm"), artifact_bytes).expect("write wasm");
        fs::write(
            package_dir.path().join(RUNTIME_FILE),
            r#"{"tools":[{"name":"echo_text","description":"Echo text","parameters":{"type":"object","properties":{"text":{"type":"string"}}}}]}"#,
        )
        .expect("write runtime config");

        let payload_hash = CryptoModule::compute_hash(artifact_bytes);
        let payload_signature = hex::encode(signing_key.sign(payload_hash.as_bytes()).to_bytes());
        write_signed_manifest(
            package_dir.path(),
            &signing_key,
            "Echo Plugin",
            "Plugin",
            "Reviewed",
            "0.1.0",
        );
        write_package(
            package_dir.path(),
            "echo.wasm",
            &payload_hash,
            &payload_signature,
        );

        let mut config = Config::default();
        config.core.data_dir = data_dir.path().to_path_buf();
        config.core.workspace = workspace.path().to_path_buf();

        let installed = install_from_directory(&config, &database, &crypto, package_dir.path())
            .await
            .expect("install plugin");

        assert_eq!(installed.name, "Echo Plugin");
        assert_eq!(installed.id, default_plugin_id("Echo Plugin"));
        assert_eq!(installed.plugin_type, "Plugin");
        assert!(installed
            .binary_path
            .as_ref()
            .is_some_and(|path| path.ends_with("echo.wasm")));
        assert!(data_dir
            .path()
            .join("plugins")
            .join(default_plugin_id("Echo Plugin"))
            .join("echo.wasm")
            .exists());
    }

    #[tokio::test]
    async fn rejects_sdk_version_mismatch_during_install() {
        let signing_key = SigningKey::from_bytes(&[9u8; 32]);
        let crypto = CryptoModule::with_key(signing_key.verifying_key());
        let package_dir = TempDir::new().expect("package dir");
        let (_db_dir, database) = test_database().await;
        let data_dir = TempDir::new().expect("data dir");
        let workspace = TempDir::new().expect("workspace dir");

        let artifact_bytes = b"fake wasm bytes";
        fs::write(package_dir.path().join("bad.wasm"), artifact_bytes).expect("write wasm");
        fs::write(package_dir.path().join(RUNTIME_FILE), r#"{"tools":[]}"#)
            .expect("write runtime config");

        let payload_hash = CryptoModule::compute_hash(artifact_bytes);
        let payload_signature = hex::encode(signing_key.sign(payload_hash.as_bytes()).to_bytes());
        write_signed_manifest(
            package_dir.path(),
            &signing_key,
            "Bad Plugin",
            "Plugin",
            "Reviewed",
            "9.9.9",
        );
        write_package(
            package_dir.path(),
            "bad.wasm",
            &payload_hash,
            &payload_signature,
        );

        let mut config = Config::default();
        config.core.data_dir = data_dir.path().to_path_buf();
        config.core.workspace = workspace.path().to_path_buf();

        let error = install_from_directory(&config, &database, &crypto, package_dir.path())
            .await
            .expect_err("sdk mismatch should fail");

        assert!(error.to_string().contains("targets SDK version"));
    }

    #[tokio::test]
    async fn upgrade_replaces_existing_plugin_record() {
        let signing_key = SigningKey::from_bytes(&[11u8; 32]);
        let crypto = CryptoModule::with_key(signing_key.verifying_key());
        let first_dir = TempDir::new().expect("first package dir");
        let second_dir = TempDir::new().expect("second package dir");
        let (_db_dir, database) = test_database().await;
        let data_dir = TempDir::new().expect("data dir");
        let workspace = TempDir::new().expect("workspace dir");

        let mut config = Config::default();
        config.core.data_dir = data_dir.path().to_path_buf();
        config.core.workspace = workspace.path().to_path_buf();

        fs::write(first_dir.path().join("echo.wasm"), b"first wasm").expect("write first wasm");
        fs::write(first_dir.path().join(RUNTIME_FILE), r#"{"tools":[]}"#)
            .expect("write first runtime");
        let first_hash = CryptoModule::compute_hash(b"first wasm");
        let first_sig = hex::encode(signing_key.sign(first_hash.as_bytes()).to_bytes());
        write_signed_manifest(
            first_dir.path(),
            &signing_key,
            "Echo Plugin",
            "Plugin",
            "Reviewed",
            "0.1.0",
        );
        write_package(first_dir.path(), "echo.wasm", &first_hash, &first_sig);
        install_from_directory(&config, &database, &crypto, first_dir.path())
            .await
            .expect("install first package");

        fs::write(second_dir.path().join("echo.wasm"), b"second wasm").expect("write second wasm");
        fs::write(second_dir.path().join(RUNTIME_FILE), r#"{"tools":[]}"#)
            .expect("write second runtime");
        let second_hash = CryptoModule::compute_hash(b"second wasm");
        let second_sig = hex::encode(signing_key.sign(second_hash.as_bytes()).to_bytes());
        write_signed_manifest(
            second_dir.path(),
            &signing_key,
            "Echo Plugin",
            "Plugin",
            "Reviewed",
            "0.1.0",
        );
        write_package(second_dir.path(), "echo.wasm", &second_hash, &second_sig);

        let upgraded = upgrade_from_directory(&config, &database, &crypto, second_dir.path())
            .await
            .expect("upgrade package");

        let stored = database
            .installed_plugins()
            .get_plugin(&upgraded.id)
            .await
            .expect("fetch upgraded plugin")
            .expect("stored upgraded plugin");

        assert_eq!(stored.binary_hash, second_hash);
    }

    #[tokio::test]
    async fn install_checked_loads_plugin_from_local_registry() {
        let signing_key = SigningKey::from_bytes(&[21u8; 32]);
        let package_dir = TempDir::new().expect("package dir");
        let (_db_dir, database) = test_database().await;
        let data_dir = TempDir::new().expect("data dir");
        let workspace = TempDir::new().expect("workspace dir");
        let registry_dir = TempDir::new().expect("registry dir");
        let crypto = CryptoModule::with_key(signing_key.verifying_key());

        let artifact_bytes = b"registry wasm bytes";
        fs::write(package_dir.path().join("echo.wasm"), artifact_bytes).expect("write wasm");
        fs::write(package_dir.path().join(RUNTIME_FILE), r#"{"tools":[]}"#)
            .expect("write runtime config");

        let payload_hash = CryptoModule::compute_hash(artifact_bytes);
        let payload_signature = hex::encode(signing_key.sign(payload_hash.as_bytes()).to_bytes());
        write_signed_manifest(
            package_dir.path(),
            &signing_key,
            "Echo Plugin",
            "Plugin",
            "Reviewed",
            "0.1.0",
        );
        write_package(
            package_dir.path(),
            "echo.wasm",
            &payload_hash,
            &payload_signature,
        );

        handle_publish(
            Some(package_dir.path().to_str().expect("package path")),
            registry_dir.path(),
            true,
        )
        .await
        .expect("publish registry bundle");

        let mut config = Config::default();
        config.core.data_dir = data_dir.path().to_path_buf();
        config.core.workspace = workspace.path().to_path_buf();

        let installed = install_from_registry(
            &config,
            &database,
            &crypto,
            registry_dir.path().to_str().expect("registry path"),
            "echo-plugin",
            Some("0.1.0"),
            None,
        )
        .await
        .expect("install from registry");

        assert_eq!(installed.id, "echo-plugin");
        assert_eq!(installed.version, "0.1.0");
        assert!(data_dir
            .path()
            .join("plugins")
            .join("echo-plugin")
            .join("echo.wasm")
            .exists());
    }

    #[tokio::test]
    async fn installs_workspace_package_under_drivers_directory() {
        let signing_key = SigningKey::from_bytes(&[31u8; 32]);
        let crypto = CryptoModule::with_key(signing_key.verifying_key());
        let package_dir = TempDir::new().expect("package dir");
        let (_db_dir, database) = test_database().await;
        let data_dir = TempDir::new().expect("data dir");
        let workspace = TempDir::new().expect("workspace dir");

        let artifact_bytes = b"native driver bytes";
        fs::write(package_dir.path().join("vision-plus.dylib"), artifact_bytes)
            .expect("write driver");
        fs::write(package_dir.path().join(RUNTIME_FILE), r#"{"tools":[]}"#)
            .expect("write runtime config");

        let payload_hash = CryptoModule::compute_hash(artifact_bytes);
        let payload_signature = hex::encode(signing_key.sign(payload_hash.as_bytes()).to_bytes());
        write_signed_manifest(
            package_dir.path(),
            &signing_key,
            "Vision Plus",
            "Brain",
            "Official",
            "0.1.0",
        );
        write_package(
            package_dir.path(),
            "vision-plus.dylib",
            &payload_hash,
            &payload_signature,
        );

        let mut config = Config::default();
        config.core.data_dir = data_dir.path().to_path_buf();
        config.core.workspace = workspace.path().to_path_buf();

        let installed = install_from_directory(&config, &database, &crypto, package_dir.path())
            .await
            .expect("install driver");

        assert_eq!(installed.plugin_type, "Brain");
        assert!(data_dir
            .path()
            .join("plugins")
            .join(default_plugin_id("Vision Plus"))
            .join("vision-plus.dylib")
            .exists());
        assert!(data_dir
            .path()
            .join("plugins")
            .join(default_plugin_id("Vision Plus"))
            .join(PACKAGE_FILE)
            .exists());
    }

    #[test]
    fn validate_expected_type_rejects_mismatched_package() {
        let signing_key = SigningKey::from_bytes(&[13u8; 32]);
        let package_dir = TempDir::new().expect("package dir");

        write_signed_manifest(
            package_dir.path(),
            &signing_key,
            "Echo Plugin",
            "Plugin",
            "Reviewed",
            "0.1.0",
        );
        fs::write(
            package_dir.path().join(PACKAGE_FILE),
            r#"{
                "artifact": "echo.wasm",
                "runtime_config": "runtime.json",
                "payload_hash": "abc123",
                "payload_signature": "deadbeef"
            }"#,
        )
        .expect("write package");
        fs::write(package_dir.path().join(RUNTIME_FILE), r#"{"tools":[]}"#).expect("write runtime");
        fs::write(package_dir.path().join("echo.wasm"), b"fake wasm bytes").expect("artifact");

        let crypto = CryptoModule::with_key(signing_key.verifying_key());
        let error = validate_expected_type(package_dir.path(), &crypto, Some(&PluginType::Mcp))
            .expect_err("type mismatch should fail");

        assert!(error.to_string().contains("this command expects 'Mcp'"));
    }
}
