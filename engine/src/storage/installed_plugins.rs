use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub plugin_type: String,
    pub trust_tier: i64,
    pub manifest: String,
    pub binary_path: Option<String>,
    pub binary_hash: String,
    pub signature: String,
    pub enabled: bool,
    pub installed_at: i64,
    pub last_used: Option<i64>,
    pub config: Option<String>,
    pub provenance_source: Option<String>,
    pub provenance_registry: Option<String>,
    pub catalog_trust_badge: Option<String>,
}

pub struct InstalledPluginRepository {
    pool: SqlitePool,
}

impl InstalledPluginRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn upsert_plugin(&self, plugin: &InstalledPlugin) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO installed_plugins
               (id, name, version, plugin_type, trust_tier, manifest, binary_path, binary_hash, signature, enabled, installed_at, last_used, config, provenance_source, provenance_registry, catalog_trust_badge)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 version = excluded.version,
                 plugin_type = excluded.plugin_type,
                 trust_tier = excluded.trust_tier,
                 manifest = excluded.manifest,
                 binary_path = excluded.binary_path,
                 binary_hash = excluded.binary_hash,
                 signature = excluded.signature,
                 enabled = excluded.enabled,
                 installed_at = excluded.installed_at,
                 last_used = excluded.last_used,
                 config = excluded.config,
                 provenance_source = excluded.provenance_source,
                 provenance_registry = excluded.provenance_registry,
                 catalog_trust_badge = excluded.catalog_trust_badge"#,
        )
        .bind(&plugin.id)
        .bind(&plugin.name)
        .bind(&plugin.version)
        .bind(&plugin.plugin_type)
        .bind(plugin.trust_tier)
        .bind(&plugin.manifest)
        .bind(&plugin.binary_path)
        .bind(&plugin.binary_hash)
        .bind(&plugin.signature)
        .bind(if plugin.enabled { 1_i64 } else { 0_i64 })
        .bind(plugin.installed_at)
        .bind(plugin.last_used)
        .bind(&plugin.config)
        .bind(&plugin.provenance_source)
        .bind(&plugin.provenance_registry)
        .bind(&plugin.catalog_trust_badge)
        .execute(&self.pool)
        .await
        .context("Failed to upsert installed plugin")?;

        Ok(())
    }

    pub async fn get_plugin(&self, id: &str) -> Result<Option<InstalledPlugin>> {
        let row = sqlx::query(
            r#"SELECT id, name, version, plugin_type, trust_tier, manifest, binary_path, binary_hash,
                      signature, enabled, installed_at, last_used, config,
                      provenance_source, provenance_registry, catalog_trust_badge
               FROM installed_plugins
               WHERE id = ?"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch installed plugin")?;

        Ok(row.map(map_installed_plugin))
    }

    pub async fn get_plugin_by_name(&self, name: &str) -> Result<Option<InstalledPlugin>> {
        let row = sqlx::query(
            r#"SELECT id, name, version, plugin_type, trust_tier, manifest, binary_path, binary_hash,
                      signature, enabled, installed_at, last_used, config,
                      provenance_source, provenance_registry, catalog_trust_badge
               FROM installed_plugins
               WHERE name = ?"#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch installed plugin by name")?;

        Ok(row.map(map_installed_plugin))
    }

    pub async fn list_plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let rows = sqlx::query(
            r#"SELECT id, name, version, plugin_type, trust_tier, manifest, binary_path, binary_hash,
                      signature, enabled, installed_at, last_used, config,
                      provenance_source, provenance_registry, catalog_trust_badge
               FROM installed_plugins
               ORDER BY name ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list installed plugins")?;

        Ok(rows.into_iter().map(map_installed_plugin).collect())
    }

    pub async fn get_enabled_plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let rows = sqlx::query(
            r#"SELECT id, name, version, plugin_type, trust_tier, manifest, binary_path, binary_hash,
                      signature, enabled, installed_at, last_used, config,
                      provenance_source, provenance_registry, catalog_trust_badge
               FROM installed_plugins
               WHERE enabled = 1
               ORDER BY name ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list enabled installed plugins")?;

        Ok(rows.into_iter().map(map_installed_plugin).collect())
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        sqlx::query("UPDATE installed_plugins SET enabled = ? WHERE id = ?")
            .bind(if enabled { 1_i64 } else { 0_i64 })
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to set installed plugin enabled state")?;

        Ok(())
    }

    pub async fn touch_last_used(&self, id: &str) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("System clock before UNIX_EPOCH")?
            .as_secs() as i64;

        sqlx::query("UPDATE installed_plugins SET last_used = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update installed plugin last_used")?;

        Ok(())
    }

    pub async fn delete_plugin(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM installed_plugins WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete installed plugin")?;

        Ok(())
    }
}

