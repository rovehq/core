//! Memory System
//!
//! Implements the three memory operations from the Rove architecture:
//! - `ingest()` / `consolidate()` / `query()`
//!
//! Both extraction and consolidation have configurable backends:
//!   - `Heuristic` — regex/patterns, zero LLM dependency
//!   - `Local`     — local LLM (Ollama / LocalBrain)
//!   - `Cloud`     — cloud LLM (OpenAI / Anthropic / Gemini)
//!   - `Auto`      — best available, falls back gracefully (default)
//!
//! Typed query accessors let callers load only the memory subset they need:
//!   `query_facts`, `query_kind`, `query_preferences`, `query_warnings`,
//!   `query_errors`, `query_entity`, `query_recent`

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{debug, error, info};

use crate::conductor::extract::{build_extractor, MemoryExtractor};
use crate::conductor::types::{
    ConsolidationResult, GraphPathHit, HitType, IngestResult, MemoryContextBundle, MemoryHit,
    MemoryIntent, MemoryKind, MemoryPlan, TaskDomain,
};
use crate::config::MemoryConfig;
use crate::llm::router::LLMRouter;
use crate::memory::knowledge_graph::{
    ensure_workspace_imported, workspace_status, CodeReviewGraphImportReport,
    CodeReviewGraphWorkspaceStatus, GraphEdge, GraphNode, RelationType,
};

// ─────────────────────────────────────────────────────────────────────────────
// MemorySystem
// ─────────────────────────────────────────────────────────────────────────────

pub struct MemorySystem {
    pool: SqlitePool,
    router: Arc<LLMRouter>,
    extractor: Arc<dyn MemoryExtractor>,
    knowledge_graph: Arc<crate::knowledge_graph::KnowledgeGraph>,
    entity_extractor: Arc<crate::knowledge_graph::EntityExtractor>,
    embedding_generator: Option<Arc<crate::conductor::EmbeddingGenerator>>,
    config: MemoryConfig,
}

impl MemorySystem {
    pub fn new(pool: SqlitePool, router: Arc<LLMRouter>) -> Self {
        Self::new_with_config(pool, router, MemoryConfig::default())
    }

    pub fn new_with_config(pool: SqlitePool, router: Arc<LLMRouter>, config: MemoryConfig) -> Self {
        let knowledge_graph = Arc::new(crate::knowledge_graph::KnowledgeGraph::new(pool.clone()));
        let entity_extractor =
            Arc::new(crate::knowledge_graph::EntityExtractor::new(router.clone()));
        let extractor = build_extractor(&config.extraction_backend, router.clone());
        Self {
            pool,
            router,
            extractor,
            knowledge_graph,
            entity_extractor,
            embedding_generator: None,
            config,
        }
    }

    pub fn with_embedding_generator(
        mut self,
        generator: Arc<crate::conductor::EmbeddingGenerator>,
    ) -> Self {
        // Auto-enable embedding at ingest when a generator is attached so that
        // new memories immediately participate in hybrid vector+BM25 search.
        self.config.embed_at_ingest = true;
        self.embedding_generator = Some(generator);
        self
    }

    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    pub async fn code_graph_status(
        &self,
        workspace_root: &Path,
    ) -> Result<CodeReviewGraphWorkspaceStatus> {
        workspace_status(&self.pool, workspace_root).await
    }

    pub async fn reindex_code_graph(
        &self,
        workspace_root: &Path,
    ) -> Result<CodeReviewGraphImportReport> {
        ensure_workspace_imported(
            &self.pool,
            &self.knowledge_graph,
            workspace_root,
            self.config.scope.as_str(),
        )
        .await
    }

    // ── ingest ───────────────────────────────────────────────────────────

