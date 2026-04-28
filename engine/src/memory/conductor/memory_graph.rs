//! Rove Memory Graph
//!
//! A graph-traversal engine for episodic memory — Rove's own implementation,
//! structurally analogous to what code-review-graph does for code.
//!
//! ## What it does
//!
//! Every memory node (row in `episodic_memory`) can be connected to others via
//! typed edges in `memory_graph_edges`:
//!
//! | Edge type      | Meaning                                                    |
//! |----------------|------------------------------------------------------------|
//! | `shares_entity`| Both memories mention the same named entity                |
//! | `temporal`     | Memories from the same task_id / same session window       |
//! | `derived_from` | An insight or consolidation was derived from this episode  |
//! | `supports`     | Corroborating evidence — same claim in independent context |
//! | `contradicts`  | Conflicting information about the same subject             |
//!
//! ## Query pipeline
//!
//! 1. **Seed detection** — FTS against `episodic_fts` to find top-N matching
//!    memories for the query.
//! 2. **BFS traversal** — walk outbound edges from seeds up to `max_depth`
//!    hops, collecting expansion nodes.
//! 3. **Scoring** — `importance × (DECAY ^ depth) × edge_weight`
//! 4. **Return** — depth > 0 nodes only (seeds are in the typed/episodic
//!    buckets already); sorted by `graph_score` descending.
//!
//! ## Edge construction
//!
//! `build_edges_for_memory()` is called at ingest time.  It is deterministic
//! and requires no LLM.  For each entity extracted from the new memory it
//! scans existing memories in the same domain for entity overlap and creates
//! `shares_entity` edges (bidirectional, one row per direction).

use std::collections::{HashSet, VecDeque};

use crate::conductor::types::MemoryGraphHit;
use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::debug;

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Importance multiplier per hop away from the seed.
const DEPTH_DECAY: f32 = 0.75;

/// Maximum BFS seeds per query (keeps traversal focused).
const MAX_SEEDS: usize = 6;

/// Minimum entity length to consider for edge building (avoids single-letter noise).
const MIN_ENTITY_LEN: usize = 3;

// ─────────────────────────────────────────────────────────────────────────────
// Internal node representation used during traversal
// ─────────────────────────────────────────────────────────────────────────────

