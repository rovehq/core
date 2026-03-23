use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::cli::database_path::database_path;
use crate::cli::output::OutputFormat;
use crate::config::Config;
use crate::runtime::Manifest;
use crate::storage::{Database, InstalledPlugin};

pub async fn handle_list(config: &Config, format: OutputFormat) -> Result<()> {
    let database = open_database(config).await?;
    let plugins = list_installed_plugins(&database).await?;

    match format {
        OutputFormat::Text => print_plugin_list(&plugins),
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "plugins": plugins }))?
            );
        }
    }

    Ok(())
}

pub async fn handle_inspect(config: &Config, selector: &str) -> Result<()> {
    let database = open_database(config).await?;
    let plugin = resolve_installed_plugin(&database, selector).await?;
    print_plugin_details(&plugin);
    Ok(())
}

pub async fn handle_list_filtered(config: &Config, format: OutputFormat, kind: &str) -> Result<()> {
    let database = open_database(config).await?;
    let plugins = filter_installed_plugins(&list_installed_plugins(&database).await?, kind);

    match format {
        OutputFormat::Text => print_filtered_list(&plugins, kind),
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ kind: plugins }))?
            );
        }
    }

    Ok(())
}

pub async fn handle_inspect_filtered(config: &Config, selector: &str, kind: &str) -> Result<()> {
    let database = open_database(config).await?;
    let plugin = resolve_filtered_plugin(&database, selector, kind).await?;
    print_plugin_details(&plugin);
    Ok(())
}

pub async fn handle_set_enabled(config: &Config, selector: &str, enabled: bool) -> Result<()> {
    let database = open_database(config).await?;
    let plugin = set_installed_plugin_enabled(&database, selector, enabled).await?;
    let state = if enabled { "enabled" } else { "disabled" };
    println!("Plugin '{}' {}.", plugin.name, state);
    Ok(())
}

pub async fn handle_set_enabled_filtered(
    config: &Config,
    selector: &str,
    enabled: bool,
    kind: &str,
) -> Result<()> {
    let database = open_database(config).await?;
    let plugin = resolve_filtered_plugin(&database, selector, kind).await?;
    database
        .installed_plugins()
        .set_enabled(&plugin.id, enabled)
        .await
        .context("Failed to update installed plugin state")?;
    let state = if enabled { "enabled" } else { "disabled" };
    println!("{} '{}' {}.", kind, plugin.name, state);
    Ok(())
}

pub async fn handle_remove(config: &Config, selector: &str) -> Result<()> {
    let database = open_database(config).await?;
    let plugin = remove_installed_plugin(&database, selector).await?;
    println!("Removed plugin '{}'.", plugin.name);
    Ok(())
}

pub async fn handle_remove_filtered(config: &Config, selector: &str, kind: &str) -> Result<()> {
    let database = open_database(config).await?;
    let plugin = resolve_filtered_plugin(&database, selector, kind).await?;
    database
        .installed_plugins()
        .delete_plugin(&plugin.id)
        .await
        .context("Failed to remove installed plugin")?;
    println!("Removed {} '{}'.", kind, plugin.name);
    Ok(())
}

pub(super) async fn open_database(config: &Config) -> Result<Database> {
    Database::new(&database_path(config))
        .await
        .context("Failed to open database")
}

pub(super) async fn list_installed_plugins(database: &Database) -> Result<Vec<InstalledPlugin>> {
    database
        .installed_plugins()
        .list_plugins()
        .await
        .context("Failed to list installed plugins")
}

pub(super) async fn resolve_filtered_plugin(
    database: &Database,
    selector: &str,
    kind: &str,
) -> Result<InstalledPlugin> {
    let plugin = resolve_installed_plugin(database, selector).await?;
    if plugin_public_kind(&plugin) == kind {
        return Ok(plugin);
    }

    bail!(
        "'{}' is installed as a {} but this command expects a {}",
        plugin.name,
        plugin_public_kind(&plugin),
        kind
    )
}

pub(crate) async fn resolve_installed_plugin(
    database: &Database,
    selector: &str,
) -> Result<InstalledPlugin> {
    if let Some(plugin) = database
        .installed_plugins()
        .get_plugin(selector)
        .await
        .context("Failed to fetch installed plugin by id")?
    {
        return Ok(plugin);
    }

    if let Some(plugin) = database
        .installed_plugins()
        .get_plugin_by_name(selector)
        .await
        .context("Failed to fetch installed plugin by name")?
    {
        return Ok(plugin);
    }

    bail!("Plugin '{}' is not installed", selector)
}

async fn set_installed_plugin_enabled(
    database: &Database,
    selector: &str,
    enabled: bool,
) -> Result<InstalledPlugin> {
    let plugin = resolve_installed_plugin(database, selector).await?;
    database
        .installed_plugins()
        .set_enabled(&plugin.id, enabled)
        .await
        .context("Failed to update installed plugin state")?;

    resolve_installed_plugin(database, &plugin.id).await
}

async fn remove_installed_plugin(database: &Database, selector: &str) -> Result<InstalledPlugin> {
    let plugin = resolve_installed_plugin(database, selector).await?;
    database
        .installed_plugins()
        .delete_plugin(&plugin.id)
        .await
        .context("Failed to remove installed plugin")?;
    Ok(plugin)
}