    pub async fn ingest(
        &self,
        task_input: &str,
        task_result: &str,
        task_id: &str,
        domain: &TaskDomain,
        sensitive: bool,
    ) -> Result<IngestResult> {
        if !self.config.always_on_enabled() {
            if self.config.should_persist_pinned_facts() {
                return crate::conductor::episodic::ingest_pinned_facts_only(
                    &self.pool,
                    &self.extractor,
                    task_input,
                    task_result,
                    task_id,
                    domain,
                    sensitive,
                    self.config.fact_store_enabled,
                )
                .await;
            }

            return Ok(IngestResult {
                memory_id: format!("graph-only:{task_id}"),
                summary:
                    "graph_only mode skips task-derived episodic memory; only task traces and pinned facts are retained."
                        .to_string(),
                entities: Vec::new(),
                topics: Vec::new(),
                importance: 0.0,
                kind: MemoryKind::General,
            });
        }

        crate::conductor::episodic::ingest(
            &self.pool,
            &self.extractor,
            &self.knowledge_graph,
            &self.entity_extractor,
            task_input,
            task_result,
            task_id,
            domain,
            sensitive,
            self.config.fact_store_enabled,
            self.config.embed_at_ingest,
            self.embedding_generator.as_ref(),
        )
        .await
    }

    // ── consolidate ──────────────────────────────────────────────────────

    pub async fn consolidate(&self) -> Result<ConsolidationResult> {
        if !self.config.always_on_enabled() {
            return Ok(ConsolidationResult::Skipped {
                reason: "memory mode is graph_only".to_string(),
            });
        }

        let result = crate::conductor::consolidation::consolidate(
            &self.pool,
            &self.router,
            self.config.min_to_consolidate,
            &self.config.consolidation_backend,
        )
        .await?;
        crate::conductor::decay::decay_importance(
            &self.pool,
            self.config.importance_decay_enabled,
            self.config.episodic_retention_days,
        )
        .await?;
        Ok(result)
    }

    // ── general query ────────────────────────────────────────────────────

    pub async fn query(
        &self,
        question: &str,
        domain: &TaskDomain,
        team_id: Option<&str>,
    ) -> Result<Vec<MemoryHit>> {
        let bundle = self
            .build_context_bundle(question, domain, team_id, None)
            .await?;
        let mut hits = bundle.flattened_hits();
        hits.extend(graph_paths_to_hits(&bundle.graph_paths));
        sort_hits(&mut hits);
        hits.truncate(self.config.query_limit as usize);
        Ok(hits)
    }

    pub async fn query_hybrid(
        &self,
        question: &str,
        domain: &TaskDomain,
        team_id: Option<&str>,
    ) -> Result<Vec<MemoryHit>> {
        self.query(question, domain, team_id).await
    }

