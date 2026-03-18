//! Memory System
//!
//! Implements the three memory operations from the Rove architecture:
//!
//! - `ingest()`  — after each task, LLM extracts summary/entities/topics/importance
//!   and stores into `episodic_memory`
//! - `consolidate()` — processes unconsolidated memories (≥3), finds cross-cutting
//!   connections, stores insights in `consolidation_insights`
//! - `query()` — FTS5 BM25 search across episodic_fts + insights_fts
//!
//! Ported from always-on-memory-agent (MIT): Python → Rust,
//! Google ADK → our LLM router, aiohttp → reqwest/tokio.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{debug, error, info};

use crate::conductor::types::{ConsolidationResult, IngestResult, MemoryHit, TaskDomain};
use crate::config::MemoryConfig;
use crate::llm::router::LLMRouter;

// ─────────────────────────────────────────────────────────────────────
// MemorySystem
// ─────────────────────────────────────────────────────────────────────

/// Core memory system that manages ingest, consolidation, and query.
///
/// Thread-safe (Arc-wrapped internally) and designed to be shared across
/// the agent core and the background consolidation loop.
pub struct MemorySystem {
    pool: SqlitePool,
    router: Arc<LLMRouter>,
    knowledge_graph: Arc<crate::knowledge_graph::KnowledgeGraph>,
    entity_extractor: Arc<crate::knowledge_graph::EntityExtractor>,
    embedding_generator: Option<Arc<crate::conductor::EmbeddingGenerator>>,
    config: MemoryConfig,
}

impl MemorySystem {
    /// Create a new MemorySystem.
    ///
    /// # Arguments
    /// * `pool` – SQLite connection pool (must have migration 003 applied)
    /// * `router` – LLM router for extraction and consolidation calls
    pub fn new(pool: SqlitePool, router: Arc<LLMRouter>) -> Self {
        Self::new_with_config(pool, router, MemoryConfig::default())
    }

    pub fn new_with_config(pool: SqlitePool, router: Arc<LLMRouter>, config: MemoryConfig) -> Self {
        let knowledge_graph = Arc::new(crate::knowledge_graph::KnowledgeGraph::new(pool.clone()));
        let entity_extractor =
            Arc::new(crate::knowledge_graph::EntityExtractor::new(router.clone()));

        Self {
            pool,
            router,
            knowledge_graph,
            entity_extractor,
            embedding_generator: None,
            config,
        }
    }

    /// Set the embedding generator (optional, requires LocalBrain)
    pub fn with_embedding_generator(
        mut self,
        generator: Arc<crate::conductor::EmbeddingGenerator>,
    ) -> Self {
        self.embedding_generator = Some(generator);
        self
    }

    // ─────────────────────────────────────────────────────────────────
    // ingest()
    // ─────────────────────────────────────────────────────────────────

    /// Ingest a completed task into episodic memory.
    ///
    /// Delegates to the episodic module for implementation.
    pub async fn ingest(
        &self,
        task_input: &str,
        task_result: &str,
        task_id: &str,
        domain: &TaskDomain,
        sensitive: bool,
    ) -> Result<IngestResult> {
        crate::conductor::episodic::ingest(
            &self.pool,
            &self.router,
            &self.knowledge_graph,
            &self.entity_extractor,
            task_input,
            task_result,
            task_id,
            domain,
            sensitive,
        )
        .await
    }

    // ─────────────────────────────────────────────────────────────────
    // consolidate()
    // ─────────────────────────────────────────────────────────────────

    /// Consolidate unconsolidated episodic memories into cross-cutting insights.
    ///
    /// Delegates to the consolidation module for implementation.
    /// Calls decay_importance() after consolidation completes.
    pub async fn consolidate(&self) -> Result<ConsolidationResult> {
        let result = crate::conductor::consolidation::consolidate(
            &self.pool,
            &self.router,
            self.config.min_to_consolidate,
        )
        .await?;

        // Apply importance decay after consolidation
        crate::conductor::decay::decay_importance(&self.pool, self.config.importance_decay_enabled)
            .await?;

        Ok(result)
    }

    // ─────────────────────────────────────────────────────────────────
    // query()
    // ─────────────────────────────────────────────────────────────────

    /// Query episodic memory and consolidation insights using domain-gated layers.
    ///
    /// Delegates to the query module for implementation.
    pub async fn query(
        &self,
        question: &str,
        domain: &TaskDomain,
        team_id: Option<&str>,
    ) -> Result<Vec<MemoryHit>> {
        let mut hits = crate::conductor::query::query(
            &self.pool,
            question,
            domain,
            team_id,
            self.config.query_limit as usize,
            self.config.min_importance_to_inject,
        )
        .await?;

        self.append_graph_hits(question, &mut hits, self.config.query_limit as usize)
            .await?;

        Ok(hits)
    }

