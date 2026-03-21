use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde_json::json;
use tempfile::TempDir;

use crate::cli::commands::{ExtensionAction, PluginScaffoldType};
use crate::cli::database_path::database_path;
use crate::cli::output::OutputFormat;
use crate::config::Config;
use crate::runtime::PluginType;
use crate::security::crypto::CryptoModule;
use crate::storage::{Database, InstalledPlugin};

pub enum ExtensionSurface {
    Skill,
    System,
    Channel,
}

impl ExtensionSurface {
    fn noun(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::System => "system",
            Self::Channel => "channel",
        }
    }

    fn scaffold_type(&self) -> PluginScaffoldType {
        match self {
            Self::Skill => PluginScaffoldType::Skill,
            Self::System => PluginScaffoldType::System,
            Self::Channel => PluginScaffoldType::Channel,
        }
    }

    fn expected_type(&self) -> PluginType {
        match self {
            Self::Skill => PluginType::Skill,
            Self::System => PluginType::Workspace,
            Self::Channel => PluginType::Channel,
        }
    }
}

pub async fn handle(
    config: &Config,
    surface: ExtensionSurface,
    action: ExtensionAction,
) -> Result<()> {
    if matches!(surface, ExtensionSurface::System) {
        if let Some(result) = try_handle_official_system_action(config, &action).await? {
            return Ok(result);
        }
    }

    match action {
        ExtensionAction::New { name } => {
            crate::cli::plugins::handle_new(&name, surface.scaffold_type()).await
        }
        ExtensionAction::Test {
            source,
            tool,
            input,
            files,
            args,
            no_build,
        } => {
            crate::cli::plugins::handle_test(
                source.as_deref(),
                tool.as_deref(),
                input.as_deref(),
                &files,
                &args,
                no_build,
            )
            .await
        }
        ExtensionAction::Pack {
            source,
            out,
            no_build,
        } => crate::cli::plugins::handle_pack(source.as_deref(), out.as_deref(), no_build).await,
        ExtensionAction::Publish {
            source,
            registry_dir,
            no_build,
        } => crate::cli::plugins::handle_publish(source.as_deref(), &registry_dir, no_build).await,
        ExtensionAction::Install {
            source,
            registry,
            version,
        } => {
            let expected_type = surface.expected_type();
            let installed = crate::cli::plugins::install_checked(
                config,
                &source,
                registry.as_deref(),
                version.as_deref(),
                Some(expected_type),
            )
            .await?;
            println!(
                "Installed {} '{}' [{}] version={}",
                surface.noun(),
                installed.name,
                installed.id,
                installed.version
            );
            Ok(())
        }
        ExtensionAction::Upgrade {
            source,
            registry,
            version,
        } => {
            let expected_type = surface.expected_type();
            let installed = crate::cli::plugins::upgrade_checked(
                config,
                &source,
                registry.as_deref(),
                version.as_deref(),
                Some(expected_type),
            )
            .await?;
            println!(
                "Upgraded {} '{}' [{}] to version {}",
                surface.noun(),
                installed.name,
                installed.id,
                installed.version
            );
            Ok(())
        }
        ExtensionAction::List => {
            crate::cli::plugins::handle_list_filtered(config, OutputFormat::Text, surface.noun())
                .await
        }
        ExtensionAction::Inspect { name } => {
            crate::cli::plugins::handle_inspect_filtered(config, &name, surface.noun()).await
        }
        ExtensionAction::Enable { name } => {
            crate::cli::plugins::handle_set_enabled_filtered(config, &name, true, surface.noun())
                .await
        }
        ExtensionAction::Disable { name } => {
            crate::cli::plugins::handle_set_enabled_filtered(config, &name, false, surface.noun())
                .await
        }
        ExtensionAction::Remove { name } => {
            crate::cli::plugins::handle_remove_filtered(config, &name, surface.noun()).await
        }
    }
}

async fn try_handle_official_system_action(
    config: &Config,
    action: &ExtensionAction,
) -> Result<Option<()>> {
    match action {
        ExtensionAction::List => {
            print_official_systems(config).await?;
            Ok(Some(()))
        }
        ExtensionAction::Inspect { name } if official_system(name).is_some() => {
            inspect_official_system(config, name).await?;
            Ok(Some(()))
        }
        ExtensionAction::Enable { name } if official_system(name).is_some() => {
            enable_official_system(config, name).await?;
            Ok(Some(()))
        }
        ExtensionAction::Disable { name } if official_system(name).is_some() => {
            disable_official_system(config, name).await?;
            Ok(Some(()))
        }
        ExtensionAction::Remove { name } if official_system(name).is_some() => {
            remove_official_system(config, name).await?;
            Ok(Some(()))
        }
        ExtensionAction::Install {
            source,
            registry,
            version,
        } if official_system(source).is_some() && registry.is_none() && version.is_none() => {
            install_official_system(config, source, false).await?;
            Ok(Some(()))
        }
        ExtensionAction::Upgrade {
            source,
            registry,
            version,
        } if official_system(source).is_some() && registry.is_none() && version.is_none() => {
            install_official_system(config, source, true).await?;
            Ok(Some(()))
        }
        _ => Ok(None),
    }
}