    pub async fn build_context_bundle(
        &self,
        question: &str,
        domain: &TaskDomain,
        team_id: Option<&str>,
        workspace_root: Option<&Path>,
    ) -> Result<MemoryContextBundle> {
        let intent = detect_memory_intent(question, domain);
        let mut fallback_reason = None;
        let layers = crate::conductor::types::ContextLayers::for_domain(domain);
        let mut plan = build_memory_plan(
            &self.config,
            intent,
            domain,
            self.config.query_limit as usize,
            fallback_reason.clone(),
        );
        let use_structural_adapter = plan
            .selected_sources
            .iter()
            .any(|source| source == "structural_adapter");

        if use_structural_adapter && layers.knowledge_graph {
            if let Some(root) = workspace_root {
                let mut status = workspace_status(&self.pool, root).await?;
                if status.available_count > 0
                    && (status.imported_count < status.available_count || status.stale_count > 0)
                {
                    let _ = ensure_workspace_imported(
                        &self.pool,
                        &self.knowledge_graph,
                        root,
                        self.config.scope.as_str(),
                    )
                    .await?;
                    status = workspace_status(&self.pool, root).await?;
                }

                if graph_required_for_domain(domain) && self.config.code_graph_required {
                    if status.available_count == 0 {
                        fallback_reason = Some(
                            "code-review-graph is missing for the active workspace".to_string(),
                        );
                    } else if status.stale_count > 0 {
                        fallback_reason = Some(
                            "structural code adapter is stale for one or more repos".to_string(),
                        );
                    }
                }
            } else if graph_required_for_domain(domain) && self.config.code_graph_required {
                fallback_reason = Some(
                    "workspace root is unavailable, so structural code graph import could not run"
                        .to_string(),
                );
            }
        }
        plan.fallback_reason = fallback_reason.clone();

        let project_context = build_project_context(workspace_root).await;
        let graph_paths = if use_structural_adapter && layers.knowledge_graph {
            let paths = self
                .query_graph_paths(question, plan.graph_depth, plan.adapter_budget)
                .await?;
            if paths.is_empty()
                && graph_required_for_domain(domain)
                && plan.fallback_reason.is_none()
            {
                plan.fallback_reason =
                    Some("no structural adapter paths matched this query".to_string());
            }
            paths
        } else {
            Vec::new()
        };

        let (
            facts,
            preferences,
            warnings,
            errors,
            episodic_hits,
            insight_hits,
            task_trace_hits,
            memory_graph_hits,
        ) = {
            let mut facts = if plan
                .selected_sources
                .iter()
                .any(|source| source == "pinned_facts")
            {
                self.query_facts(question).await?
            } else {
                Vec::new()
            };
            facts.truncate(plan.facts_budget);

            let mut preferences = if self.config.always_on_enabled()
                && plan
                    .selected_sources
                    .iter()
                    .any(|source| source == "typed_memory")
            {
                self.query_preferences(domain).await?
            } else {
                Vec::new()
            };
            preferences.truncate(plan.facts_budget.min(3));

            let mut warnings = if self.config.always_on_enabled()
                && plan
                    .selected_sources
                    .iter()
                    .any(|source| source == "typed_memory")
            {
                self.query_warnings(domain).await?
            } else {
                Vec::new()
            };
            warnings.truncate(plan.facts_budget.min(3));

            let mut errors = if self.config.always_on_enabled()
                && plan
                    .selected_sources
                    .iter()
                    .any(|source| source == "typed_memory")
            {
                self.query_errors(domain).await?
            } else {
                Vec::new()
            };
            errors.truncate(plan.semantic_budget.min(4));

            let semantic_hits = if self.config.always_on_enabled()
                && plan
                    .selected_sources
                    .iter()
                    .any(|source| source == "semantic_memory")
            {
                if matches!(intent, MemoryIntent::RecentContext) {
                    self.query_recent(domain, plan.semantic_budget.max(1))
                        .await?
                } else {
                    crate::conductor::query::query_hybrid(
                        &self.pool,
                        self.embedding_generator.as_ref(),
                        question,
                        domain,
                        team_id,
                        plan.semantic_budget.max(1),
                        self.config.min_importance_to_inject,
                    )
                    .await?
                }
            } else {
                Vec::new()
            };

            let mut task_trace_hits = if plan
                .selected_sources
                .iter()
                .any(|source| source == "task_traces")
            {
                if matches!(intent, MemoryIntent::RecentContext) {
                    self.query_recent_task_traces(domain, plan.task_trace_budget.max(1))
                        .await?
                } else {
                    self.query_task_traces(question, domain, plan.task_trace_budget.max(1))
                        .await?
                }
            } else {
                Vec::new()
            };

            let typed_ids: std::collections::HashSet<String> = facts
                .iter()
                .chain(preferences.iter())
                .chain(warnings.iter())
                .chain(errors.iter())
                .chain(task_trace_hits.iter())
                .map(|h| h.id.clone())
                .collect();

            let mut episodic_hits = Vec::new();
            let mut insight_hits = Vec::new();
            let mut semantic_task_trace_hits = Vec::new();
            for hit in semantic_hits {
                if typed_ids.contains(&hit.id) {
                    continue;
                }
                match hit.hit_type {
                    HitType::Insight => insight_hits.push(hit),
                    HitType::TaskTrace => semantic_task_trace_hits.push(hit),
                    HitType::Episodic => episodic_hits.push(hit),
                    HitType::Fact | HitType::KnowledgeGraph => {}
                }
            }

            task_trace_hits.extend(semantic_task_trace_hits);
            sort_hits(&mut task_trace_hits);
            task_trace_hits.truncate(plan.task_trace_budget.max(1));

            let domain_str = format!("{:?}", domain).to_ascii_lowercase();
            let memory_graph_hits = if plan
                .selected_sources
                .iter()
                .any(|source| source == "memory_graph")
            {
                crate::conductor::memory_graph::memory_graph_context(
                    &self.pool,
                    question,
                    &domain_str,
                    self.config.min_importance_to_inject,
                    plan.memory_graph_budget.max(1),
                )
                .await
                .unwrap_or_default()
            } else {
                Vec::new()
            };

            (
                facts,
                preferences,
                warnings,
                errors,
                episodic_hits,
                insight_hits,
                task_trace_hits,
                memory_graph_hits,
            )
        };

        Ok(MemoryContextBundle {
            plan,
            facts,
            preferences,
            warnings,
            errors,
            graph_paths,
            episodic_hits,
            insight_hits,
            task_trace_hits,
            memory_graph_hits,
            project_context,
        })
    }

