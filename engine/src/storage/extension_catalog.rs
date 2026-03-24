use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionCatalogEntry {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
    pub trust_badge: String,
    pub latest_version: String,
    pub latest_published_at: i64,
    pub registry_source: String,
    pub index_path: String,
    pub manifest_json: String,
    pub permission_summary: Vec<String>,
    pub permission_warnings: Vec<String>,
    pub release_summary: Option<String>,
    pub fetched_at: i64,
}

pub struct ExtensionCatalogRepository {
    pool: SqlitePool,
}

impl ExtensionCatalogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn replace_all(&self, entries: &[ExtensionCatalogEntry]) -> Result<()> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .context("Failed to start extension catalog transaction")?;

        sqlx::query("DELETE FROM extension_catalog_entries")
            .execute(&mut *transaction)
            .await
            .context("Failed to clear extension catalog cache")?;

        for entry in entries {
            sqlx::query(
                r#"INSERT INTO extension_catalog_entries
                   (id, name, kind, description, trust_badge, latest_version, latest_published_at,
                    registry_source, index_path, manifest_json, permission_summary_json,
                    permission_warnings_json, release_summary, fetched_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&entry.id)
            .bind(&entry.name)
            .bind(&entry.kind)
            .bind(&entry.description)
            .bind(&entry.trust_badge)
            .bind(&entry.latest_version)
            .bind(entry.latest_published_at)
            .bind(&entry.registry_source)
            .bind(&entry.index_path)
            .bind(&entry.manifest_json)
            .bind(
                serde_json::to_string(&entry.permission_summary)
                    .context("Failed to serialize extension catalog permission summary")?,
            )
            .bind(
                serde_json::to_string(&entry.permission_warnings)
                    .context("Failed to serialize extension catalog permission warnings")?,
            )
            .bind(&entry.release_summary)
            .bind(entry.fetched_at)
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("Failed to cache extension catalog entry '{}'", entry.id))?;
        }

        transaction
            .commit()
            .await
            .context("Failed to commit extension catalog transaction")?;

        Ok(())
    }

    pub async fn list_entries(&self) -> Result<Vec<ExtensionCatalogEntry>> {
        let rows = sqlx::query(
            r#"SELECT id, name, kind, description, trust_badge, latest_version, latest_published_at,
                      registry_source, index_path, manifest_json, permission_summary_json,
                      permission_warnings_json, release_summary, fetched_at
               FROM extension_catalog_entries
               ORDER BY name ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list extension catalog cache entries")?;

        rows.into_iter().map(map_entry).collect()
    }

    pub async fn get_entry(&self, id: &str) -> Result<Option<ExtensionCatalogEntry>> {
        let row = sqlx::query(
            r#"SELECT id, name, kind, description, trust_badge, latest_version, latest_published_at,
                      registry_source, index_path, manifest_json, permission_summary_json,
                      permission_warnings_json, release_summary, fetched_at
               FROM extension_catalog_entries
               WHERE id = ?"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .with_context(|| format!("Failed to fetch extension catalog entry '{}'", id))?;

        row.map(map_entry).transpose()
    }

    pub async fn last_fetched_at(&self) -> Result<Option<i64>> {
        sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(fetched_at) FROM extension_catalog_entries")
            .fetch_one(&self.pool)
            .await
            .context("Failed to query extension catalog cache freshness")
    }
}

fn map_entry(row: SqliteRow) -> Result<ExtensionCatalogEntry> {
    Ok(ExtensionCatalogEntry {
        id: row.get("id"),
        name: row.get("name"),
        kind: row.get("kind"),
        description: row.get("description"),
        trust_badge: row.get("trust_badge"),
        latest_version: row.get("latest_version"),
        latest_published_at: row.get("latest_published_at"),
        registry_source: row.get("registry_source"),
        index_path: row.get("index_path"),
        manifest_json: row.get("manifest_json"),
        permission_summary: serde_json::from_str(row.get::<String, _>("permission_summary_json").as_str())
            .context("Failed to parse extension catalog permission summary")?,
        permission_warnings: serde_json::from_str(
            row.get::<String, _>("permission_warnings_json").as_str(),
        )
        .context("Failed to parse extension catalog permission warnings")?,
        release_summary: row.get("release_summary"),
        fetched_at: row.get("fetched_at"),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::storage::Database;

    use super::{ExtensionCatalogEntry, ExtensionCatalogRepository};

    async fn repo() -> (TempDir, ExtensionCatalogRepository) {
        let temp_dir = TempDir::new().expect("temp dir");
        let database = Database::new(&temp_dir.path().join("catalog.db"))
            .await
            .expect("database");
        (temp_dir, database.extension_catalog())
    }

    fn sample_entry(id: &str) -> ExtensionCatalogEntry {
        ExtensionCatalogEntry {
            id: id.to_string(),
            name: "Echo Skill".to_string(),
            kind: "skill".to_string(),
            description: "Test catalog entry".to_string(),
            trust_badge: "verified".to_string(),
            latest_version: "0.2.0".to_string(),
            latest_published_at: 1_710_000_000,
            registry_source: "https://registry.roveai.co/extensions".to_string(),
            index_path: format!("{id}/index.json"),
            manifest_json: "{}".to_string(),
            permission_summary: vec!["filesystem: none".to_string()],
            permission_warnings: vec![],
            release_summary: Some("Test release".to_string()),
            fetched_at: 1_710_000_100,
        }
    }

    #[tokio::test]
    async fn replace_all_overwrites_existing_catalog_entries() {
        let (_temp_dir, repo) = repo().await;
        repo.replace_all(&[sample_entry("echo-skill")])
            .await
            .expect("seed entries");
        repo.replace_all(&[sample_entry("grep-skill")])
            .await
            .expect("replace entries");

        let entries = repo.list_entries().await.expect("list entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "grep-skill");
    }
}
