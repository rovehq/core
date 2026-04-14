use serde::{Deserialize, Serialize};

use crate::conductor::types::{ConsolidationBackend, ExtractionBackend};

use super::defaults::{
    default_consolidation_interval_mins, default_episodic_retention_days,
    default_max_session_tokens, default_min_importance_to_inject, default_min_to_consolidate,
    default_query_limit, default_true,
};

fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    #[default]
    GraphOnly,
    AlwaysOn,
}

impl MemoryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GraphOnly => "graph_only",
            Self::AlwaysOn => "always_on",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryGraphEnrichment {
    #[default]
    Deterministic,
    DeterministicPlusLlm,
}

impl MemoryGraphEnrichment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::DeterministicPlusLlm => "deterministic_plus_llm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBundleStrategy {
    #[default]
    Adaptive,
}

impl MemoryBundleStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRetrievalAssist {
    #[default]
    Off,
    Rerank,
    Compress,
}

impl MemoryRetrievalAssist {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Rerank => "rerank",
            Self::Compress => "compress",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAdapterMode {
    Off,
    #[default]
    Auto,
    Required,
}

impl MemoryAdapterMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Required => "required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    #[default]
    PerNode,
}

impl MemoryScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PerNode => "per_node",
        }
    }
}

/// Memory system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Top-level runtime memory mode.
    #[serde(default)]
    pub mode: MemoryMode,
    /// Final context packing strategy.
    #[serde(default)]
    pub bundle_strategy: MemoryBundleStrategy,
    /// Optional retrieval assistance after deterministic planning.
    #[serde(default)]
    pub retrieval_assist: MemoryRetrievalAssist,
    /// Optional LLM enrichment policy for the graph layer.
    #[serde(default)]
    pub graph_enrichment: MemoryGraphEnrichment,
    /// Scope for persisted memory/graph state.
    #[serde(default)]
    pub scope: MemoryScope,
    /// Require a usable code-review-graph for structural code retrieval.
    #[serde(default = "default_true")]
    pub code_graph_required: bool,
    /// Optional structural adapter mode for code/workspace retrieval.
    #[serde(default)]
    pub code_adapter_mode: MemoryAdapterMode,
    /// Maximum tokens for short-term session memory.
    #[serde(default = "default_max_session_tokens")]
    pub max_session_tokens: usize,
    /// Default number of days to keep episodic memories if active.
    #[serde(default = "default_episodic_retention_days")]
    pub episodic_retention_days: u32,
    /// Consolidation interval in minutes.
    #[serde(default = "default_consolidation_interval_mins")]
    pub consolidation_interval_mins: u64,
    /// Minimum memories required to trigger consolidation.
    #[serde(default = "default_min_to_consolidate")]
    pub min_to_consolidate: usize,
    /// Maximum results returned by query.
    #[serde(default = "default_query_limit")]
    pub query_limit: u32,
    /// Minimum importance threshold for injection.
    #[serde(default = "default_min_importance_to_inject")]
    pub min_importance_to_inject: f32,
    /// Enable automatic importance decay.
    #[serde(default = "default_true")]
    pub importance_decay_enabled: bool,

    // ── Backend selection ──────────────────────────────────────────────────
    /// Which backend extracts summary/entities/topics/importance at ingest time.
    ///
    /// - `heuristic` — no LLM, regex patterns, always available
    /// - `local`     — local LLM only (Ollama/LocalBrain)
    /// - `cloud`     — cloud LLM only (OpenAI/Anthropic/Gemini)
    /// - `auto`      — best available: cloud → local → heuristic (default)
    #[serde(default)]
    pub extraction_backend: ExtractionBackend,

    /// Which backend runs background consolidation.
    ///
    /// - `llm`       — LLM finds cross-cutting patterns
    /// - `heuristic` — entity co-occurrence counting, no LLM
    /// - `disabled`  — skip consolidation entirely
    /// - `auto`      — try LLM, fall back to heuristic (default)
    #[serde(default)]
    pub consolidation_backend: ConsolidationBackend,

    /// Generate vector embeddings immediately at ingest time (not on backfill).
    /// Requires LocalBrain. Enables fresh hybrid search for new memories.
    #[serde(default = "default_false")]
    pub embed_at_ingest: bool,

    /// Enable the structured fact store (`memory_facts` table).
    /// When enabled, "remember X" and "my Y is Z" patterns are also written
    /// to a fast key-value table that is always injected first, never decayed.
    #[serde(default = "default_true")]
    pub fact_store_enabled: bool,

    /// Persist explicit/pinned facts even when `mode = graph_only`.
    #[serde(default = "default_true")]
    pub persist_pinned_facts: bool,

    /// Keep compact task traces available even when `mode = graph_only`.
    #[serde(default = "default_true")]
    pub persist_task_traces: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            mode: MemoryMode::GraphOnly,
            bundle_strategy: MemoryBundleStrategy::Adaptive,
            retrieval_assist: MemoryRetrievalAssist::Off,
            graph_enrichment: MemoryGraphEnrichment::Deterministic,
            scope: MemoryScope::PerNode,
            code_graph_required: true,
            code_adapter_mode: MemoryAdapterMode::Auto,
            max_session_tokens: default_max_session_tokens(),
            episodic_retention_days: default_episodic_retention_days(),
            consolidation_interval_mins: default_consolidation_interval_mins(),
            min_to_consolidate: default_min_to_consolidate(),
            query_limit: default_query_limit(),
            min_importance_to_inject: default_min_importance_to_inject(),
            importance_decay_enabled: default_true(),
            extraction_backend: ExtractionBackend::Auto,
            consolidation_backend: ConsolidationBackend::Auto,
            embed_at_ingest: false,
            fact_store_enabled: true,
            persist_pinned_facts: true,
            persist_task_traces: true,
        }
    }
}

impl MemoryConfig {
    pub fn always_on_enabled(&self) -> bool {
        matches!(self.mode, MemoryMode::AlwaysOn)
    }

    pub fn should_query_code_adapter(&self) -> bool {
        !matches!(self.code_adapter_mode, MemoryAdapterMode::Off)
    }

    pub fn should_persist_pinned_facts(&self) -> bool {
        self.fact_store_enabled && (self.always_on_enabled() || self.persist_pinned_facts)
    }

    pub fn should_query_task_traces(&self) -> bool {
        self.always_on_enabled() || self.persist_task_traces
    }
}