    // ── typed query accessors ─────────────────────────────────────────────

    /// Query the structured fact store via FTS.
    pub async fn query_facts(&self, question: &str) -> Result<Vec<MemoryHit>> {
        crate::conductor::fact_store::query_from_fact_store(
            &self.pool,
            question,
            self.config.query_limit as usize,
        )
        .await
    }

    /// Retrieve all stored facts (no FTS — use for full context injection).
    pub async fn get_all_facts(&self) -> Result<Vec<crate::conductor::fact_store::FactRow>> {
        crate::conductor::fact_store::get_all_facts(&self.pool).await
    }

    /// Query memories by MemoryKind (loads only the requested kind).
    pub async fn query_kind(
        &self,
        kind: &MemoryKind,
        domain: &TaskDomain,
    ) -> Result<Vec<MemoryHit>> {
        crate::conductor::query::query_by_kind(
            &self.pool,
            kind,
            domain,
            self.config.query_limit as usize,
            self.config.min_importance_to_inject,
        )
        .await
    }

    /// Query Preference-kind memories for a domain.
    pub async fn query_preferences(&self, domain: &TaskDomain) -> Result<Vec<MemoryHit>> {
        self.query_kind(&MemoryKind::Preference, domain).await
    }

    /// Query Warning-kind memories for a domain.
    pub async fn query_warnings(&self, domain: &TaskDomain) -> Result<Vec<MemoryHit>> {
        self.query_kind(&MemoryKind::Warning, domain).await
    }

    /// Query Error + Fix kind memories for a domain (combined).
    pub async fn query_errors(&self, domain: &TaskDomain) -> Result<Vec<MemoryHit>> {
        let limit = self.config.query_limit as usize;
        let min_imp = self.config.min_importance_to_inject;
        let mut hits = crate::conductor::query::query_by_kind(
            &self.pool,
            &MemoryKind::Error,
            domain,
            limit,
            min_imp,
        )
        .await?;
        hits.extend(
            crate::conductor::query::query_by_kind(
                &self.pool,
                &MemoryKind::Fix,
                domain,
                limit,
                min_imp,
            )
            .await?,
        );
        hits.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        Ok(hits)
    }

    /// Query memories mentioning a specific entity or concept via FTS.
    pub async fn query_entity(
        &self,
        entity_name: &str,
        domain: &TaskDomain,
    ) -> Result<Vec<MemoryHit>> {
        crate::conductor::query::query_entity_scoped(
            &self.pool,
            entity_name,
            domain,
            self.config.query_limit as usize,
        )
        .await
    }