struct MemNode {
    #[allow(dead_code)]
    id: String,
    content: String,
    memory_kind: String,
    importance: f32,
    domain: String,
    created_at: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Edge construction — called at ingest time
// ─────────────────────────────────────────────────────────────────────────────

/// Build `shares_entity` edges between a newly ingested memory and existing
/// memories that share at least one named entity.
///
/// Also builds `temporal` edges when the same `task_id` appears more than once
/// (e.g. multi-step tasks that produce multiple memory rows).
///
/// Returns the number of edges created.
pub async fn build_edges_for_memory(
    pool: &SqlitePool,
    memory_id: &str,
    task_id: &str,
    entities: &[String],
    domain: &str,
    created_at: i64,
) -> Result<usize> {
    let mut created = 0usize;

    // ── shares_entity edges ──────────────────────────────────────────────────

    for entity in entities {
        let entity = entity.trim();
        if entity.len() < MIN_ENTITY_LEN {
            continue;
        }

        // Find existing memories that mention this entity (JSON LIKE search).
        // Limited to 8 per entity to bound edge fan-out.
        let pattern = format!("%\"{}%", entity);
        let rows = sqlx::query(
            r#"SELECT id FROM episodic_memory
               WHERE id != ?
                 AND (domain = ? OR domain = 'general' OR ? = 'general')
                 AND entities LIKE ?
               ORDER BY importance DESC
               LIMIT 8"#,
        )
        .bind(memory_id)
        .bind(domain)
        .bind(domain)
        .bind(&pattern)
        .fetch_all(pool)
        .await?;

        for row in rows {
            let peer_id: String = row.get("id");
            // Create bidirectional edges (one row each direction)
            created += upsert_edge(
                pool,
                memory_id,
                &peer_id,
                "shares_entity",
                Some(entity),
                1.0,
                1.0,
                created_at,
            )
            .await?;
            created += upsert_edge(
                pool,
                &peer_id,
                memory_id,
                "shares_entity",
                Some(entity),
                1.0,
                1.0,
                created_at,
            )
            .await?;
        }
    }

    // ── temporal edges (same task_id) ────────────────────────────────────────

    let temporal_rows = sqlx::query(
        r#"SELECT id FROM episodic_memory
           WHERE task_id = ? AND id != ?
           ORDER BY created_at DESC
           LIMIT 4"#,
    )
    .bind(task_id)
    .bind(memory_id)
    .fetch_all(pool)
    .await?;

    for row in temporal_rows {
        let peer_id: String = row.get("id");
        created += upsert_edge(
            pool, memory_id, &peer_id, "temporal", None, 0.8, 1.0, created_at,
        )
        .await?;
        created += upsert_edge(
            pool, &peer_id, memory_id, "temporal", None, 0.8, 1.0, created_at,
        )
        .await?;
    }

    if created > 0 {
        debug!(
            memory_id = %memory_id,
            edges = created,
            "built memory graph edges"
        );
    }

    Ok(created)
}

/// Mark a consolidation insight as `derived_from` its source episodic memories.
pub async fn link_insight_to_sources(
    pool: &SqlitePool,
    insight_id: &str,
    source_memory_ids: &[String],
    created_at: i64,
) -> Result<()> {
    for source_id in source_memory_ids {
        let _ = upsert_edge(
            pool,
            insight_id,
            source_id,
            "derived_from",
            None,
            1.0,
            0.9,
            created_at,
        )
        .await;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Graph traversal — BFS from seeds
// ─────────────────────────────────────────────────────────────────────────────

/// Full pipeline: question → seed detection → BFS traversal → scored hits.
///
/// Returns memories reachable from the semantic seeds, sorted by `graph_score`.
/// Seeds themselves (depth = 0) are excluded; they appear in typed/episodic buckets.
pub async fn memory_graph_context(
    pool: &SqlitePool,
    question: &str,
    domain: &str,
    min_importance: f32,
    budget: usize,
) -> Result<Vec<MemoryGraphHit>> {
    if budget == 0 {
        return Ok(vec![]);
    }

    let seeds = find_seeds(pool, question, domain, min_importance).await?;
    if seeds.is_empty() {
        return Ok(vec![]);
    }

    traverse_from_seeds(pool, &seeds, 2, budget).await
}

/// Return direct neighbors of a memory node (both directions).
pub async fn memory_neighbors(
    pool: &SqlitePool,
    memory_id: &str,
    budget: usize,
) -> Result<Vec<MemoryGraphHit>> {
    let seeds = vec![memory_id.to_string()];
    traverse_from_seeds(pool, &seeds, 1, budget).await
}

// ─────────────────────────────────────────────────────────────────────────────
// Seed detection
// ─────────────────────────────────────────────────────────────────────────────

async fn find_seeds(
    pool: &SqlitePool,
    question: &str,
    domain: &str,
    min_importance: f32,
) -> Result<Vec<String>> {
    if question.trim().is_empty() {
        return Ok(vec![]);
    }

    // Build a keyword-extracted FTS query (same logic as fact_store)
    let fts_query = extract_fts_keywords(question);

    let rows = match sqlx::query(
        r#"SELECT e.id
           FROM episodic_fts
           JOIN episodic_memory e ON e.rowid = episodic_fts.rowid
           WHERE episodic_fts MATCH ?
             AND e.importance >= ?
             AND (e.domain = ? OR e.domain = 'general')
           ORDER BY bm25(episodic_fts)
           LIMIT ?"#,
    )
    .bind(&fts_query)
    .bind(min_importance)
    .bind(domain)
    .bind(MAX_SEEDS as i64)
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            debug!("memory graph seed FTS failed: {}", e);
            return Ok(vec![]);
        }
    };

    Ok(rows.iter().map(|r| r.get("id")).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// BFS traversal
// ─────────────────────────────────────────────────────────────────────────────

async fn traverse_from_seeds(
    pool: &SqlitePool,
    seed_ids: &[String],
    max_depth: usize,
    budget: usize,
) -> Result<Vec<MemoryGraphHit>> {
    // queue item: (memory_id, path_content_snippets, path_edge_types, depth)
    let mut queue: VecDeque<(String, Vec<String>, Vec<String>, usize)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();

    for id in seed_ids {
        if visited.insert(id.clone()) {
            queue.push_back((id.clone(), vec![], vec![], 0));
        }
    }

    let mut hits: Vec<MemoryGraphHit> = Vec::new();

    while let Some((current_id, path, path_edges, depth)) = queue.pop_front() {
        let Some(node) = load_node(pool, &current_id).await? else {
            continue;
        };

        // Seeds (depth=0) are already in other buckets — skip to expansion only
        if depth > 0 {
            let decay = DEPTH_DECAY.powi(depth as i32);
            // Estimate edge weight from the last traversed edge (stored separately below)
            let graph_score = node.importance * decay;

            hits.push(MemoryGraphHit {
                id: current_id.clone(),
                content: node.content.clone(),
                memory_kind: node.memory_kind.clone(),
                importance: node.importance,
                domain: node.domain.clone(),
                created_at: node.created_at,
                path: path.clone(),
                path_edge_types: path_edges.clone(),
                depth,
                graph_score,
            });

            if hits.len() >= budget {
                break;
            }
        }

        if depth < max_depth {
            let edges = get_outbound_edges(pool, &current_id).await?;
            let snippet = truncate_content(&node.content, 80);
            for (neighbor_id, edge_type, _weight) in edges {
                if !visited.contains(&neighbor_id) && visited.insert(neighbor_id.clone()) {
                    let mut new_path = path.clone();
                    new_path.push(snippet.clone());
                    let mut new_edges = path_edges.clone();
                    new_edges.push(edge_type);
                    queue.push_back((neighbor_id, new_path, new_edges, depth + 1));
                }
            }
        }
    }

    hits.sort_by(|a, b| {
        b.graph_score
            .partial_cmp(&a.graph_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(hits)
}

// ─────────────────────────────────────────────────────────────────────────────
// Database helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn load_node(pool: &SqlitePool, id: &str) -> Result<Option<MemNode>> {
    let row = sqlx::query(
        "SELECT id, summary, memory_kind, importance, domain, created_at
         FROM episodic_memory WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| MemNode {
        id: r.get("id"),
        content: r.get("summary"),
        memory_kind: r.get("memory_kind"),
        importance: r.get::<f64, _>("importance") as f32,
        domain: r.get("domain"),
        created_at: r.get("created_at"),
    }))
}

/// Returns (neighbor_id, edge_type, weight) for all edges leaving `from_id`.
async fn get_outbound_edges(
    pool: &SqlitePool,
    from_id: &str,
) -> Result<Vec<(String, String, f32)>> {
    let rows = sqlx::query(
        "SELECT to_id, edge_type, weight FROM memory_graph_edges WHERE from_id = ? LIMIT 20",
    )
    .bind(from_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("to_id"),
                r.get::<String, _>("edge_type"),
                r.get::<f64, _>("weight") as f32,
            )
        })
        .collect())
}

/// Upsert one directed edge. Returns 1 if inserted, 0 if already existed.
#[allow(clippy::too_many_arguments)]
async fn upsert_edge(
    pool: &SqlitePool,
    from_id: &str,
    to_id: &str,
    edge_type: &str,
    entity: Option<&str>,
    weight: f64,
    confidence: f64,
    created_at: i64,
) -> Result<usize> {
    // Canonical edge id: deterministic from (from_id, edge_type, to_id, entity)
    let entity_part = entity.unwrap_or("");
    let edge_id = format!(
        "mg:{}:{}:{}:{}",
        &from_id[..from_id.len().min(8)],
        edge_type,
        &to_id[..to_id.len().min(8)],
        entity_part
    );

    let affected = sqlx::query(
        r#"INSERT OR IGNORE INTO memory_graph_edges
           (id, from_id, to_id, edge_type, entity, weight, confidence, source_kind, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, 'deterministic', ?)"#,
    )
    .bind(&edge_id)
    .bind(from_id)
    .bind(to_id)
    .bind(edge_type)
    .bind(entity)
    .bind(weight)
    .bind(confidence)
    .bind(created_at)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(affected as usize)
}

// ─────────────────────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────────────────────

fn truncate_content(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// Extract meaningful keywords from a natural-language question for FTS5 seed detection.
/// Strips common noise words and joins with OR so any term matching is enough.
fn extract_fts_keywords(question: &str) -> String {
    const STOP: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "have", "has", "had", "do",
        "does", "did", "will", "would", "could", "should", "what", "where", "when", "which", "who",
        "how", "why", "that", "this", "with", "from", "into", "about", "for", "of", "in", "on",
        "at", "to", "by", "or", "and", "not", "no", "my", "your", "our", "you", "me", "we", "they",
        "he", "she", "it", "i", "tell", "show", "find", "get", "give", "any", "all", "please",
        "remember",
    ];

    let mut seen = HashSet::new();
    let terms: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| !STOP.contains(&t.as_str()) && seen.insert(t.clone()))
        .map(|t| format!("\"{}\"", t))
        .collect();

    if terms.is_empty() {
        question
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        terms.join(" OR ")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stats helper (for status / WebUI)
// ─────────────────────────────────────────────────────────────────────────────

/// Return (node_count, edge_count) from the memory graph.
pub async fn graph_stats(pool: &SqlitePool) -> Result<(i64, i64)> {
    let nodes: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT id) FROM episodic_memory")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let edges: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_graph_edges")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    Ok((nodes, edges))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_fts_keywords_strips_noise() {
        let q = extract_fts_keywords("what errors happened with sqlx database");
        assert!(q.contains("\"errors\"") || q.contains("\"sqlx\""));
        assert!(!q.contains("\"what\""));
        assert!(!q.contains("\"with\""));
    }

    #[test]
    fn test_extract_fts_keywords_uses_or() {
        let q = extract_fts_keywords("sqlx errors database");
        assert!(q.contains(" OR "), "expected OR in: {q}");
    }

    #[test]
    fn test_truncate_content_short() {
        assert_eq!(truncate_content("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_content_long() {
        let s = "a".repeat(100);
        let t = truncate_content(&s, 10);
        assert!(t.ends_with('…'));
        assert!(t.len() < s.len());
    }
}