async fn print_official_systems(config: &Config) -> Result<()> {
    let database = open_database(config).await?;
    let installed = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins")?;

    println!("Official system extensions:");
    for system in OFFICIAL_SYSTEMS {
        let state = system_state(config, &installed, system.id);
        println!("- {} [{}] {}", system.id, state, system.description);
    }

    let custom = installed
        .iter()
        .filter(|plugin| plugin_public_kind(plugin) == "system" && !is_official_system_id(&plugin.id))
        .cloned()
        .collect::<Vec<_>>();
    if !custom.is_empty() {
        println!();
        println!("Installed custom systems:");
        for plugin in custom {
            println!(
                "- {} [{}] version={}",
                plugin.name,
                if plugin.enabled { "enabled" } else { "disabled" },
                plugin.version
            );
        }
    }

    Ok(())
}

async fn inspect_official_system(config: &Config, name: &str) -> Result<()> {
    let database = open_database(config).await?;
    let installed = database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins")?;
    let system = official_system(name).expect("validated official system");

    println!("name: {}", system.id);
    println!("kind: system");
    println!("source: official");
    println!("state: {}", system_state(config, &installed, system.id));
    println!("description: {}", system.description);

    if let Some(plugin) = installed
        .iter()
        .find(|plugin| plugin.id == system.id || plugin.name.eq_ignore_ascii_case(system.id))
    {
        println!("version: {}", plugin.version);
        if let Some(path) = &plugin.binary_path {
            println!("artifact: {}", path);
        }
    } else {
        println!("install: rove system install {}", system.id);
    }

    Ok(())
}

async fn enable_official_system(config: &Config, name: &str) -> Result<()> {
    let database = open_database(config).await?;
    if let Some(plugin) = resolve_installed_official_system(&database, name).await? {
        database
            .installed_plugins()
            .set_enabled(&plugin.id, true)
            .await
            .context("Failed to enable installed system")?;
        disable_legacy_system_flag(config, name)?;
        println!("Enabled system '{}'.", name);
        return Ok(());
    }

    install_official_system(config, name, false).await
}

async fn disable_official_system(config: &Config, name: &str) -> Result<()> {
    let database = open_database(config).await?;
    if let Some(plugin) = resolve_installed_official_system(&database, name).await? {
        database
            .installed_plugins()
            .set_enabled(&plugin.id, false)
            .await
            .context("Failed to disable installed system")?;
        disable_legacy_system_flag(config, name)?;
        println!("Disabled system '{}'.", name);
        return Ok(());
    }

    set_legacy_system_flag(config, name, false)?;
    println!(
        "Disabled legacy built-in compatibility for '{}'. Install it with `rove system install {}` to use the official extension path.",
        name, name
    );
    Ok(())
}

async fn remove_official_system(config: &Config, name: &str) -> Result<()> {
    let database = open_database(config).await?;
    if let Some(plugin) = resolve_installed_official_system(&database, name).await? {
        database
            .installed_plugins()
            .delete_plugin(&plugin.id)
            .await
            .context("Failed to remove installed system")?;
        let install_dir = installed_system_dir(config, &plugin.id);
        if install_dir.exists() {
            fs::remove_dir_all(&install_dir).with_context(|| {
                format!("Failed to remove installed system directory '{}'", install_dir.display())
            })?;
        }
        disable_legacy_system_flag(config, name)?;
        println!("Removed system '{}'.", name);
        return Ok(());
    }

    set_legacy_system_flag(config, name, false)?;
    println!("Disabled legacy built-in system '{}'.", name);
    Ok(())
}

async fn install_official_system(config: &Config, name: &str, upgrade: bool) -> Result<()> {
    let system = official_system(name).context("Unknown official system")?;
    let package_dir = stage_official_system_package(system)?;

    let installed = if upgrade {
        crate::cli::plugins::upgrade_checked(
            config,
            package_dir.path().to_string_lossy().as_ref(),
            None,
            None,
            Some(PluginType::Workspace),
        )
        .await?
    } else {
        match crate::cli::plugins::install_checked(
            config,
            package_dir.path().to_string_lossy().as_ref(),
            None,
            None,
            Some(PluginType::Workspace),
        )
        .await
        {
            Ok(installed) => installed,
            Err(error) if error.to_string().contains("already installed") => {
                crate::cli::plugins::upgrade_checked(
                    config,
                    package_dir.path().to_string_lossy().as_ref(),
                    None,
                    None,
                    Some(PluginType::Workspace),
                )
                .await?
            }
            Err(error) => return Err(error),
        }
    };

    disable_legacy_system_flag(config, name)?;
    println!(
        "{} official system '{}' [{}] version={}",
        if upgrade { "Upgraded" } else { "Installed" },
        installed.name,
        installed.id,
        installed.version
    );
    Ok(())
}

