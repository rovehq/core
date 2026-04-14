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
use crate::storage::Database;

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