    /// Return the N most recent episodic memories for a domain.
    pub async fn query_recent(&self, domain: &TaskDomain, limit: usize) -> Result<Vec<MemoryHit>> {
        crate::conductor::query::query_recent(
            &self.pool,
            domain,
            limit,
            self.config.min_importance_to_inject,
        )
        .await
    }

    pub async fn query_task_traces(
        &self,
        question: &str,
        domain: &TaskDomain,
        limit: usize,
    ) -> Result<Vec<MemoryHit>> {
        crate::conductor::query::query_task_traces(&self.pool, question, domain, limit).await
    }

    pub async fn query_recent_task_traces(
        &self,
        domain: &TaskDomain,
        limit: usize,
    ) -> Result<Vec<MemoryHit>> {
        crate::conductor::query::query_recent_task_traces(&self.pool, domain, limit).await
    }

    // ── hybrid search / backfill ─────────────────────────────────────────

    /// Generate embeddings for memories that were ingested before a generator
    /// was attached (or before `embed_at_ingest` was enabled).
    ///
    /// Returns the number of newly embedded memories.
    pub async fn backfill_embeddings(&self, batch_size: usize) -> Result<usize> {
        let Some(generator) = &self.embedding_generator else {
            return Ok(0);
        };
        generator.backfill_embeddings(batch_size).await
    }

    // ── decay ────────────────────────────────────────────────────────────

    pub async fn decay_importance(&self, enabled: bool) -> Result<()> {
        crate::conductor::decay::decay_importance(
            &self.pool,
            enabled,
            self.config.episodic_retention_days,
        )
        .await
    }

    async fn query_graph_paths(
        &self,
        question: &str,
        graph_depth: usize,
        limit: usize,
    ) -> Result<Vec<GraphPathHit>> {
        let mut seeds = Vec::new();
        let mut seen_seeds = HashSet::new();
        for term in graph_terms(question) {
            for node in self.knowledge_graph.search_nodes(&term).await? {
                if seen_seeds.insert(node.id.clone()) {
                    seeds.push(node);
                }
            }
        }

        if seeds.is_empty() {
            if let Some(node) = self.knowledge_graph.find_node_by_label(question).await? {
                seeds.push(node);
            }
        }

        let mut hits = Vec::new();
        let mut seen_paths = HashSet::new();
        let edge_budget = graph_depth.max(1) * 2;

        for seed in seeds.into_iter().take(limit.max(1)) {
            let mut edges = self.knowledge_graph.get_outgoing_edges(&seed.id).await?;
            edges.extend(self.knowledge_graph.get_incoming_edges(&seed.id).await?);
            edges.sort_by(|left, right| {
                left.source_kind
                    .deterministic_rank()
                    .cmp(&right.source_kind.deterministic_rank())
                    .then_with(|| right.confidence.total_cmp(&left.confidence))
                    .then_with(|| right.weight.total_cmp(&left.weight))
            });

            for edge in edges.into_iter().take(edge_budget) {
                let hit = self.graph_path_hit_for_edge(&seed, &edge).await?;
                let Some(hit) = hit else {
                    continue;
                };
                if seen_paths.insert(hit.summary.clone()) {
                    hits.push(hit);
                }
            }
        }

        hits.sort_by(|left, right| right.score.total_cmp(&left.score));
        hits.truncate(limit);
        Ok(hits)
    }

