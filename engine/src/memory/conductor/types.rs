use serde::{Deserialize, Serialize};

// Re-export TaskDomain from SDK (moved to shared types)
pub use sdk::TaskDomain;

// ─────────────────────────────────────────────────────────────────────────────
// MemoryKind — typed classification stamped on every episodic memory at ingest
// ─────────────────────────────────────────────────────────────────────────────

/// Semantic type of an episodic memory.
/// Set at ingest time by the extractor (heuristic or LLM).
/// Enables typed/scoped queries without loading all memories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// Explicit user fact: "remember X", "my Y is Z"
    Fact,
    /// User stated preference: "prefer X", "always do Y"
    Preference,
    /// Warning / anti-pattern: "never", "avoid", "don't do"
    Warning,
    /// Architectural decision: "decided to", "going with", "switched to"
    Decision,
    /// Error / panic observed during task
    Error,
    /// Fix applied for an error: "fixed", "resolved", "root cause was"
    Fix,
    /// Agent event / task execution trace
    Trace,
    /// Anything that doesn't match a specific pattern
    #[default]
    General,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryKind::Fact => "fact",
            MemoryKind::Preference => "preference",
            MemoryKind::Warning => "warning",
            MemoryKind::Decision => "decision",
            MemoryKind::Error => "error",
            MemoryKind::Fix => "fix",
            MemoryKind::Trace => "trace",
            MemoryKind::General => "general",
        }
    }

    pub fn from_kind_str(s: &str) -> Self {
        match s {
            "fact" => MemoryKind::Fact,
            "preference" => MemoryKind::Preference,
            "warning" => MemoryKind::Warning,
            "decision" => MemoryKind::Decision,
            "error" => MemoryKind::Error,
            "fix" => MemoryKind::Fix,
            "trace" => MemoryKind::Trace,
            _ => MemoryKind::General,
        }
    }

    /// Returns true for kinds that are always high-value to inject (regardless of domain)
    pub fn always_inject(&self) -> bool {
        matches!(
            self,
            MemoryKind::Fact | MemoryKind::Preference | MemoryKind::Warning
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GraphSourceKind {
    CodeReviewGraph,
    ProjectContext,
    TaskTrace,
    FactStore,
    Episodic,
    Insight,
    LlmInferred,
    #[default]
    Deterministic,
}

impl GraphSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CodeReviewGraph => "code_review_graph",
            Self::ProjectContext => "project_context",
            Self::TaskTrace => "task_trace",
            Self::FactStore => "fact_store",
            Self::Episodic => "episodic",
            Self::Insight => "insight",
            Self::LlmInferred => "llm_inferred",
            Self::Deterministic => "deterministic",
        }
    }

    pub fn deterministic_rank(&self) -> u8 {
        match self {
            Self::CodeReviewGraph
            | Self::ProjectContext
            | Self::TaskTrace
            | Self::FactStore
            | Self::Episodic
            | Self::Insight
            | Self::Deterministic => 0,
            Self::LlmInferred => 1,
        }
    }

    pub fn from_kind_str(value: &str) -> Self {
        match value {
            "code_review_graph" => Self::CodeReviewGraph,
            "project_context" => Self::ProjectContext,
            "task_trace" => Self::TaskTrace,
            "fact_store" => Self::FactStore,
            "episodic" => Self::Episodic,
            "insight" => Self::Insight,
            "llm_inferred" => Self::LlmInferred,
            _ => Self::Deterministic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryIntent {
    StructuralCode,
    UserFact,
    Preference,
    Warning,
    IncidentOrFix,
    RecentContext,
    RelationshipQuery,
    #[default]
    GeneralSemantic,
}

impl MemoryIntent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StructuralCode => "structural_code",
            Self::UserFact => "user_fact",
            Self::Preference => "preference",
            Self::Warning => "warning",
            Self::IncidentOrFix => "incident_or_fix",
            Self::RecentContext => "recent_context",
            Self::RelationshipQuery => "relationship_query",
            Self::GeneralSemantic => "general_semantic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPlan {
    pub intent: MemoryIntent,
    pub selected_sources: Vec<String>,
    pub graph_depth: usize,
    pub facts_budget: usize,
    pub task_trace_budget: usize,
    pub memory_graph_budget: usize,
    pub adapter_budget: usize,
    pub semantic_budget: usize,
    pub fallback_reason: Option<String>,
}

/// A memory node reachable via graph traversal from a seed memory.
///
/// Unlike `GraphPathHit` (which comes from the knowledge/entity graph),
/// `MemoryGraphHit` represents a path through the episodic memory graph —
/// memories connected by shared entities, temporal proximity, or lineage.
/// The `path` field shows the chain of content that led here from the seed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraphHit {
    /// episodic_memory.id of the reached node
    pub id: String,
    /// Truncated content of the memory
    pub content: String,
    /// MemoryKind string of the reached node (warning, fact, decision, …)
    pub memory_kind: String,
    /// Importance score of the reached node
    pub importance: f32,
    /// Domain of the reached node
    pub domain: String,
    pub created_at: i64,
    /// Content snippets of each memory traversed to get here (seed → … → this)
    pub path: Vec<String>,
    /// Edge types traversed in order (shares_entity, temporal, derived_from, …)
    pub path_edge_types: Vec<String>,
    /// How many hops from the nearest seed
    pub depth: usize,
    /// Combined score: importance × decay^depth × edge_weight
    pub graph_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphPathHit {
    pub summary: String,
    pub path: Vec<String>,
    pub source_kinds: Vec<String>,
    pub source_refs: Vec<String>,
    pub confidence: f32,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContextBundle {
    pub plan: MemoryPlan,
    pub facts: Vec<MemoryHit>,
    pub preferences: Vec<MemoryHit>,
    pub warnings: Vec<MemoryHit>,
    pub errors: Vec<MemoryHit>,
    pub graph_paths: Vec<GraphPathHit>,
    /// Memories reached by traversing the memory-to-memory edge graph.
    /// Sorted by graph_score (importance × depth-decay × edge-weight).
    /// Only depth > 0 nodes — seeds are already in the typed/episodic buckets.
    pub memory_graph_hits: Vec<MemoryGraphHit>,
    pub episodic_hits: Vec<MemoryHit>,
    pub insight_hits: Vec<MemoryHit>,
    pub task_trace_hits: Vec<MemoryHit>,
    pub project_context: Option<String>,
}

impl MemoryContextBundle {
    pub fn flattened_hits(&self) -> Vec<MemoryHit> {
        let mut hits = Vec::new();
        hits.extend(self.facts.clone());
        hits.extend(self.preferences.clone());
        hits.extend(self.warnings.clone());
        hits.extend(self.errors.clone());
        hits.extend(self.insight_hits.clone());
        hits.extend(self.episodic_hits.clone());
        hits.extend(self.task_trace_hits.clone());
        for g in &self.memory_graph_hits {
            hits.push(MemoryHit {
                id: g.id.clone(),
                source: "memory_graph".to_string(),
                content: g.content.clone(),
                rank: g.graph_score as f64,
                hit_type: HitType::Episodic,
                importance: g.importance,
                created_at: g.created_at,
                final_score: g.graph_score,
            });
        }
        hits
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ExtractionBackend — controls which backend is used in ingest()
// ─────────────────────────────────────────────────────────────────────────────

/// Which extraction backend to use for memory ingest.
///
/// User-configurable via `memory.extraction_backend` in config.toml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionBackend {
    /// Pure regex/pattern matching — no LLM required.
    /// Fastest, always available, handles ~80% of useful patterns.
    Heuristic,
    /// Use local LLM (Ollama / LocalBrain) only.
    /// Good quality, fully offline.
    Local,
    /// Use cloud LLM only (OpenAI, Anthropic, Gemini).
    /// Best quality, requires API key.
    Cloud,
    /// Try cloud → local → heuristic in order. Use best available.
    /// This is the default for most deployments.
    #[default]
    Auto,
}

// ─────────────────────────────────────────────────────────────────────────────
// ConsolidationBackend — controls which backend runs consolidate()
// ─────────────────────────────────────────────────────────────────────────────

/// Which backend to use for memory consolidation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationBackend {
    /// Use LLM to find cross-cutting patterns.
    /// Best insight quality. Requires LLM.
    Llm,
    /// Entity co-occurrence counting — no LLM.
    /// Finds which entities/topics appear together across memories.
    Heuristic,
    /// Disable consolidation entirely.
    Disabled,
    /// Try LLM, fall back to heuristic if unavailable.
    #[default]
    Auto,
}

// ─────────────────────────────────────────────────────────────────────────────
// 25-domain → memory TaskDomain mapping
// ─────────────────────────────────────────────────────────────────────────────

/// Map a dispatch domain (from the 25-domain classifier) to a memory TaskDomain.
/// The memory system has 5 coarse domains; this bridges the gap.
pub fn dispatch_domain_to_memory_domain(dispatch_domain: &str) -> TaskDomain {
    match dispatch_domain {
        "code" | "testing" | "ml" | "security" => TaskDomain::Code,
        "git" => TaskDomain::Git,
        "shell" | "devops" | "infra" => TaskDomain::Shell,
        "database" | "data" | "api" => TaskDomain::Data,
        "browser" | "search" => TaskDomain::Browser,
        _ => TaskDomain::General,
    }
}

/// Available step execution status indicators
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepResult {
    Success,
    Failed(String),
}

/// The local type of step that the conductor is executing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StepType {
    #[default]
    Execute,
    Research,
    Verify,
}

/// Specialist role assigned to a plan step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StepRole {
    Researcher,
    #[default]
    Executor,
    Verifier,
}

