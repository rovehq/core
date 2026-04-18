use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryEntityKind {
    Episodic,
    Fact,
    Insight,
}

impl MemoryEntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Episodic => "episodic",
            Self::Fact => "fact",
            Self::Insight => "insight",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryMutationAction {
    Create,
    Update,
    Delete,
    Redact,
}

impl MemoryMutationAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Redact => "redact",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryVersionRecord {
    pub id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub version_num: i64,
    pub action: String,
    pub content_hash: String,
    pub snapshot_json: String,
    pub actor: String,
    pub source_task_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAuditRecord {
    pub id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub action: String,
    pub actor: String,
    pub source_task_id: Option<String>,
    pub precondition_hash: Option<String>,
    pub content_hash: Option<String>,
    pub metadata_json: Option<String>,
    pub created_at: i64,
}

pub struct MemoryAuditRepository {
    pool: SqlitePool,
}

impl MemoryAuditRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn record_version(
        &self,
        entity_kind: MemoryEntityKind,
        entity_id: &str,
        action: MemoryMutationAction,
        content_hash: &str,
        snapshot_json: &str,
        actor: &str,
        source_task_id: Option<&str>,
    ) -> Result<MemoryVersionRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().timestamp();
        let entity_kind_str = entity_kind.as_str();
        let action_str = action.as_str();
        let version_num: i64 = sqlx::query_scalar(
            r#"SELECT COALESCE(MAX(version_num), 0) + 1
               FROM memory_versions
               WHERE entity_kind = ? AND entity_id = ?"#,
        )
        .bind(entity_kind_str)
        .bind(entity_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to calculate next memory version")?;

        sqlx::query(
            r#"INSERT INTO memory_versions
               (id, entity_kind, entity_id, version_num, action, content_hash, snapshot_json, actor, source_task_id, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&id)
        .bind(entity_kind_str)
        .bind(entity_id)
        .bind(version_num)
        .bind(action_str)
        .bind(content_hash)
        .bind(snapshot_json)
        .bind(actor)
        .bind(source_task_id)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .context("Failed to insert memory version record")?;

        Ok(MemoryVersionRecord {
            id,
            entity_kind: entity_kind_str.to_string(),
            entity_id: entity_id.to_string(),
            version_num,
            action: action_str.to_string(),
            content_hash: content_hash.to_string(),
            snapshot_json: snapshot_json.to_string(),
            actor: actor.to_string(),
            source_task_id: source_task_id.map(ToOwned::to_owned),
            created_at,
        })
    }

    pub async fn record_audit(
        &self,
        entity_kind: MemoryEntityKind,
        entity_id: &str,
        action: MemoryMutationAction,
        actor: &str,
        source_task_id: Option<&str>,
        precondition_hash: Option<&str>,
        content_hash: Option<&str>,
        metadata_json: Option<&str>,
    ) -> Result<MemoryAuditRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().timestamp();
        let entity_kind_str = entity_kind.as_str();
        let action_str = action.as_str();

        sqlx::query(
            r#"INSERT INTO memory_audit_log
               (id, entity_kind, entity_id, action, actor, source_task_id, precondition_hash, content_hash, metadata_json, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&id)
        .bind(entity_kind_str)
        .bind(entity_id)
        .bind(action_str)
        .bind(actor)
        .bind(source_task_id)
        .bind(precondition_hash)
        .bind(content_hash)
        .bind(metadata_json)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .context("Failed to insert memory audit record")?;

        Ok(MemoryAuditRecord {
            id,
            entity_kind: entity_kind_str.to_string(),
            entity_id: entity_id.to_string(),
            action: action_str.to_string(),
            actor: actor.to_string(),
            source_task_id: source_task_id.map(ToOwned::to_owned),
            precondition_hash: precondition_hash.map(ToOwned::to_owned),
            content_hash: content_hash.map(ToOwned::to_owned),
            metadata_json: metadata_json.map(ToOwned::to_owned),
            created_at,
        })
    }

    pub async fn list_versions(
        &self,
        entity_kind: MemoryEntityKind,
        entity_id: &str,
    ) -> Result<Vec<MemoryVersionRecord>> {
        let rows = sqlx::query(
            r#"SELECT id, entity_kind, entity_id, version_num, action, content_hash, snapshot_json, actor, source_task_id, created_at
               FROM memory_versions
               WHERE entity_kind = ? AND entity_id = ?
               ORDER BY version_num DESC"#,
        )
        .bind(entity_kind.as_str())
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list memory versions")?;

        Ok(rows
            .into_iter()
            .map(|row| MemoryVersionRecord {
                id: row.get("id"),
                entity_kind: row.get("entity_kind"),
                entity_id: row.get("entity_id"),
                version_num: row.get("version_num"),
                action: row.get("action"),
                content_hash: row.get("content_hash"),
                snapshot_json: row.get("snapshot_json"),
                actor: row.get("actor"),
                source_task_id: row.get("source_task_id"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    pub async fn list_audit(
        &self,
        entity_kind: MemoryEntityKind,
        entity_id: &str,
    ) -> Result<Vec<MemoryAuditRecord>> {
        let rows = sqlx::query(
            r#"SELECT id, entity_kind, entity_id, action, actor, source_task_id, precondition_hash, content_hash, metadata_json, created_at
               FROM memory_audit_log
               WHERE entity_kind = ? AND entity_id = ?
               ORDER BY created_at DESC"#,
        )
        .bind(entity_kind.as_str())
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list memory audit records")?;

        Ok(rows
            .into_iter()
            .map(|row| MemoryAuditRecord {
                id: row.get("id"),
                entity_kind: row.get("entity_kind"),
                entity_id: row.get("entity_id"),
                action: row.get("action"),
                actor: row.get("actor"),
                source_task_id: row.get("source_task_id"),
                precondition_hash: row.get("precondition_hash"),
                content_hash: row.get("content_hash"),
                metadata_json: row.get("metadata_json"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    pub fn episodic_snapshot_hash(
        summary: &str,
        entities: &str,
        topics: &str,
        importance: f32,
        domain: &str,
        memory_kind: &str,
        sensitive: bool,
        consolidated: bool,
        consolidation_id: Option<&str>,
    ) -> Result<(String, String)> {
        let snapshot = serde_json::json!({
            "summary": summary,
            "entities": parse_json_array(entities),
            "topics": parse_json_array(topics),
            "importance": importance,
            "domain": domain,
            "memory_kind": memory_kind,
            "sensitive": sensitive,
            "consolidated": consolidated,
            "consolidation_id": consolidation_id,
        });
        let snapshot_json = serde_json::to_string(&snapshot)?;
        Ok((content_hash(&snapshot_json), snapshot_json))
    }

    pub fn fact_snapshot_hash(
        value: &str,
        task_id: Option<&str>,
        memory_id: Option<&str>,
    ) -> Result<(String, String)> {
        let snapshot = serde_json::json!({
            "value": value,
            "task_id": task_id,
            "memory_id": memory_id,
        });
        let snapshot_json = serde_json::to_string(&snapshot)?;
        Ok((content_hash(&snapshot_json), snapshot_json))
    }

    pub fn insight_snapshot_hash(
        insight: &str,
        source_ids: &str,
        domain: Option<&str>,
    ) -> Result<(String, String)> {
        let snapshot = serde_json::json!({
            "insight": insight,
            "source_ids": parse_json_array(source_ids),
            "domain": domain,
        });
        let snapshot_json = serde_json::to_string(&snapshot)?;
        Ok((content_hash(&snapshot_json), snapshot_json))
    }
}

fn parse_json_array(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn content_hash(snapshot_json: &str) -> String {
    blake3::hash(snapshot_json.as_bytes()).to_hex().to_string()
}

pub async fn record_episodic_version_by_id(
    pool: &SqlitePool,
    id: &str,
    action: MemoryMutationAction,
    actor: &str,
    source_task_id: Option<&str>,
) -> Result<Option<MemoryVersionRecord>> {
    let row = sqlx::query(
        r#"SELECT summary, entities, topics, importance, domain, memory_kind, sensitive, consolidated, consolidation_id
           FROM episodic_memory
           WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch episodic memory for versioning")?;

    let Some(row) = row else {
        return Ok(None);
    };

    let (content_hash, snapshot_json) = MemoryAuditRepository::episodic_snapshot_hash(
        row.get::<String, _>("summary").as_str(),
        row.get::<String, _>("entities").as_str(),
        row.get::<String, _>("topics").as_str(),
        row.get::<f32, _>("importance"),
        row.get::<String, _>("domain").as_str(),
        row.get::<String, _>("memory_kind").as_str(),
        row.get::<i64, _>("sensitive") != 0,
        row.get::<i64, _>("consolidated") != 0,
        row.get::<Option<String>, _>("consolidation_id").as_deref(),
    )?;

    let repo = MemoryAuditRepository::new(pool.clone());
    let version = repo
        .record_version(
            MemoryEntityKind::Episodic,
            id,
            action,
            &content_hash,
            &snapshot_json,
            actor,
            source_task_id,
        )
        .await?;
    let _ = repo
        .record_audit(
            MemoryEntityKind::Episodic,
            id,
            action,
            actor,
            source_task_id,
            None,
            Some(&content_hash),
            None,
        )
        .await?;
    Ok(Some(version))
}

pub async fn record_fact_version_by_key(
    pool: &SqlitePool,
    key: &str,
    action: MemoryMutationAction,
    actor: &str,
) -> Result<Option<MemoryVersionRecord>> {
    let row = sqlx::query(
        r#"SELECT value, task_id, memory_id
           FROM memory_facts
           WHERE key = ?"#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch fact for versioning")?;

    let Some(row) = row else {
        return Ok(None);
    };

    let task_id: Option<String> = row.get("task_id");
    let (content_hash, snapshot_json) = MemoryAuditRepository::fact_snapshot_hash(
        row.get::<String, _>("value").as_str(),
        task_id.as_deref(),
        row.get::<Option<String>, _>("memory_id").as_deref(),
    )?;

    let repo = MemoryAuditRepository::new(pool.clone());
    let version = repo
        .record_version(
            MemoryEntityKind::Fact,
            key,
            action,
            &content_hash,
            &snapshot_json,
            actor,
            task_id.as_deref(),
        )
        .await?;
    let _ = repo
        .record_audit(
            MemoryEntityKind::Fact,
            key,
            action,
            actor,
            task_id.as_deref(),
            None,
            Some(&content_hash),
            None,
        )
        .await?;
    Ok(Some(version))
}

pub async fn record_insight_version_by_id(
    pool: &SqlitePool,
    id: &str,
    action: MemoryMutationAction,
    actor: &str,
) -> Result<Option<MemoryVersionRecord>> {
    let row = sqlx::query(
        r#"SELECT insight, source_ids, domain
           FROM consolidation_insights
           WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("Failed to fetch insight for versioning")?;

    let Some(row) = row else {
        return Ok(None);
    };

    let (content_hash, snapshot_json) = MemoryAuditRepository::insight_snapshot_hash(
        row.get::<String, _>("insight").as_str(),
        row.get::<String, _>("source_ids").as_str(),
        row.get::<Option<String>, _>("domain").as_deref(),
    )?;

    let repo = MemoryAuditRepository::new(pool.clone());
    let version = repo
        .record_version(
            MemoryEntityKind::Insight,
            id,
            action,
            &content_hash,
            &snapshot_json,
            actor,
            None,
        )
        .await?;
    let _ = repo
        .record_audit(
            MemoryEntityKind::Insight,
            id,
            action,
            actor,
            None,
            None,
            Some(&content_hash),
            None,
        )
        .await?;
    Ok(Some(version))
}

pub async fn current_episodic_hash(pool: &SqlitePool, id: &str) -> Result<Option<String>> {
    let row = sqlx::query(
        r#"SELECT summary, entities, topics, importance, domain, memory_kind, sensitive, consolidated, consolidation_id
           FROM episodic_memory
           WHERE id = ?"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|row| {
            MemoryAuditRepository::episodic_snapshot_hash(
                row.get::<String, _>("summary").as_str(),
                row.get::<String, _>("entities").as_str(),
                row.get::<String, _>("topics").as_str(),
                row.get::<f32, _>("importance"),
                row.get::<String, _>("domain").as_str(),
                row.get::<String, _>("memory_kind").as_str(),
                row.get::<i64, _>("sensitive") != 0,
                row.get::<i64, _>("consolidated") != 0,
                row.get::<Option<String>, _>("consolidation_id").as_deref(),
            )
            .map(|(hash, _)| hash)
        })
        .transpose()?)
}

pub async fn current_fact_hash(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row = sqlx::query(
        r#"SELECT value, task_id, memory_id
           FROM memory_facts
           WHERE key = ?"#,
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .map(|row| {
            MemoryAuditRepository::fact_snapshot_hash(
                row.get::<String, _>("value").as_str(),
                row.get::<Option<String>, _>("task_id").as_deref(),
                row.get::<Option<String>, _>("memory_id").as_deref(),
            )
            .map(|(hash, _)| hash)
        })
        .transpose()?)
}

pub fn redact_value(label: &str) -> String {
    format!("[REDACTED:{}]", label)
}

pub fn metadata_map(entries: &[(&str, String)]) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let mut map = BTreeMap::new();
    for (key, value) in entries {
        map.insert((*key).to_string(), value.clone());
    }
    serde_json::to_string(&map).ok()
}