fn stage_official_system_package(system: &OfficialSystem) -> Result<TempDir> {
    let temp_dir = TempDir::new().context("Failed to create temp package directory")?;
    let artifact_path = build_official_system_artifact(system)?;
    let artifact_name = artifact_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Official system artifact filename is missing")?
        .to_string();
    let artifact_bytes = fs::read(&artifact_path)
        .with_context(|| format!("Failed to read '{}'", artifact_path.display()))?;
    let payload_hash = CryptoModule::compute_hash(&artifact_bytes);

    fs::copy(&artifact_path, temp_dir.path().join(&artifact_name)).with_context(|| {
        format!(
            "Failed to stage '{}' into '{}'",
            artifact_path.display(),
            temp_dir.path().display()
        )
    })?;

    fs::write(
        temp_dir.path().join("manifest.json"),
        serde_json::to_string_pretty(&json!({
            "name": system.id,
            "version": env!("CARGO_PKG_VERSION"),
            "sdk_version": crate::runtime::SDK_VERSION,
            "plugin_type": "Workspace",
            "permissions": system_permissions(system.id),
            "trust_tier": "Official",
            "min_model": null,
            "description": system.description,
            "signature": "LOCAL_DEV_MANIFEST_SIGNATURE"
        }))?,
    )?;
    fs::write(
        temp_dir.path().join("plugin-package.json"),
        serde_json::to_string_pretty(&json!({
            "id": system.id,
            "artifact": artifact_name,
            "runtime_config": "runtime.json",
            "payload_hash": payload_hash,
            "payload_signature": "LOCAL_DEV_PAYLOAD_SIGNATURE",
            "enabled": true
        }))?,
    )?;
    fs::write(
        temp_dir.path().join("runtime.json"),
        serde_json::to_string_pretty(&json!({ "tools": system_tools(system.id) }))?,
    )?;

    Ok(temp_dir)
}

fn build_official_system_artifact(system: &OfficialSystem) -> Result<PathBuf> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("Engine manifest has no workspace parent")?
        .to_path_buf();
    let status = Command::new("cargo")
        .args(["build", "-p", system.crate_name, "--release"])
        .current_dir(&workspace_root)
        .status()
        .with_context(|| {
            format!(
                "Failed to build official system crate '{}' in '{}'",
                system.crate_name,
                workspace_root.display()
            )
        })?;
    if !status.success() {
        bail!("cargo build failed for official system '{}'", system.id);
    }

    let artifact = workspace_root
        .join("target")
        .join("release")
        .join(native_artifact_filename(system.crate_name));
    if !artifact.exists() {
        bail!(
            "Built artifact '{}' was not found for official system '{}'",
            artifact.display(),
            system.id
        );
    }
    Ok(artifact)
}

fn native_artifact_filename(crate_name: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        format!("lib{}.dylib", crate_name)
    }
    #[cfg(target_os = "linux")]
    {
        format!("lib{}.so", crate_name)
    }
    #[cfg(target_os = "windows")]
    {
        format!("{}.dll", crate_name)
    }
}

async fn open_database(config: &Config) -> Result<Database> {
    Database::new(&database_path(config))
        .await
        .context("Failed to open database")
}

async fn resolve_installed_official_system(
    database: &Database,
    name: &str,
) -> Result<Option<InstalledPlugin>> {
    if let Some(plugin) = database
        .installed_plugins()
        .get_plugin(name)
        .await
        .context("Failed to fetch installed system by id")?
    {
        return Ok(Some(plugin));
    }

    database
        .installed_plugins()
        .get_plugin_by_name(name)
        .await
        .context("Failed to fetch installed system by name")
}

fn system_state(config: &Config, installed: &[InstalledPlugin], id: &str) -> &'static str {
    if let Some(plugin) = installed.iter().find(|plugin| plugin.id == id) {
        return if plugin.enabled {
            "installed"
        } else {
            "installed-disabled"
        };
    }

    if legacy_system_flag(config, id) {
        "legacy-enabled"
    } else {
        "available"
    }
}

fn installed_system_dir(config: &Config, id: &str) -> PathBuf {
    config.core.data_dir.join("plugins").join(id)
}

