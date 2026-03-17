/// Plugin management operations
///
/// This module provides functions for managing plugin metadata in the database.
/// All queries use parameterized queries for SQL injection prevention.
///
/// Requirements: 12.2, 12.6, 12.7, 12.10
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};

/// Plugin record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub wasm_path: String,
    pub wasm_hash: String,
    pub manifest_json: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Plugin repository for database operations
pub struct PluginRepository {
    pool: SqlitePool,
}

impl PluginRepository {
    /// Create a new plugin repository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Register a new plugin
    ///
    /// Requirements: 12.6, 12.10
    pub async fn register_plugin(
        &self,
        id: &str,
        name: &str,
        version: &str,
        wasm_path: &str,
        wasm_hash: &str,
        manifest_json: &str,
    ) -> Result<Plugin> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let enabled = true;

        // Use parameterized query to prevent SQL injection
        sqlx::query(
            "INSERT INTO plugins (id, name, version, wasm_path, wasm_hash, manifest_json, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(id)
        .bind(name)
        .bind(version)
        .bind(wasm_path)
        .bind(wasm_hash)
        .bind(manifest_json)
        .bind(enabled)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to register plugin")?;

        Ok(Plugin {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            wasm_path: wasm_path.to_string(),
            wasm_hash: wasm_hash.to_string(),
            manifest_json: manifest_json.to_string(),
            enabled,
            created_at: now,
            updated_at: now,
        })
    }

    /// Update plugin metadata
    ///
    /// Requirements: 12.6, 12.10
    pub async fn update_plugin(
        &self,
        id: &str,
        version: &str,
        wasm_path: &str,
        wasm_hash: &str,
        manifest_json: &str,
    ) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        sqlx::query(
            "UPDATE plugins SET version = ?, wasm_path = ?, wasm_hash = ?, manifest_json = ?, updated_at = ? WHERE id = ?"
        )
        .bind(version)
        .bind(wasm_path)
        .bind(wasm_hash)
        .bind(manifest_json)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to update plugin")?;

        Ok(())
    }

    /// Enable or disable a plugin
    ///
    /// Requirements: 12.6, 12.10
    pub async fn set_plugin_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        sqlx::query("UPDATE plugins SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(enabled)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to set plugin enabled status")?;

        Ok(())
    }

    /// Get a plugin by ID
    ///
    /// Requirements: 12.6, 12.10
    pub async fn get_plugin(&self, id: &str) -> Result<Option<Plugin>> {
        let row = sqlx::query(
            "SELECT id, name, version, wasm_path, wasm_hash, manifest_json, enabled, created_at, updated_at FROM plugins WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch plugin")?;

        Ok(row.map(|r| Plugin {
            id: r.get("id"),
            name: r.get("name"),
            version: r.get("version"),
            wasm_path: r.get("wasm_path"),
            wasm_hash: r.get("wasm_hash"),
            manifest_json: r.get("manifest_json"),
            enabled: r.get("enabled"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }))
    }

    /// Get a plugin by name
    ///
    /// Requirements: 12.6, 12.10
    pub async fn get_plugin_by_name(&self, name: &str) -> Result<Option<Plugin>> {
        let row = sqlx::query(
            "SELECT id, name, version, wasm_path, wasm_hash, manifest_json, enabled, created_at, updated_at FROM plugins WHERE name = ?"
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch plugin by name")?;

        Ok(row.map(|r| Plugin {
            id: r.get("id"),
            name: r.get("name"),
            version: r.get("version"),
            wasm_path: r.get("wasm_path"),
            wasm_hash: r.get("wasm_hash"),
            manifest_json: r.get("manifest_json"),
            enabled: r.get("enabled"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }))
    }

    /// Get all plugins
    ///
    /// Requirements: 12.6, 12.10
    pub async fn get_all_plugins(&self) -> Result<Vec<Plugin>> {
        let rows = sqlx::query(
            "SELECT id, name, version, wasm_path, wasm_hash, manifest_json, enabled, created_at, updated_at FROM plugins ORDER BY name ASC"
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch all plugins")?;

        Ok(rows
            .into_iter()
            .map(|r| Plugin {
                id: r.get("id"),
                name: r.get("name"),
                version: r.get("version"),
                wasm_path: r.get("wasm_path"),
                wasm_hash: r.get("wasm_hash"),
                manifest_json: r.get("manifest_json"),
                enabled: r.get("enabled"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    /// Get all enabled plugins
    ///
    /// Requirements: 12.6, 12.10
    pub async fn get_enabled_plugins(&self) -> Result<Vec<Plugin>> {
        let enabled = true;

        let rows = sqlx::query(
            "SELECT id, name, version, wasm_path, wasm_hash, manifest_json, enabled, created_at, updated_at FROM plugins WHERE enabled = ? ORDER BY name ASC"
        )
        .bind(enabled)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch enabled plugins")?;

        Ok(rows
            .into_iter()
            .map(|r| Plugin {
                id: r.get("id"),
                name: r.get("name"),
                version: r.get("version"),
                wasm_path: r.get("wasm_path"),
                wasm_hash: r.get("wasm_hash"),
                manifest_json: r.get("manifest_json"),
                enabled: r.get("enabled"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    /// Delete a plugin
    ///
    /// Requirements: 12.6, 12.10
    pub async fn delete_plugin(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM plugins WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete plugin")?;

        Ok(())
    }

    /// Check if a plugin exists by name
    ///
    /// Requirements: 12.6, 12.10
    pub async fn plugin_exists(&self, name: &str) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plugins WHERE name = ?")
            .bind(name)
            .fetch_one(&self.pool)
            .await
            .context("Failed to check plugin existence")?;

        Ok(count > 0)
    }
}
