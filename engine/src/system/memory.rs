use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use brain::reasoning::LocalBrain;
use serde::{Deserialize, Serialize};

use crate::cli::database_path::database_path;
use crate::conductor::types::TaskDomain;
use crate::conductor::EmbeddingGenerator;
use crate::conductor::{
    GraphPathHit, MemoryContextBundle, MemoryGraphHit, MemoryHit, MemorySystem,
};
use crate::config::{
    Config, MemoryAdapterMode, MemoryBundleStrategy, MemoryGraphEnrichment, MemoryMode,
    MemoryRetrievalAssist,
};
use crate::llm::router::LLMRouter;
use crate::storage::{
    current_episodic_hash, current_fact_hash, metadata_map, redact_value, Database,
    MemoryAuditRecord, MemoryAuditRepository, MemoryEntityKind, MemoryMutationAction,
    MemoryVersionRecord,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemorySurfaceStats {
    pub facts: i64,
    pub task_traces: i64,
    pub episodic: i64,
    pub insights: i64,
    pub total_episodic: i64,
    pub embedded_episodic: i64,
    pub embedding_coverage_pct: f32,
    pub memory_graph_edges: i64,
    pub edge_types: HashMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySurfaceStatus {
    pub mode: String,
    pub bundle_strategy: String,
    pub retrieval_assist: String,
    pub graph_enrichment: String,
    pub scope: String,
    pub code_graph_required: bool,
    pub code_adapter_mode: String,
    pub always_on_enabled: bool,
    pub persist_pinned_facts: bool,
    pub persist_task_traces: bool,
    pub graph_status: crate::memory::knowledge_graph::CodeReviewGraphWorkspaceStatus,
    pub graph_stats: HashMap<String, i64>,
    pub memory_stats: MemorySurfaceStats,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemorySurfaceUpdate {
    pub mode: Option<String>,
    pub bundle_strategy: Option<String>,
    pub retrieval_assist: Option<String>,
    pub graph_enrichment: Option<String>,
    pub code_graph_required: Option<bool>,
    pub code_adapter_mode: Option<String>,
    pub persist_pinned_facts: Option<bool>,
    pub persist_task_traces: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryBackfillRequest {
    pub batch_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBackfillResponse {
    pub backfilled: usize,
    pub status: MemorySurfaceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQueryRequest {
    pub question: String,
    #[serde(default)]
    pub explain: bool,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryExplainResponse {
    pub intent: String,
    pub mode: String,
    pub sources: Vec<String>,
    pub fallback_reason: Option<String>,
    pub graph_paths_used: usize,
    pub memory_graph_hits_used: usize,
    pub task_trace_hits_used: usize,
    pub llm_enrichment_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryQueryResponse {
    pub facts: Vec<MemoryHit>,
    pub preferences: Vec<MemoryHit>,
    pub warnings: Vec<MemoryHit>,
    pub errors: Vec<MemoryHit>,
    pub graph_paths: Vec<GraphPathHit>,
    pub memory_graph_hits: Vec<MemoryGraphHit>,
    pub episodic_hits: Vec<MemoryHit>,
    pub insight_hits: Vec<MemoryHit>,
    pub task_trace_hits: Vec<MemoryHit>,
    pub project_context: Option<String>,
    pub explain: Option<MemoryExplainResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraphInspectResponse {
    pub entity: Option<String>,
    pub graph_status: crate::memory::knowledge_graph::CodeReviewGraphWorkspaceStatus,
    pub graph_stats: HashMap<String, i64>,
    pub paths: Vec<GraphPathHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryIngestRequest {
    pub note: String,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicRecord {
    pub id: String,
    pub task_id: String,
    pub summary: String,
    pub content_hash: String,
    pub importance: f32,
    pub memory_kind: Option<String>,
    pub domain: String,
    pub created_at: i64,
    pub access_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicBrowseResponse {
    pub items: Vec<EpisodicRecord>,
    pub total: i64,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactRecord {
    pub key: String,
    pub value: String,
    pub content_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryVersionHistoryResponse {
    pub entity_kind: String,
    pub entity_id: String,
    pub versions: Vec<MemoryVersionRecord>,
    pub audit: Vec<MemoryAuditRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryDeleteRequest {
    pub expected_content_hash: Option<String>,
    pub actor: Option<String>,
    pub source_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryRedactRequest {
    pub expected_content_hash: Option<String>,
    pub actor: Option<String>,
    pub source_task_id: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDeleteResponse {
    pub deleted: bool,
    pub content_hash: Option<String>,
}

pub struct MemoryManager {
    config: Config,
}

impl MemoryManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn status(&self) -> Result<MemorySurfaceStatus> {
        let database = self.database().await?;
        let memory = self.memory_system(database.pool().clone());
        let graph_status = if self.config.core.workspace.exists() {
            memory
                .code_graph_status(&self.config.core.workspace)
                .await?
        } else {
            Default::default()
        };
        let graph_stats = crate::knowledge_graph::KnowledgeGraph::new(database.pool().clone())
            .get_stats()
            .await?;
        let memory_stats = self.memory_stats(database.pool().clone()).await?;

        let mut warnings = Vec::new();
        if !self.config.memory.always_on_enabled() {
            warnings.push(
                "always_on memory is disabled; only pinned facts, compact task traces, and structural retrieval remain active."
                    .to_string(),
            );
        }
        if self.config.memory.should_query_code_adapter()
            && self.config.memory.code_graph_required
            && graph_status.available_count == 0
        {
            warnings.push(
                "structural code adapter data is missing for the current workspace, so code-path retrieval is degraded."
                    .to_string(),
            );
        }
        if graph_status.stale_count > 0 {
            warnings.push(format!(
                "{} structural adapter repo(s) are stale and should be reindexed.",
                graph_status.stale_count
            ));
        }

        Ok(MemorySurfaceStatus {
            mode: self.config.memory.mode.as_str().to_string(),
            bundle_strategy: self.config.memory.bundle_strategy.as_str().to_string(),
            retrieval_assist: self.config.memory.retrieval_assist.as_str().to_string(),
            graph_enrichment: self.config.memory.graph_enrichment.as_str().to_string(),
            scope: self.config.memory.scope.as_str().to_string(),
            code_graph_required: self.config.memory.code_graph_required,
            code_adapter_mode: self.config.memory.code_adapter_mode.as_str().to_string(),
            always_on_enabled: self.config.memory.always_on_enabled(),
            persist_pinned_facts: self.config.memory.persist_pinned_facts,
            persist_task_traces: self.config.memory.persist_task_traces,
            graph_status,
            graph_stats,
            memory_stats,
            warnings,
        })
    }

    pub async fn replace(&self, update: MemorySurfaceUpdate) -> Result<MemorySurfaceStatus> {
        let mut config = Config::load_or_create()?;
        if let Some(mode) = update.mode.as_deref() {
            config.memory.mode = parse_mode(mode)?;
        }
        if let Some(strategy) = update.bundle_strategy.as_deref() {
            config.memory.bundle_strategy = parse_bundle_strategy(strategy)?;
        }
        if let Some(assist) = update.retrieval_assist.as_deref() {
            config.memory.retrieval_assist = parse_retrieval_assist(assist)?;
        }
        if let Some(enrichment) = update.graph_enrichment.as_deref() {
            config.memory.graph_enrichment = parse_graph_enrichment(enrichment)?;
        }
        if let Some(required) = update.code_graph_required {
            config.memory.code_graph_required = required;
        }
        if let Some(mode) = update.code_adapter_mode.as_deref() {
            config.memory.code_adapter_mode = parse_adapter_mode(mode)?;
        }
        if let Some(persist) = update.persist_pinned_facts {
            config.memory.persist_pinned_facts = persist;
        }
        if let Some(persist) = update.persist_task_traces {
            config.memory.persist_task_traces = persist;
        }
        config.save()?;
        Self::new(config).status().await
    }

    pub async fn set_mode(&self, mode: MemoryMode) -> Result<MemorySurfaceStatus> {
        self.replace(MemorySurfaceUpdate {
            mode: Some(mode.as_str().to_string()),
            bundle_strategy: None,
            retrieval_assist: None,
            graph_enrichment: None,
            code_graph_required: None,
            code_adapter_mode: None,
            persist_pinned_facts: None,
            persist_task_traces: None,
        })
        .await
    }

    pub async fn query(&self, request: MemoryQueryRequest) -> Result<MemoryQueryResponse> {
        let database = self.database().await?;
        let memory = self.memory_system(database.pool().clone());
        let domain = parse_domain(request.domain.as_deref());
        let bundle = memory
            .build_context_bundle(
                &request.question,
                &domain,
                None,
                Some(&self.config.core.workspace),
            )
            .await?;
        Ok(query_response_from_bundle(
            &self.config,
            bundle,
            request.explain,
        ))
    }

    pub async fn inspect_graph(
        &self,
        entity: Option<String>,
    ) -> Result<MemoryGraphInspectResponse> {
        let database = self.database().await?;
        let memory = self.memory_system(database.pool().clone());
        let status = if self.config.core.workspace.exists() {
            memory
                .code_graph_status(&self.config.core.workspace)
                .await?
        } else {
            Default::default()
        };
        let stats = crate::knowledge_graph::KnowledgeGraph::new(database.pool().clone())
            .get_stats()
            .await?;
        let paths = if let Some(entity_name) = entity.as_deref() {
            memory
                .build_context_bundle(
                    entity_name,
                    &TaskDomain::Code,
                    None,
                    Some(&self.config.core.workspace),
                )
                .await?
                .graph_paths
        } else {
            Vec::new()
        };

        Ok(MemoryGraphInspectResponse {
            entity,
            graph_status: status,
            graph_stats: stats,
            paths,
        })
    }

    pub async fn reindex(&self) -> Result<MemorySurfaceStatus> {
        let database = self.database().await?;
        let memory = self.memory_system(database.pool().clone());
        memory
            .reindex_code_graph(&self.config.core.workspace)
            .await?;
        self.status().await
    }

    pub async fn adapter_status(
        &self,
    ) -> Result<crate::memory::knowledge_graph::CodeReviewGraphWorkspaceStatus> {
        let database = self.database().await?;
        let memory = self.memory_system(database.pool().clone());
        if self.config.core.workspace.exists() {
            memory.code_graph_status(&self.config.core.workspace).await
        } else {
            Ok(Default::default())
        }
    }

    pub async fn refresh_adapters(
        &self,
    ) -> Result<crate::memory::knowledge_graph::CodeReviewGraphWorkspaceStatus> {
        self.reindex().await?;
        self.adapter_status().await
    }

    pub async fn backfill_embeddings(&self, batch_size: usize) -> Result<usize> {
        let database = self.database().await?;
        let memory = self.memory_system(database.pool().clone());
        memory.backfill_embeddings(batch_size).await
    }

    pub async fn list_episodic(&self, offset: i64, limit: i64) -> Result<EpisodicBrowseResponse> {
        let database = self.database().await?;
        let pool = database.pool().clone();

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM episodic_memory")
            .fetch_one(&pool)
            .await?;

        let rows = sqlx::query(
            r#"SELECT id, task_id, summary, entities, topics, importance, memory_kind, domain, created_at, access_count, sensitive, consolidated, consolidation_id
               FROM episodic_memory
               ORDER BY created_at DESC
               LIMIT ? OFFSET ?"#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&pool)
        .await?;

        let items = rows
            .iter()
            .map(|row| EpisodicRecord {
                id: sqlx::Row::get(row, "id"),
                task_id: sqlx::Row::get(row, "task_id"),
                summary: sqlx::Row::get(row, "summary"),
                content_hash: MemoryAuditRepository::episodic_snapshot_hash(
                    sqlx::Row::get::<String, _>(row, "summary").as_str(),
                    sqlx::Row::get::<String, _>(row, "entities").as_str(),
                    sqlx::Row::get::<String, _>(row, "topics").as_str(),
                    sqlx::Row::get(row, "importance"),
                    sqlx::Row::get::<String, _>(row, "domain").as_str(),
                    sqlx::Row::get::<String, _>(row, "memory_kind").as_str(),
                    sqlx::Row::get::<i64, _>(row, "sensitive") != 0,
                    sqlx::Row::get::<i64, _>(row, "consolidated") != 0,
                    sqlx::Row::get::<Option<String>, _>(row, "consolidation_id").as_deref(),
                )
                .map(|(hash, _)| hash)
                .unwrap_or_default(),
                importance: sqlx::Row::get(row, "importance"),
                memory_kind: sqlx::Row::get(row, "memory_kind"),
                domain: sqlx::Row::get(row, "domain"),
                created_at: sqlx::Row::get(row, "created_at"),
                access_count: sqlx::Row::get(row, "access_count"),
            })
            .collect();

        Ok(EpisodicBrowseResponse {
            items,
            total,
            offset,
            limit,
        })
    }

    pub async fn list_facts(&self) -> Result<Vec<FactRecord>> {
        let database = self.database().await?;
        let pool = database.pool().clone();

        let rows = sqlx::query(
            r#"SELECT key, value, task_id, memory_id, created_at, updated_at
               FROM memory_facts
               ORDER BY updated_at DESC"#,
        )
        .fetch_all(&pool)
        .await?;

        Ok(rows
            .iter()
            .map(|row| FactRecord {
                key: sqlx::Row::get(row, "key"),
                value: sqlx::Row::get(row, "value"),
                content_hash: MemoryAuditRepository::fact_snapshot_hash(
                    sqlx::Row::get::<String, _>(row, "value").as_str(),
                    sqlx::Row::get::<Option<String>, _>(row, "task_id").as_deref(),
                    sqlx::Row::get::<Option<String>, _>(row, "memory_id").as_deref(),
                )
                .map(|(hash, _)| hash)
                .unwrap_or_default(),
                created_at: sqlx::Row::get(row, "created_at"),
                updated_at: sqlx::Row::get(row, "updated_at"),
            })
            .collect())
    }

    pub async fn episodic_history(&self, id: &str) -> Result<MemoryVersionHistoryResponse> {
        let database = self.database().await?;
        let audit = database.memory_audit();
        Ok(MemoryVersionHistoryResponse {
            entity_kind: MemoryEntityKind::Episodic.as_str().to_string(),
            entity_id: id.to_string(),
            versions: audit.list_versions(MemoryEntityKind::Episodic, id).await?,
            audit: audit.list_audit(MemoryEntityKind::Episodic, id).await?,
        })
    }

    pub async fn fact_history(&self, key: &str) -> Result<MemoryVersionHistoryResponse> {
        let database = self.database().await?;
        let audit = database.memory_audit();
        Ok(MemoryVersionHistoryResponse {
            entity_kind: MemoryEntityKind::Fact.as_str().to_string(),
            entity_id: key.to_string(),
            versions: audit.list_versions(MemoryEntityKind::Fact, key).await?,
            audit: audit.list_audit(MemoryEntityKind::Fact, key).await?,
        })
    }

    pub async fn delete_episodic(
        &self,
        id: &str,
        request: MemoryDeleteRequest,
    ) -> Result<MemoryDeleteResponse> {
        let database = self.database().await?;
        let pool = database.pool().clone();
        let current_hash = current_episodic_hash(&pool, id).await?;
        if let Some(expected) = request.expected_content_hash.as_deref() {
            match current_hash.as_deref() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    return Err(anyhow!(
                        "content hash mismatch for episodic memory '{}': expected {}, found {}",
                        id,
                        expected,
                        actual
                    ));
                }
                None => {
                    return Err(anyhow!("episodic memory '{}' was not found", id));
                }
            }
        } else if current_hash.is_none() {
            return Ok(MemoryDeleteResponse {
                deleted: false,
                content_hash: None,
            });
        }

        let row = sqlx::query(
            r#"SELECT summary, entities, topics, importance, domain, memory_kind, sensitive, consolidated, consolidation_id
               FROM episodic_memory
               WHERE id = ?"#,
        )
        .bind(id)
        .fetch_one(&pool)
        .await?;
        let (content_hash, snapshot_json) = MemoryAuditRepository::episodic_snapshot_hash(
            sqlx::Row::get::<String, _>(&row, "summary").as_str(),
            sqlx::Row::get::<String, _>(&row, "entities").as_str(),
            sqlx::Row::get::<String, _>(&row, "topics").as_str(),
            sqlx::Row::get(&row, "importance"),
            sqlx::Row::get::<String, _>(&row, "domain").as_str(),
            sqlx::Row::get::<String, _>(&row, "memory_kind").as_str(),
            sqlx::Row::get::<i64, _>(&row, "sensitive") != 0,
            sqlx::Row::get::<i64, _>(&row, "consolidated") != 0,
            sqlx::Row::get::<Option<String>, _>(&row, "consolidation_id").as_deref(),
        )?;
        let actor = request.actor.unwrap_or_else(|| "operator".to_string());
        let audit = database.memory_audit();
        audit
            .record_version(
                MemoryEntityKind::Episodic,
                id,
                MemoryMutationAction::Delete,
                &content_hash,
                &snapshot_json,
                &actor,
                request.source_task_id.as_deref(),
            )
            .await?;

        // Cascade delete graph edges first
        sqlx::query("DELETE FROM memory_graph_edges WHERE from_id = ? OR to_id = ?")
            .bind(id)
            .bind(id)
            .execute(&pool)
            .await?;

        let result = sqlx::query("DELETE FROM episodic_memory WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await?;

        audit
            .record_audit(
                MemoryEntityKind::Episodic,
                id,
                MemoryMutationAction::Delete,
                &actor,
                request.source_task_id.as_deref(),
                request.expected_content_hash.as_deref(),
                Some(&content_hash),
                metadata_map(&[("deleted", (result.rows_affected() > 0).to_string())]).as_deref(),
            )
            .await?;

        Ok(MemoryDeleteResponse {
            deleted: result.rows_affected() > 0,
            content_hash: Some(content_hash),
        })
    }

    pub async fn delete_fact(
        &self,
        key: &str,
        request: MemoryDeleteRequest,
    ) -> Result<MemoryDeleteResponse> {
        let database = self.database().await?;
        let pool = database.pool().clone();
        let current_hash = current_fact_hash(&pool, key).await?;
        if let Some(expected) = request.expected_content_hash.as_deref() {
            match current_hash.as_deref() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    return Err(anyhow!(
                        "content hash mismatch for fact '{}': expected {}, found {}",
                        key,
                        expected,
                        actual
                    ));
                }
                None => return Err(anyhow!("memory fact '{}' was not found", key)),
            }
        } else if current_hash.is_none() {
            return Ok(MemoryDeleteResponse {
                deleted: false,
                content_hash: None,
            });
        }

        let row = sqlx::query(
            r#"SELECT value, task_id, memory_id
               FROM memory_facts
               WHERE key = ?"#,
        )
        .bind(key)
        .fetch_one(&pool)
        .await?;
        let task_id: Option<String> = sqlx::Row::get(&row, "task_id");
        let (content_hash, snapshot_json) = MemoryAuditRepository::fact_snapshot_hash(
            sqlx::Row::get::<String, _>(&row, "value").as_str(),
            task_id.as_deref(),
            sqlx::Row::get::<Option<String>, _>(&row, "memory_id").as_deref(),
        )?;
        let actor = request.actor.unwrap_or_else(|| "operator".to_string());
        let audit = database.memory_audit();
        audit
            .record_version(
                MemoryEntityKind::Fact,
                key,
                MemoryMutationAction::Delete,
                &content_hash,
                &snapshot_json,
                &actor,
                request.source_task_id.as_deref().or(task_id.as_deref()),
            )
            .await?;

        let result = sqlx::query("DELETE FROM memory_facts WHERE key = ?")
            .bind(key)
            .execute(&pool)
            .await?;

        audit
            .record_audit(
                MemoryEntityKind::Fact,
                key,
                MemoryMutationAction::Delete,
                &actor,
                request.source_task_id.as_deref().or(task_id.as_deref()),
                request.expected_content_hash.as_deref(),
                Some(&content_hash),
                metadata_map(&[("deleted", (result.rows_affected() > 0).to_string())]).as_deref(),
            )
            .await?;

        Ok(MemoryDeleteResponse {
            deleted: result.rows_affected() > 0,
            content_hash: Some(content_hash),
        })
    }

    pub async fn redact_episodic(
        &self,
        id: &str,
        request: MemoryRedactRequest,
    ) -> Result<EpisodicRecord> {
        let database = self.database().await?;
        let pool = database.pool().clone();
        let current_hash = current_episodic_hash(&pool, id).await?;
        if let Some(expected) = request.expected_content_hash.as_deref() {
            match current_hash.as_deref() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    return Err(anyhow!(
                        "content hash mismatch for episodic memory '{}': expected {}, found {}",
                        id,
                        expected,
                        actual
                    ));
                }
                None => return Err(anyhow!("episodic memory '{}' was not found", id)),
            }
        }

        let redaction_label = request.label.unwrap_or_else(|| "memory".to_string());
        let redacted_summary = redact_value(&redaction_label);
        sqlx::query(
            r#"UPDATE episodic_memory
               SET summary = ?, entities = '[]', topics = '[]', tags = '[]'
               WHERE id = ?"#,
        )
        .bind(&redacted_summary)
        .bind(id)
        .execute(&pool)
        .await?;

        let actor = request.actor.unwrap_or_else(|| "operator".to_string());
        let row = sqlx::query(
            r#"SELECT task_id, summary, importance, memory_kind, domain, created_at, access_count
               FROM episodic_memory
               WHERE id = ?"#,
        )
        .bind(id)
        .fetch_one(&pool)
        .await?;
        let content_hash = current_episodic_hash(&pool, id).await?.unwrap_or_default();
        let audit = database.memory_audit();
        let snapshot_row = sqlx::query(
            r#"SELECT summary, entities, topics, importance, domain, memory_kind, sensitive, consolidated, consolidation_id
               FROM episodic_memory
               WHERE id = ?"#,
        )
        .bind(id)
        .fetch_one(&pool)
        .await?;
        let (_, snapshot_json) = MemoryAuditRepository::episodic_snapshot_hash(
            sqlx::Row::get::<String, _>(&snapshot_row, "summary").as_str(),
            sqlx::Row::get::<String, _>(&snapshot_row, "entities").as_str(),
            sqlx::Row::get::<String, _>(&snapshot_row, "topics").as_str(),
            sqlx::Row::get(&snapshot_row, "importance"),
            sqlx::Row::get::<String, _>(&snapshot_row, "domain").as_str(),
            sqlx::Row::get::<String, _>(&snapshot_row, "memory_kind").as_str(),
            sqlx::Row::get::<i64, _>(&snapshot_row, "sensitive") != 0,
            sqlx::Row::get::<i64, _>(&snapshot_row, "consolidated") != 0,
            sqlx::Row::get::<Option<String>, _>(&snapshot_row, "consolidation_id").as_deref(),
        )?;
        audit
            .record_version(
                MemoryEntityKind::Episodic,
                id,
                MemoryMutationAction::Redact,
                &content_hash,
                &snapshot_json,
                &actor,
                request.source_task_id.as_deref(),
            )
            .await?;
        audit
            .record_audit(
                MemoryEntityKind::Episodic,
                id,
                MemoryMutationAction::Redact,
                &actor,
                request.source_task_id.as_deref(),
                request.expected_content_hash.as_deref(),
                Some(&content_hash),
                metadata_map(&[("label", redaction_label)]).as_deref(),
            )
            .await?;

        Ok(EpisodicRecord {
            id: id.to_string(),
            task_id: sqlx::Row::get(&row, "task_id"),
            summary: sqlx::Row::get(&row, "summary"),
            content_hash,
            importance: sqlx::Row::get(&row, "importance"),
            memory_kind: sqlx::Row::get(&row, "memory_kind"),
            domain: sqlx::Row::get(&row, "domain"),
            created_at: sqlx::Row::get(&row, "created_at"),
            access_count: sqlx::Row::get(&row, "access_count"),
        })
    }

    pub async fn redact_fact(&self, key: &str, request: MemoryRedactRequest) -> Result<FactRecord> {
        let database = self.database().await?;
        let pool = database.pool().clone();
        let current_hash = current_fact_hash(&pool, key).await?;
        if let Some(expected) = request.expected_content_hash.as_deref() {
            match current_hash.as_deref() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    return Err(anyhow!(
                        "content hash mismatch for fact '{}': expected {}, found {}",
                        key,
                        expected,
                        actual
                    ));
                }
                None => return Err(anyhow!("memory fact '{}' was not found", key)),
            }
        }

        let redaction_label = request.label.unwrap_or_else(|| "fact".to_string());
        let updated_at = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"UPDATE memory_facts
               SET value = ?, updated_at = ?
               WHERE key = ?"#,
        )
        .bind(redact_value(&redaction_label))
        .bind(updated_at)
        .bind(key)
        .execute(&pool)
        .await?;

        let row = sqlx::query(
            r#"SELECT value, task_id, memory_id, created_at, updated_at
               FROM memory_facts
               WHERE key = ?"#,
        )
        .bind(key)
        .fetch_one(&pool)
        .await?;
        let content_hash = current_fact_hash(&pool, key).await?.unwrap_or_default();
        let task_id: Option<String> = sqlx::Row::get(&row, "task_id");
        let actor = request.actor.unwrap_or_else(|| "operator".to_string());
        let audit = database.memory_audit();
        let (_, snapshot_json) = MemoryAuditRepository::fact_snapshot_hash(
            sqlx::Row::get::<String, _>(&row, "value").as_str(),
            task_id.as_deref(),
            sqlx::Row::get::<Option<String>, _>(&row, "memory_id").as_deref(),
        )?;
        audit
            .record_version(
                MemoryEntityKind::Fact,
                key,
                MemoryMutationAction::Redact,
                &content_hash,
                &snapshot_json,
                &actor,
                request.source_task_id.as_deref().or(task_id.as_deref()),
            )
            .await?;
        audit
            .record_audit(
                MemoryEntityKind::Fact,
                key,
                MemoryMutationAction::Redact,
                &actor,
                request.source_task_id.as_deref().or(task_id.as_deref()),
                request.expected_content_hash.as_deref(),
                Some(&content_hash),
                metadata_map(&[("label", redaction_label)]).as_deref(),
            )
            .await?;

        Ok(FactRecord {
            key: key.to_string(),
            value: sqlx::Row::get(&row, "value"),
            content_hash,
            created_at: sqlx::Row::get(&row, "created_at"),
            updated_at: sqlx::Row::get(&row, "updated_at"),
        })
    }

    pub async fn ingest_note(&self, request: MemoryIngestRequest) -> Result<MemoryHit> {
        if !self.config.memory.always_on_enabled()
            && !self.config.memory.should_persist_pinned_facts()
        {
            return Err(anyhow!(
                "manual note ingest requires always_on or pinned fact persistence"
            ));
        }

        let database = self.database().await?;
        let memory = self.memory_system(database.pool().clone());
        let domain = parse_domain(request.domain.as_deref());
        let task_id = format!("manual-note-{}", uuid::Uuid::new_v4());
        // Pass empty result — for manual notes there is no "task result" to append
        let result = memory
            .ingest(&request.note, "", &task_id, &domain, false)
            .await?;

        Ok(MemoryHit {
            id: result.memory_id,
            source: "manual_note".to_string(),
            content: result.summary,
            rank: 0.0,
            hit_type: crate::conductor::HitType::Episodic,
            importance: result.importance as f32,
            created_at: chrono::Utc::now().timestamp(),
            final_score: result.importance as f32,
        })
    }

    async fn database(&self) -> Result<Database> {
        Database::new(&database_path(&self.config)).await
    }

    fn memory_system(&self, pool: sqlx::SqlitePool) -> MemorySystem {
        let router = Arc::new(LLMRouter::new(vec![], Arc::new(self.config.llm.clone())));
        let base = MemorySystem::new_with_config(pool.clone(), router, self.config.memory.clone());
        if let Some(local_brain) = detect_local_brain() {
            let generator = Arc::new(EmbeddingGenerator::new(pool, Some(local_brain)));
            base.with_embedding_generator(generator)
        } else {
            base
        }
    }

    async fn memory_stats(&self, pool: sqlx::SqlitePool) -> Result<MemorySurfaceStats> {
        let facts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_facts")
            .fetch_one(&pool)
            .await?;
        let task_traces: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events")
            .fetch_one(&pool)
            .await?;
        let episodic: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM episodic_memory")
            .fetch_one(&pool)
            .await?;
        let insights: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consolidation_insights")
            .fetch_one(&pool)
            .await?;
        let memory_graph_edges: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_graph_edges")
            .fetch_one(&pool)
            .await?;
        let embedded_episodic: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM episodic_memory WHERE embedding IS NOT NULL")
                .fetch_one(&pool)
                .await?;

        let edge_types_rows = sqlx::query(
            r#"SELECT edge_type, COUNT(*) as count
               FROM memory_graph_edges
               GROUP BY edge_type
               ORDER BY edge_type ASC"#,
        )
        .fetch_all(&pool)
        .await?;

        let mut edge_types = HashMap::new();
        for row in edge_types_rows {
            let edge_type: String = sqlx::Row::get(&row, "edge_type");
            let count: i64 = sqlx::Row::get(&row, "count");
            edge_types.insert(edge_type, count);
        }

        let embedding_coverage_pct = if episodic == 0 {
            0.0
        } else {
            (embedded_episodic as f32 / episodic as f32) * 100.0
        };

        Ok(MemorySurfaceStats {
            facts,
            task_traces,
            episodic,
            insights,
            total_episodic: episodic,
            embedded_episodic,
            embedding_coverage_pct,
            memory_graph_edges,
            edge_types,
        })
    }
}

fn query_response_from_bundle(
    config: &Config,
    bundle: MemoryContextBundle,
    explain: bool,
) -> MemoryQueryResponse {
    let explain = explain.then(|| MemoryExplainResponse {
        intent: bundle.plan.intent.as_str().to_string(),
        mode: config.memory.mode.as_str().to_string(),
        sources: bundle.plan.selected_sources.clone(),
        fallback_reason: bundle.plan.fallback_reason.clone(),
        graph_paths_used: bundle.graph_paths.len(),
        memory_graph_hits_used: bundle.memory_graph_hits.len(),
        task_trace_hits_used: bundle.task_trace_hits.len(),
        llm_enrichment_enabled: matches!(
            config.memory.graph_enrichment,
            MemoryGraphEnrichment::DeterministicPlusLlm
        ),
    });

    MemoryQueryResponse {
        facts: bundle.facts,
        preferences: bundle.preferences,
        warnings: bundle.warnings,
        errors: bundle.errors,
        graph_paths: bundle.graph_paths,
        memory_graph_hits: bundle.memory_graph_hits,
        episodic_hits: bundle.episodic_hits,
        insight_hits: bundle.insight_hits,
        task_trace_hits: bundle.task_trace_hits,
        project_context: bundle.project_context,
        explain,
    }
}

fn parse_mode(value: &str) -> Result<MemoryMode> {
    match value {
        "graph_only" | "graph-only" => Ok(MemoryMode::GraphOnly),
        "always_on" | "always-on" => Ok(MemoryMode::AlwaysOn),
        other => Err(anyhow!("unknown memory mode '{}'", other)),
    }
}

fn parse_graph_enrichment(value: &str) -> Result<MemoryGraphEnrichment> {
    match value {
        "deterministic" => Ok(MemoryGraphEnrichment::Deterministic),
        "deterministic_plus_llm" | "deterministic-plus-llm" => {
            Ok(MemoryGraphEnrichment::DeterministicPlusLlm)
        }
        other => Err(anyhow!("unknown graph enrichment '{}'", other)),
    }
}

fn parse_bundle_strategy(value: &str) -> Result<MemoryBundleStrategy> {
    match value {
        "adaptive" => Ok(MemoryBundleStrategy::Adaptive),
        other => Err(anyhow!("unknown bundle strategy '{}'", other)),
    }
}

fn parse_retrieval_assist(value: &str) -> Result<MemoryRetrievalAssist> {
    match value {
        "off" => Ok(MemoryRetrievalAssist::Off),
        "rerank" => Ok(MemoryRetrievalAssist::Rerank),
        "compress" => Ok(MemoryRetrievalAssist::Compress),
        other => Err(anyhow!("unknown retrieval assist '{}'", other)),
    }
}

fn parse_adapter_mode(value: &str) -> Result<MemoryAdapterMode> {
    match value {
        "off" => Ok(MemoryAdapterMode::Off),
        "auto" => Ok(MemoryAdapterMode::Auto),
        "required" => Ok(MemoryAdapterMode::Required),
        other => Err(anyhow!("unknown adapter mode '{}'", other)),
    }
}

fn parse_domain(value: Option<&str>) -> TaskDomain {
    match value.unwrap_or("general").to_ascii_lowercase().as_str() {
        "code" => TaskDomain::Code,
        "git" => TaskDomain::Git,
        "shell" => TaskDomain::Shell,
        "browser" => TaskDomain::Browser,
        "data" => TaskDomain::Data,
        _ => TaskDomain::General,
    }
}

fn detect_local_brain() -> Option<Arc<LocalBrain>> {
    let metadata = read_local_brain_metadata()?;
    let port = metadata.port;
    let model = metadata.model_name();
    Some(Arc::new(LocalBrain::new(
        format!("http://localhost:{}", port),
        model,
    )))
}

#[derive(Debug, Deserialize)]
struct LocalBrainMetadata {
    model_path: String,
    port: u16,
}

impl LocalBrainMetadata {
    fn model_name(&self) -> String {
        std::path::Path::new(&self.model_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("qwen2.5-coder-0.5b")
            .to_string()
    }
}

fn read_local_brain_metadata() -> Option<LocalBrainMetadata> {
    let path = LocalBrain::default_brain_dir()?.join("llama-server.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_manager() -> (TempDir, Database, MemoryManager) {
        let temp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.core.workspace = temp.path().join("workspace");
        config.core.data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config.core.workspace).unwrap();
        std::fs::create_dir_all(&config.core.data_dir).unwrap();

        let database = Database::new(&database_path(&config)).await.unwrap();
        let manager = MemoryManager::new(config);
        (temp, database, manager)
    }

    async fn insert_episodic(pool: &sqlx::SqlitePool, id: &str, summary: &str) {
        sqlx::query(
            r#"INSERT INTO episodic_memory
               (id, task_id, summary, entities, topics, importance, consolidated, tags, created_at, domain, sensitive, memory_kind, access_count)
               VALUES (?, ?, ?, '[]', '[]', 0.8, 0, '[]', ?, 'general', 0, 'task_trace', 0)"#,
        )
        .bind(id)
        .bind("task-1")
        .bind(summary)
        .bind(chrono::Utc::now().timestamp())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_fact(pool: &sqlx::SqlitePool, key: &str, value: &str) {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO memory_facts (key, value, task_id, memory_id, created_at, updated_at)
               VALUES (?, ?, 'task-1', NULL, ?, ?)"#,
        )
        .bind(key)
        .bind(value)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn delete_episodic_rejects_stale_content_hash() {
        let (_temp, database, manager) = test_manager().await;
        insert_episodic(database.pool(), "mem-1", "remember tenant alpha").await;

        let error = manager
            .delete_episodic(
                "mem-1",
                MemoryDeleteRequest {
                    expected_content_hash: Some("stale".to_string()),
                    actor: Some("tester".to_string()),
                    source_task_id: None,
                },
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("content hash mismatch"));
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM episodic_memory WHERE id = ?")
                .bind("mem-1")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(remaining, 1);
    }

    #[tokio::test]
    async fn redact_fact_records_version_and_audit() {
        let (_temp, database, manager) = test_manager().await;
        insert_fact(database.pool(), "tenant.alpha", "staging api uses alpha").await;
        let expected_hash = current_fact_hash(database.pool(), "tenant.alpha")
            .await
            .unwrap()
            .unwrap();

        let updated = manager
            .redact_fact(
                "tenant.alpha",
                MemoryRedactRequest {
                    expected_content_hash: Some(expected_hash.clone()),
                    actor: Some("compliance".to_string()),
                    source_task_id: Some("case-42".to_string()),
                    label: Some("tenant".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.value, "[REDACTED:tenant]");
        let history = manager.fact_history("tenant.alpha").await.unwrap();
        assert_eq!(history.versions.len(), 1);
        assert_eq!(history.versions[0].action, "redact");
        assert_eq!(history.audit.len(), 1);
        assert_eq!(
            history.audit[0].precondition_hash.as_deref(),
            Some(expected_hash.as_str())
        );
        assert_eq!(history.audit[0].actor, "compliance");
    }

    #[tokio::test]
    async fn delete_fact_records_delete_version_and_audit() {
        let (_temp, database, manager) = test_manager().await;
        insert_fact(database.pool(), "tenant.beta", "prod api uses beta").await;
        let expected_hash = current_fact_hash(database.pool(), "tenant.beta")
            .await
            .unwrap()
            .unwrap();

        let deleted = manager
            .delete_fact(
                "tenant.beta",
                MemoryDeleteRequest {
                    expected_content_hash: Some(expected_hash.clone()),
                    actor: Some("operator".to_string()),
                    source_task_id: Some("cleanup-7".to_string()),
                },
            )
            .await
            .unwrap();

        assert!(deleted.deleted);
        assert_eq!(
            deleted.content_hash.as_deref(),
            Some(expected_hash.as_str())
        );

        let history = manager.fact_history("tenant.beta").await.unwrap();
        assert_eq!(history.versions.len(), 1);
        assert_eq!(history.versions[0].action, "delete");
        assert_eq!(history.audit.len(), 1);
        assert_eq!(history.audit[0].action, "delete");
        assert_eq!(
            history.audit[0].precondition_hash.as_deref(),
            Some(expected_hash.as_str())
        );

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_facts WHERE key = ?")
            .bind("tenant.beta")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    }
}