impl StepRole {
    pub fn for_step_type(step_type: &StepType) -> Self {
        match step_type {
            StepType::Research => Self::Researcher,
            StepType::Verify => Self::Verifier,
            StepType::Execute => Self::Executor,
        }
    }
}

/// Routing constraint for a DAG node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RoutePolicy {
    LocalOnly,
    LocalPreferred,
    CloudOnly,
    #[default]
    Inherit,
}

/// Represents an individual step within a task plan
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub order: u32,
    pub step_type: StepType,
    #[serde(default)]
    pub role: StepRole,
    #[serde(default)]
    pub parallel_safe: bool,
    #[serde(default)]
    pub route_policy: RoutePolicy,
    pub dependencies: Vec<String>,
    pub description: String,
    pub expected_outcome: String,
}

/// Describes the environment routing behavior for a task chunk
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionMode {
    #[default]
    Direct,
    Reflective,
    DeepResearch,
}

/// Stores stage information limits and tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub description: String,
    pub tools_allowed: Vec<String>,
}

/// Structure modeling the layout generated by the planner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConductorPlan {
    pub id: String,
    pub original_goal: String,
    pub mode: ExecutionMode,
    pub stages: Vec<Stage>,
    pub steps: Vec<PlanStep>,
    pub created_at: i64,
}