    async fn graph_path_hit_for_edge(
        &self,
        seed: &GraphNode,
        edge: &GraphEdge,
    ) -> Result<Option<GraphPathHit>> {
        let (from_node, to_node) = if edge.from_id == seed.id {
            let Some(target) = self.knowledge_graph.get_node(&edge.to_id).await? else {
                return Ok(None);
            };
            (seed.clone(), target)
        } else {
            let Some(source) = self.knowledge_graph.get_node(&edge.from_id).await? else {
                return Ok(None);
            };
            (source, seed.clone())
        };

        let mut source_kinds = vec![
            from_node.source_kind.as_str().to_string(),
            edge.source_kind.as_str().to_string(),
            to_node.source_kind.as_str().to_string(),
        ];
        source_kinds.sort();
        source_kinds.dedup();

        let mut source_refs = Vec::new();
        if let Some(source_ref) = from_node.source_ref.clone() {
            source_refs.push(source_ref);
        }
        if let Some(source_ref) = edge.source_ref.clone() {
            source_refs.push(source_ref);
        }
        if let Some(source_ref) = to_node.source_ref.clone() {
            source_refs.push(source_ref);
        }

        let confidence =
            ((from_node.confidence + edge.confidence + to_node.confidence) / 3.0).clamp(0.0, 1.0);
        let score = (2.0 - edge.source_kind.deterministic_rank() as f32 * 0.5)
            + confidence
            + edge.weight.clamp(0.0, 1.0);

        Ok(Some(GraphPathHit {
            summary: format_graph_relationship(&from_node, &to_node, &edge.relation),
            path: vec![
                from_node.label.clone(),
                edge.relation.as_str().to_string(),
                to_node.label.clone(),
            ],
            source_kinds,
            source_refs,
            confidence,
            score,
        }))
    }

    // ── consolidation loop ───────────────────────────────────────────────

    pub async fn start_consolidation_loop(self: Arc<Self>, interval: Duration) {
        if !self.config.always_on_enabled() {
            info!("Consolidation loop skipped because memory mode is graph_only");
            return;
        }
        info!("Consolidation loop started (interval: {:?})", interval);
        loop {
            tokio::time::sleep(interval).await;
            debug!("Running scheduled consolidation");
            match self.consolidate().await {
                Ok(ConsolidationResult::Skipped { reason }) => {
                    debug!("Consolidation skipped: {}", reason);
                }
                Ok(ConsolidationResult::Completed {
                    memories_processed,
                    insights_generated,
                }) => {
                    info!(
                        "Consolidation: {} memories → {} insights",
                        memories_processed, insights_generated
                    );
                }
                Err(e) => {
                    error!("Consolidation loop error (continuing): {}", e);
                }
            }
        }
    }
}

fn build_memory_plan(
    config: &MemoryConfig,
    intent: MemoryIntent,
    domain: &TaskDomain,
    query_limit: usize,
    fallback_reason: Option<String>,
) -> MemoryPlan {
    let mut selected_sources = Vec::new();
    if matches!(
        domain,
        TaskDomain::Code | TaskDomain::Git | TaskDomain::General
    ) {
        selected_sources.push("project_context".to_string());
    }

    if config.should_persist_pinned_facts() {
        selected_sources.push("pinned_facts".to_string());
    }

    if config.should_query_task_traces()
        && matches!(
            intent,
            MemoryIntent::IncidentOrFix
                | MemoryIntent::RecentContext
                | MemoryIntent::GeneralSemantic
        )
    {
        selected_sources.push("task_traces".to_string());
    }

    if config.always_on_enabled()
        && !matches!(
            intent,
            MemoryIntent::StructuralCode | MemoryIntent::RelationshipQuery
        )
    {
        selected_sources.push("memory_graph".to_string());

        if matches!(
            intent,
            MemoryIntent::Preference | MemoryIntent::Warning | MemoryIntent::IncidentOrFix
        ) {
            selected_sources.push("typed_memory".to_string());
        }

        if !matches!(
            intent,
            MemoryIntent::UserFact
                | MemoryIntent::Preference
                | MemoryIntent::Warning
                | MemoryIntent::StructuralCode
                | MemoryIntent::RelationshipQuery
        ) {
            selected_sources.push("semantic_memory".to_string());
        }
    }

    if config.should_query_code_adapter()
        && matches!(
            intent,
            MemoryIntent::StructuralCode | MemoryIntent::RelationshipQuery
        )
        && graph_required_for_domain(domain)
    {
        selected_sources.push("structural_adapter".to_string());
    }

    let (
        graph_depth,
        facts_budget,
        task_trace_budget,
        memory_graph_budget,
        adapter_budget,
        semantic_budget,
    ) = match intent {
        MemoryIntent::StructuralCode | MemoryIntent::RelationshipQuery => (
            3,
            4.min(query_limit),
            query_limit.min(4),
            query_limit.min(3),
            query_limit.min(8),
            query_limit.min(4),
        ),
        MemoryIntent::UserFact | MemoryIntent::Preference | MemoryIntent::Warning => (
            1,
            query_limit.min(6),
            query_limit.min(2),
            query_limit.min(4),
            0,
            query_limit.min(2),
        ),
        MemoryIntent::IncidentOrFix | MemoryIntent::RecentContext => (
            2,
            query_limit.min(4),
            query_limit.min(6),
            query_limit.min(6),
            0,
            query_limit.min(6),
        ),
        MemoryIntent::GeneralSemantic => (
            2,
            query_limit.min(4),
            query_limit.min(3),
            query_limit.min(4),
            0,
            query_limit.min(5),
        ),
    };

    selected_sources.sort();
    selected_sources.dedup();

    MemoryPlan {
        intent,
        selected_sources,
        graph_depth,
        facts_budget,
        task_trace_budget,
        memory_graph_budget,
        adapter_budget,
        semantic_budget,
        fallback_reason,
    }
}