fn print_plugin_list(plugins: &[InstalledPlugin]) {
    if plugins.is_empty() {
        println!("No installed plugins.");
        return;
    }

    println!("Installed plugins:");
    for plugin in plugins {
        let state = if plugin.enabled {
            "enabled"
        } else {
            "disabled"
        };
        println!(
            "- {} [{}] type={} version={} tier={}",
            plugin.name, state, plugin.plugin_type, plugin.version, plugin.trust_tier
        );
    }
}

fn print_filtered_list(plugins: &[InstalledPlugin], kind: &str) {
    if plugins.is_empty() {
        println!("No installed {}s.", kind);
        return;
    }

    println!("Installed {}s:", kind);
    for plugin in plugins {
        let state = if plugin.enabled {
            "enabled"
        } else {
            "disabled"
        };
        println!(
            "- {} [{}] version={} tier={}",
            plugin.name, state, plugin.version, plugin.trust_tier
        );
    }
}

fn filter_installed_plugins(plugins: &[InstalledPlugin], kind: &str) -> Vec<InstalledPlugin> {
    plugins
        .iter()
        .filter(|plugin| plugin_public_kind(plugin) == kind)
        .cloned()
        .collect()
}

fn plugin_public_kind(plugin: &InstalledPlugin) -> &'static str {
    match plugin.plugin_type.as_str() {
        "Skill" => "skill",
        "Workspace" => "system",
        "Channel" => "channel",
        "Mcp" => "connector",
        "Brain" => "brain",
        _ => "extension",
    }
}

fn print_plugin_details(plugin: &InstalledPlugin) {
    let manifest = Manifest::from_json(&plugin.manifest).ok();

    println!("id: {}", plugin.id);
    println!("name: {}", plugin.name);
    println!("version: {}", plugin.version);
    println!("type: {}", plugin.plugin_type);
    println!("enabled: {}", plugin.enabled);
    println!("trust_tier: {}", plugin.trust_tier);
    println!("installed_at: {}", plugin.installed_at);
    if let Some(last_used) = plugin.last_used {
        println!("last_used: {}", last_used);
    } else {
        println!("last_used: never");
    }

    if let Some(path) = &plugin.binary_path {
        println!("binary_path: {}", path);
    } else {
        println!("binary_path: (none)");
    }
    println!("binary_hash: {}", plugin.binary_hash);
    println!("signature: {}", plugin.signature);

    if let Some(config) = &plugin.config {
        println!("runtime_config: {}", config);
    } else {
        println!("runtime_config: (none)");
    }

    if let Some(manifest) = manifest {
        println!("sdk_version: {}", manifest.sdk_version);
        println!("description: {}", manifest.description);
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::storage::InstalledPlugin;

    use super::{
        list_installed_plugins, remove_installed_plugin, resolve_installed_plugin,
        set_installed_plugin_enabled, Database,
    };

    fn sample_plugin(id: &str, name: &str) -> InstalledPlugin {
        InstalledPlugin {
            id: id.to_string(),
            name: name.to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Skill".to_string(),
            trust_tier: 1,
            manifest: format!(
                r#"{{"name":"{name}","version":"0.1.0","sdk_version":"0.1.0","plugin_type":"Skill","permissions":{{"filesystem":[],"network":[],"memory_read":false,"memory_write":false,"tools":[]}},"trust_tier":"Reviewed","min_model":null,"description":"{name} plugin"}}"#
            ),
            binary_path: Some(format!("/tmp/{}.wasm", name)),
            binary_hash: "abc123".to_string(),
            signature: "deadbeef".to_string(),
            enabled: true,
            installed_at: 1_710_000_000,
            last_used: None,
            config: Some(r#"{"tools":[]}"#.to_string()),
        }
    }

    async fn database() -> (TempDir, Database) {
        let temp_dir = TempDir::new().expect("temp dir");
        let database = Database::new(&temp_dir.path().join("plugins.db"))
            .await
            .expect("database");
        (temp_dir, database)
    }

    #[tokio::test]
    async fn resolves_installed_plugin_by_name_and_id() {
        let (_temp_dir, database) = database().await;
        let plugin = sample_plugin("plugin-1", "echo");

        database
            .installed_plugins()
            .upsert_plugin(&plugin)
            .await
            .expect("insert plugin");

        let by_id = resolve_installed_plugin(&database, "plugin-1")
            .await
            .expect("resolve by id");
        let by_name = resolve_installed_plugin(&database, "echo")
            .await
            .expect("resolve by name");

        assert_eq!(by_id.id, plugin.id);
        assert_eq!(by_name.name, plugin.name);
    }

    #[tokio::test]
    async fn enable_toggle_updates_installed_plugin_row() {
        let (_temp_dir, database) = database().await;
        let plugin = sample_plugin("plugin-2", "toggle");

        database
            .installed_plugins()
            .upsert_plugin(&plugin)
            .await
            .expect("insert plugin");

        let disabled = set_installed_plugin_enabled(&database, "toggle", false)
            .await
            .expect("disable plugin");
        assert!(!disabled.enabled);

        let listed = list_installed_plugins(&database)
            .await
            .expect("list plugins");
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].enabled);
    }

    #[tokio::test]
    async fn remove_deletes_installed_plugin_row() {
        let (_temp_dir, database) = database().await;
        let plugin = sample_plugin("plugin-3", "remove-me");

        database
            .installed_plugins()
            .upsert_plugin(&plugin)
            .await
            .expect("insert plugin");

        let removed = remove_installed_plugin(&database, "remove-me")
            .await
            .expect("remove plugin");
        assert_eq!(removed.id, "plugin-3");

        let listed = list_installed_plugins(&database)
            .await
            .expect("list plugins");
        assert!(listed.is_empty());
    }
}