/// Budgets representing token amounts per memory layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBudget {
    pub session_tokens: usize,
    pub system_tokens: usize,
    pub episodic_tokens: usize,
    pub project_tokens: usize,
}

impl Default for MemoryBudget {
    fn default() -> Self {
        Self {
            session_tokens: 8000,
            system_tokens: 4000,
            episodic_tokens: 16000,
            project_tokens: 32000,
        }
    }
}

/// Determines which memory layers are active for a given task domain.
#[derive(Debug, Clone, Copy)]
pub struct ContextLayers {
    pub episodic: bool,
    pub insights: bool,
    pub task_trace: bool,
    pub project: bool,
    pub knowledge_graph: bool,
}

impl ContextLayers {
    pub fn for_domain(domain: &TaskDomain) -> Self {
        match domain {
            TaskDomain::Code => Self {
                episodic: true,
                insights: true,
                task_trace: true,
                project: true,
                knowledge_graph: true,
            },
            TaskDomain::Git => Self {
                episodic: true,
                insights: true,
                task_trace: true,
                project: false,
                knowledge_graph: true,
            },
            TaskDomain::Shell => Self {
                episodic: false,
                insights: false,
                task_trace: true,
                project: false,
                knowledge_graph: false,
            },
            TaskDomain::General => Self {
                episodic: true,
                insights: true,
                task_trace: false,
                project: false,
                knowledge_graph: true,
            },
            TaskDomain::Browser | TaskDomain::Data => Self {
                episodic: true,
                insights: false,
                task_trace: false,
                project: false,
                knowledge_graph: false,
            },
        }
    }
}

/// Type of memory hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HitType {
    Episodic,
    Insight,
    KnowledgeGraph,
    TaskTrace,
    /// Structured fact from the memory_facts key-value store.
    Fact,
}

/// Result of a memory query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub id: String,
    pub source: String,
    pub content: String,
    pub rank: f64,
    pub hit_type: HitType,
    pub importance: f32,
    pub created_at: i64,
    pub final_score: f32,
}

/// Result of an ingest operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestResult {
    pub memory_id: String,
    pub summary: String,
    pub entities: Vec<String>,
    pub topics: Vec<String>,
    pub importance: f64,
    /// Semantic kind — set at ingest time, used for typed queries.
    pub kind: MemoryKind,
}

/// Result of a consolidation operation.
#[derive(Debug, Clone)]
pub enum ConsolidationResult {
    Skipped {
        reason: String,
    },
    Completed {
        memories_processed: usize,
        insights_generated: usize,
    },
}

/// Structured extraction from the ingest prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct IngestExtraction {
    pub summary: String,
    pub entities: Vec<String>,
    pub topics: Vec<String>,
    pub importance: f64,
}

/// Structured insight from the consolidation prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConsolidationInsight {
    pub insight: String,
    pub source_ids: Vec<String>,
}