fn detect_memory_intent(question: &str, domain: &TaskDomain) -> MemoryIntent {
    let query = question.to_ascii_lowercase();
    if query.contains("remember") || query.contains("my ") || query.contains("user fact") {
        return MemoryIntent::UserFact;
    }
    if query.contains("prefer") || query.contains("preference") || query.contains("always use") {
        return MemoryIntent::Preference;
    }
    if query.contains("warning") || query.contains("avoid") || query.contains("never ") {
        return MemoryIntent::Warning;
    }
    if query.contains("error")
        || query.contains("failed")
        || query.contains("failure")
        || query.contains("fix")
        || query.contains("incident")
        || query.contains("regression")
    {
        return MemoryIntent::IncidentOrFix;
    }
    if query.contains("recent")
        || query.contains("last time")
        || query.contains("previously")
        || query.contains("what happened")
    {
        return MemoryIntent::RecentContext;
    }
    if query.contains("relationship")
        || query.contains("depends")
        || query.contains("dependency")
        || query.contains("caller")
        || query.contains("callee")
        || query.contains("imports")
        || query.contains("tests")
        || query.contains("path")
    {
        return MemoryIntent::RelationshipQuery;
    }
    if matches!(domain, TaskDomain::Code | TaskDomain::Git)
        || query.contains("module")
        || query.contains("function")
        || query.contains("file")
        || query.contains("architecture")
    {
        return MemoryIntent::StructuralCode;
    }
    MemoryIntent::GeneralSemantic
}

async fn build_project_context(workspace_root: Option<&Path>) -> Option<String> {
    let root = workspace_root?;
    crate::conductor::project::ProjectMemory::scan(root)
        .await
        .ok()
        .map(|project| project.format_for_prompt())
}

fn graph_required_for_domain(domain: &TaskDomain) -> bool {
    matches!(domain, TaskDomain::Code | TaskDomain::Git)
}

fn graph_terms(question: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "what", "where", "when", "which", "that", "this", "with", "from", "into", "about",
        "stored", "using", "project", "please", "there", "show", "list", "memory", "graph",
        "recent", "context",
    ];

    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for token in question
        .split(|ch: char| {
            !ch.is_ascii_alphanumeric() && ch != '_' && ch != '/' && ch != '~' && ch != ':'
        })
        .filter(|token| token.len() >= 3)
    {
        let token = token.to_ascii_lowercase();
        if STOP_WORDS.contains(&token.as_str()) || !seen.insert(token.clone()) {
            continue;
        }
        terms.push(token);
    }
    terms
}