fn disable_legacy_system_flag(config: &Config, name: &str) -> Result<()> {
    set_legacy_system_flag(config, name, false)
}

fn set_legacy_system_flag(config: &Config, name: &str, enabled: bool) -> Result<()> {
    let mut config = config.clone();
    match name {
        "filesystem" => config.plugins.fs_editor = enabled,
        "terminal" => config.plugins.terminal = enabled,
        "vision" => config.plugins.screenshot = enabled,
        _ => bail!("Unknown official system '{}'", name),
    }
    config.save()?;
    Ok(())
}

fn legacy_system_flag(config: &Config, name: &str) -> bool {
    match name {
        "filesystem" => config.plugins.fs_editor,
        "terminal" => config.plugins.terminal,
        "vision" => config.plugins.screenshot,
        _ => false,
    }
}

fn plugin_public_kind(plugin: &InstalledPlugin) -> &'static str {
    match plugin.plugin_type.as_str() {
        "Skill" => "skill",
        "Workspace" => "system",
        "Channel" => "channel",
        "Mcp" => "connector",
        _ => "plugin",
    }
}

fn official_system(name: &str) -> Option<&'static OfficialSystem> {
    OFFICIAL_SYSTEMS
        .iter()
        .find(|system| system.id.eq_ignore_ascii_case(name))
}

fn is_official_system_id(id: &str) -> bool {
    OFFICIAL_SYSTEMS.iter().any(|system| system.id == id)
}

struct OfficialSystem {
    id: &'static str,
    crate_name: &'static str,
    description: &'static str,
}

const OFFICIAL_SYSTEMS: &[OfficialSystem] = &[
    OfficialSystem {
        id: "filesystem",
        crate_name: "filesystem",
        description: "Workspace file read/write/list primitives.",
    },
    OfficialSystem {
        id: "terminal",
        crate_name: "terminal",
        description: "Secure local terminal command execution.",
    },
    OfficialSystem {
        id: "vision",
        crate_name: "screenshot",
        description: "Local screenshot capture primitives.",
    },
];

fn system_permissions(id: &str) -> serde_json::Value {
    match id {
        "filesystem" | "terminal" | "vision" => json!({
            "filesystem": ["workspace/**"],
            "network": [],
            "memory_read": false,
            "memory_write": false,
            "tools": []
        }),
        _ => json!({
            "filesystem": [],
            "network": [],
            "memory_read": false,
            "memory_write": false,
            "tools": []
        }),
    }
}

fn system_tools(id: &str) -> Vec<serde_json::Value> {
    match id {
        "filesystem" => vec![
            json!({"name":"read_file","description":"Read the contents of a file.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]},"domains":["filesystem","read","all"]}),
            json!({"name":"write_file","description":"Write content to a file.","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]},"domains":["filesystem","write","all"]}),
            json!({"name":"delete_file","description":"Delete a file.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]},"domains":["filesystem","write","all"]}),
            json!({"name":"list_dir","description":"List files in a directory.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]},"domains":["filesystem","read","all"]}),
            json!({"name":"file_exists","description":"Check whether a path exists.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]},"domains":["filesystem","read","all"]}),
        ],
        "terminal" => vec![json!({"name":"run_command","description":"Execute an allowed terminal command.","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]},"domains":["shell","git","code","all"]})],
        "vision" => vec![json!({"name":"capture_screen","description":"Capture a screenshot.","parameters":{"type":"object","properties":{"output_file":{"type":"string"}}},"domains":["vision","all"]})],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{official_system, system_state};
    use crate::config::Config;
    use crate::storage::InstalledPlugin;

    fn installed_plugin(id: &str, enabled: bool) -> InstalledPlugin {
        InstalledPlugin {
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Workspace".to_string(),
            trust_tier: 0,
            manifest: "{}".to_string(),
            binary_path: Some(format!("{id}.dylib")),
            binary_hash: "abc123".to_string(),
            signature: "LOCAL_DEV_PAYLOAD_SIGNATURE".to_string(),
            enabled,
            installed_at: 1,
            last_used: None,
            config: Some(r#"{"tools":[]}"#.to_string()),
        }
    }

    #[test]
    fn official_system_lookup_is_case_insensitive() {
        let system = official_system("FiLeSyStEm").expect("official system");
        assert_eq!(system.id, "filesystem");
    }

    #[test]
    fn system_state_prefers_installed_record_over_legacy_flag() {
        let mut config = Config::default();
        config.plugins.terminal = true;

        let installed = vec![installed_plugin("terminal", false)];
        assert_eq!(
            system_state(&config, &installed, "terminal"),
            "installed-disabled"
        );

        let enabled = vec![installed_plugin("terminal", true)];
        assert_eq!(system_state(&config, &enabled, "terminal"), "installed");
    }
}