fn map_installed_plugin(row: SqliteRow) -> InstalledPlugin {
    InstalledPlugin {
        id: row.get("id"),
        name: row.get("name"),
        version: row.get("version"),
        plugin_type: row.get("plugin_type"),
        trust_tier: row.get("trust_tier"),
        manifest: row.get("manifest"),
        binary_path: row.get("binary_path"),
        binary_hash: row.get("binary_hash"),
        signature: row.get("signature"),
        enabled: row.get::<i64, _>("enabled") != 0,
        installed_at: row.get("installed_at"),
        last_used: row.get("last_used"),
        config: row.get("config"),
        provenance_source: row.get("provenance_source"),
        provenance_registry: row.get("provenance_registry"),
        catalog_trust_badge: row.get("catalog_trust_badge"),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::storage::Database;

    use super::{InstalledPlugin, InstalledPluginRepository};

    fn sample_plugin(id: &str) -> InstalledPlugin {
        InstalledPlugin {
            id: id.to_string(),
            name: "echo-plugin".to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Plugin".to_string(),
            trust_tier: 1,
            manifest: r#"{"name":"echo-plugin","version":"0.1.0","sdk_version":"0.1.0","plugin_type":"Plugin","permissions":{"filesystem":[],"network":[],"memory_read":false,"memory_write":false,"tools":[]},"trust_tier":"Reviewed","min_model":null,"description":"Echo plugin"}"#.to_string(),
            binary_path: Some("/tmp/echo.wasm".to_string()),
            binary_hash: "abc123".to_string(),
            signature: "deadbeef".to_string(),
            enabled: true,
            installed_at: 1_710_000_000,
            last_used: None,
            config: Some(r#"{"entry":"default"}"#.to_string()),
            provenance_source: Some("public_catalog".to_string()),
            provenance_registry: Some("https://registry.roveai.co/extensions".to_string()),
            catalog_trust_badge: Some("verified".to_string()),
        }
    }

    async fn repo() -> (TempDir, InstalledPluginRepository) {
        let temp_dir = TempDir::new().expect("temp dir");
        let database = Database::new(&temp_dir.path().join("installed.db"))
            .await
            .expect("database");
        (temp_dir, database.installed_plugins())
    }

    #[tokio::test]
    async fn upsert_and_get_enabled_plugins() {
        let (_temp_dir, repo) = repo().await;
        let plugin = sample_plugin("plugin-1");

        repo.upsert_plugin(&plugin).await.expect("upsert plugin");

        let enabled = repo.get_enabled_plugins().await.expect("enabled plugins");
        assert_eq!(enabled, vec![plugin]);
    }

    #[tokio::test]
    async fn set_enabled_filters_results() {
        let (_temp_dir, repo) = repo().await;
        let plugin = sample_plugin("plugin-2");

        repo.upsert_plugin(&plugin).await.expect("upsert plugin");
        repo.set_enabled("plugin-2", false)
            .await
            .expect("disable plugin");

        let enabled = repo.get_enabled_plugins().await.expect("enabled plugins");
        assert!(enabled.is_empty());

        let disabled = repo.get_plugin("plugin-2").await.expect("fetch plugin");
        assert!(!disabled.expect("stored plugin").enabled);
    }

    #[tokio::test]
    async fn touch_last_used_updates_timestamp() {
        let (_temp_dir, repo) = repo().await;
        let plugin = sample_plugin("plugin-3");

        repo.upsert_plugin(&plugin).await.expect("upsert plugin");
        repo.touch_last_used("plugin-3")
            .await
            .expect("touch last used");

        let stored = repo
            .get_plugin_by_name("echo-plugin")
            .await
            .expect("fetch plugin by name")
            .expect("stored plugin");
        assert!(stored.last_used.is_some());
    }
}