fn format_graph_relationship(from: &GraphNode, to: &GraphNode, relation: &RelationType) -> String {
    match relation {
        RelationType::WorksOn => format!("{} works on {}", from.label, to.label),
        RelationType::StoredAt => format!("{} is stored at {}", from.label, to.label),
        RelationType::Uses => format!("{} uses {}", from.label, to.label),
        RelationType::UsedBy => format!("{} is used by {}", from.label, to.label),
        RelationType::TestedBy => format!("{} is tested by {}", from.label, to.label),
        RelationType::Contains => format!("{} contains {}", from.label, to.label),
        _ => format!("{} {} {}", from.label, relation.as_str(), to.label),
    }
}

fn graph_paths_to_hits(paths: &[GraphPathHit]) -> Vec<MemoryHit> {
    let now = crate::conductor::scorer::unix_now();
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| MemoryHit {
            id: format!("graph-path-{index}"),
            source: "code_review_graph".to_string(),
            content: path.summary.clone(),
            rank: path.score as f64,
            hit_type: HitType::KnowledgeGraph,
            importance: path.confidence,
            created_at: now,
            final_score: path.score,
        })
        .collect()
}

// Fact = Insight > KnowledgeGraph > Episodic > TaskTrace
fn sort_hits(hits: &mut [MemoryHit]) {
    hits.sort_by(|a, b| {
        use std::cmp::Ordering;
        let priority = |h: &MemoryHit| match h.hit_type {
            HitType::Fact | HitType::Insight => 0u8,
            HitType::KnowledgeGraph => 1,
            HitType::Episodic => 2,
            HitType::TaskTrace => 3,
        };
        priority(a).cmp(&priority(b)).then_with(|| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(Ordering::Equal)
        })
    });
}

#[cfg(test)]
mod tests {
    use super::{build_memory_plan, detect_memory_intent};
    use crate::conductor::types::{ContextLayers, MemoryIntent, TaskDomain};
    use crate::config::MemoryConfig;

    #[test]
    fn test_context_layers_code_domain() {
        let l = ContextLayers::for_domain(&TaskDomain::Code);
        assert!(l.episodic && l.insights && l.task_trace && l.project);
    }

    #[test]
    fn test_context_layers_shell_domain() {
        let l = ContextLayers::for_domain(&TaskDomain::Shell);
        assert!(!l.episodic && !l.insights && l.task_trace && !l.project);
    }

    #[test]
    fn test_context_layers_general_domain() {
        let l = ContextLayers::for_domain(&TaskDomain::General);
        assert!(l.episodic && l.insights && !l.task_trace && !l.project);
    }

    #[test]
    fn test_detect_memory_intent_prefers_structural_code() {
        let intent =
            detect_memory_intent("which function calls workflow runtime", &TaskDomain::Code);
        assert!(matches!(
            intent,
            MemoryIntent::StructuralCode | MemoryIntent::RelationshipQuery
        ));
    }

    #[test]
    fn test_build_memory_plan_graph_only_skips_semantic_sources() {
        let plan = build_memory_plan(
            &MemoryConfig::default(),
            MemoryIntent::StructuralCode,
            &TaskDomain::Code,
            8,
            None,
        );
        assert!(plan
            .selected_sources
            .iter()
            .any(|source| source == "structural_adapter"));
        assert!(!plan
            .selected_sources
            .iter()
            .any(|source| source == "semantic_memory"));
    }

    #[test]
    fn test_build_memory_plan_user_fact_skips_structural_adapter() {
        let plan = build_memory_plan(
            &MemoryConfig::default(),
            MemoryIntent::UserFact,
            &TaskDomain::General,
            8,
            None,
        );

        assert!(plan
            .selected_sources
            .iter()
            .any(|source| source == "pinned_facts"));
        assert!(!plan
            .selected_sources
            .iter()
            .any(|source| source == "structural_adapter"));
    }
}