    /// Query episodic memory using hybrid search (BM25 + cosine similarity).
    ///
    /// Delegates to the query module for implementation.
    pub async fn query_hybrid(
        &self,
        question: &str,
        domain: &TaskDomain,
        team_id: Option<&str>,
    ) -> Result<Vec<MemoryHit>> {
        let mut hits = crate::conductor::query::query_hybrid(
            &self.pool,
            self.embedding_generator.as_ref(),
            question,
            domain,
            team_id,
            self.config.query_limit as usize,
            self.config.min_importance_to_inject,
        )
        .await?;

        self.append_graph_hits(question, &mut hits, self.config.query_limit as usize)
            .await?;

        Ok(hits)
    }

    // ─────────────────────────────────────────────────────────────────
    // decay_importance()
    // ─────────────────────────────────────────────────────────────────

    /// Decay importance for unused memories and prune fully decayed entries.
    ///
    /// Delegates to the decay module for implementation.
    pub async fn decay_importance(&self, enabled: bool) -> Result<()> {
        crate::conductor::decay::decay_importance(&self.pool, enabled).await
    }

    async fn append_graph_hits(
        &self,
        question: &str,
        hits: &mut Vec<MemoryHit>,
        query_limit: usize,
    ) -> Result<()> {
        let graph_query =
            crate::knowledge_graph::GraphQuery::new(self.knowledge_graph.as_ref().clone());
        let graph_facts = graph_query.related_facts(question, query_limit).await?;
        let now = crate::conductor::scorer::unix_now();

        for (index, fact) in graph_facts.into_iter().enumerate() {
            if hits.iter().any(|hit| hit.content == fact) {
                continue;
            }

            hits.push(MemoryHit {
                id: format!("graph-fact-{}", index),
                source: "graph".to_string(),
                content: fact,
                rank: 0.0,
                hit_type: crate::conductor::types::HitType::KnowledgeGraph,
                importance: 0.9,
                created_at: now,
                final_score: 0.95 - (index as f32 * 0.01),
            });
        }

        sort_hits(hits);
        hits.truncate(query_limit);
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────
    // Background consolidation loop
    // ─────────────────────────────────────────────────────────────────

    /// Start the background consolidation loop.
    ///
    /// Runs `consolidate()` every `interval` and **never panics** — all
    /// errors are logged and the loop continues. Designed to be spawned
    /// via `tokio::spawn`.
    pub async fn start_consolidation_loop(self: Arc<Self>, interval: Duration) {
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
                        "Consolidation cycle: {} memories → {} insights",
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

fn sort_hits(hits: &mut [MemoryHit]) {
    hits.sort_by(|a, b| {
        use std::cmp::Ordering;

        let priority = |hit: &MemoryHit| match hit.hit_type {
            crate::conductor::types::HitType::Insight => 0_u8,
            crate::conductor::types::HitType::KnowledgeGraph => 1,
            crate::conductor::types::HitType::Episodic => 2,
            crate::conductor::types::HitType::TaskTrace => 3,
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
    use crate::conductor::types::{ContextLayers, TaskDomain};

    #[test]
    fn test_context_layers_code_domain() {
        let layers = ContextLayers::for_domain(&TaskDomain::Code);
        assert!(layers.episodic);
        assert!(layers.insights);
        assert!(layers.task_trace);
        assert!(layers.project);
    }

    #[test]
    fn test_context_layers_git_domain() {
        let layers = ContextLayers::for_domain(&TaskDomain::Git);
        assert!(layers.episodic);
        assert!(layers.insights);
        assert!(layers.task_trace);
        assert!(!layers.project);
    }

    #[test]
    fn test_context_layers_shell_domain() {
        let layers = ContextLayers::for_domain(&TaskDomain::Shell);
        assert!(!layers.episodic);
        assert!(!layers.insights);
        assert!(layers.task_trace);
        assert!(!layers.project);
    }

    #[test]
    fn test_context_layers_general_domain() {
        let layers = ContextLayers::for_domain(&TaskDomain::General);
        assert!(layers.episodic);
        assert!(layers.insights);
        assert!(!layers.task_trace);
        assert!(!layers.project);
    }

    #[test]
    fn test_context_layers_browser_domain() {
        let layers = ContextLayers::for_domain(&TaskDomain::Browser);
        assert!(layers.episodic);
        assert!(!layers.insights);
        assert!(!layers.task_trace);
        assert!(!layers.project);
    }

    #[test]
    fn test_context_layers_data_domain() {
        let layers = ContextLayers::for_domain(&TaskDomain::Data);
        assert!(layers.episodic);
        assert!(!layers.insights);
        assert!(!layers.task_trace);
        assert!(!layers.project);
    }
}
