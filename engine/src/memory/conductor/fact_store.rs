//! Structured Fact Store
//!
//! The fact store is a lightweight key-value table (`memory_facts`) for
//! high-confidence structured facts extracted at ingest time — things like
//! user properties ("my linter is clippy"), workspace paths, and explicit
//! "remember that …" statements.
//!
//! Characteristics:
//!   - **Never decayed** — facts persist until explicitly deleted
//!   - **Always injected first** — facts appear before episodic results
//!   - **O(1) key lookup** — direct lookup via TEXT PRIMARY KEY
//!   - **FTS5-backed search** — fuzzy retrieval via `memory_facts_fts`
//!
//! The `upsert_fact()` function de-duplicates at write time:
//!   - User-property keys (e.g. "preferred_linter") → replace old value
//!   - "remembered_fact" keys → hash the value to make a unique key per fact

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use tracing::debug;

use crate::conductor::types::{HitType, MemoryHit};
use crate::storage::{record_fact_version_by_key, MemoryMutationAction};

// ─────────────────────────────────────────────────────────────────────────────
// FactRow — a single row from memory_facts
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FactRow {
    pub key: String,
    pub value: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// upsert_fact — write or update a structured fact
// ─────────────────────────────────────────────────────────────────────────────

/// Upsert a fact into the structured fact store.
///
/// For "remembered_fact" keys, a stable hash of the value is used as a
/// unique key suffix so multiple remembered facts can coexist.
/// For all other keys the key is the primary key — a new value replaces the old.
pub async fn upsert_fact(
    pool: &SqlitePool,
    key: &str,
    value: &str,
    task_id: &str,
    memory_id: &str,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    // Make "remembered_fact" entries unique by hashing the content
    let canonical_key = if key == "remembered_fact" {
        let mut hasher = DefaultHasher::new();
        value.to_ascii_lowercase().hash(&mut hasher);
        format!("remembered_fact:{:016x}", hasher.finish())
    } else {
        key.to_string()
    };

    sqlx::query(
        r#"INSERT INTO memory_facts (key, value, task_id, memory_id, created_at, updated_at)
           VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT(key) DO UPDATE SET
             value      = excluded.value,
             task_id    = excluded.task_id,
             memory_id  = excluded.memory_id,
             updated_at = excluded.updated_at"#,
    )
    .bind(&canonical_key)
    .bind(value)
    .bind(task_id)
    .bind(memory_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    let action = if sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM memory_versions WHERE entity_kind = 'fact' AND entity_id = ?",
    )
    .bind(&canonical_key)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
        == 0
    {
        MemoryMutationAction::Create
    } else {
        MemoryMutationAction::Update
    };
    let _ = record_fact_version_by_key(pool, &canonical_key, action, "memory_ingest").await;

    debug!(key = %canonical_key, "upserted memory fact");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// get_all_facts — retrieve every stored fact (for context injection)
// ─────────────────────────────────────────────────────────────────────────────

/// Return all stored facts ordered by last updated descending.
pub async fn get_all_facts(pool: &SqlitePool) -> Result<Vec<FactRow>> {
    let rows = sqlx::query(
        r#"SELECT key, value, created_at, updated_at
           FROM memory_facts
           ORDER BY updated_at DESC"#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| FactRow {
            key: r.get("key"),
            value: r.get("value"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// query_from_fact_store — FTS search over stored facts
// ─────────────────────────────────────────────────────────────────────────────

/// Search the fact store via FTS5 BM25.
///
/// Returns `MemoryHit` values with `hit_type = Fact` and `importance = 1.0`.
/// An empty question returns an empty result (use `get_all_facts` instead).
pub async fn query_from_fact_store(
    pool: &SqlitePool,
    question: &str,
    limit: usize,
) -> Result<Vec<MemoryHit>> {
    if question.trim().is_empty() {
        return Ok(vec![]);
    }

    let fts_query = build_fts_query(question);
    let rows = match sqlx::query(
        r#"SELECT f.key, f.value, f.created_at, bm25(memory_facts_fts) as bm25_rank
           FROM memory_facts_fts
           JOIN memory_facts f ON f.rowid = memory_facts_fts.rowid
           WHERE memory_facts_fts MATCH ?
           ORDER BY bm25_rank
           LIMIT ?"#,
    )
    .bind(&fts_query)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            debug!("Fact store FTS query failed: {}", e);
            return Ok(vec![]);
        }
    };

    let now = chrono::Utc::now().timestamp();
    let hits = rows
        .iter()
        .map(|row| {
            let key: String = row.get("key");
            let value: String = row.get("value");
            let created_at: i64 = row.get("created_at");
            let bm25_rank: f32 = row.get("bm25_rank");

            // Display key without hash suffix for remembered facts
            let display_key = if let Some(base) = key.strip_prefix("remembered_fact:") {
                if base.len() == 16 {
                    "remembered_fact".to_string()
                } else {
                    key.clone()
                }
            } else {
                key.clone()
            };

            // Recency factor (facts don't decay but newer facts are slightly preferred)
            let age_days = ((now - created_at).max(0) as f64 / 86_400.0).min(365.0);
            let recency = (1.0 - age_days / 365.0 * 0.1) as f32; // max 10% penalty after a year

            MemoryHit {
                id: key,
                source: "fact_store".to_string(),
                content: format!("{}: {}", display_key, value),
                rank: bm25_rank as f64,
                hit_type: HitType::Fact,
                importance: 1.0,
                created_at,
                final_score: 0.95 * recency,
            }
        })
        .collect();

    Ok(hits)
}

// ─────────────────────────────────────────────────────────────────────────────
// FTS query builder — strips noise words, uses OR semantics
// ─────────────────────────────────────────────────────────────────────────────

/// Build a FTS5 query from a natural-language question.
///
/// Strips common noise/stop words and joins remaining terms with OR so that
/// "what do you know about the rove project" → `"rove" OR "project"` rather
/// than requiring ALL words to appear in the document.
/// Falls back to the raw question (truncated) if no terms survive.
fn build_fts_query(question: &str) -> String {
    const STOP_WORDS: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
        "what", "where", "when", "which", "who", "how", "why", "that", "this", "these", "those",
        "with", "from", "into", "about", "for", "of", "in", "on", "at", "to", "by", "or", "and",
        "not", "no", "my", "your", "our", "their", "you", "me", "we", "they", "he", "she", "it",
        "i", "tell", "show", "know", "list", "get", "give", "find", "any", "all", "me", "remember",
        "stored", "store", "using", "please", "there",
    ];

    let mut seen = std::collections::HashSet::new();
    let terms: Vec<String> = question
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| !STOP_WORDS.contains(&t.as_str()) && seen.insert(t.clone()))
        .collect();

    if terms.is_empty() {
        // Nothing meaningful — fall back; FTS5 may return nothing, that's fine
        question
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        // Quote each term to prevent FTS5 syntax errors, join with OR
        terms
            .iter()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remembered_fact_key_is_unique_per_content() {
        // Two different facts should produce different keys
        let mut h1 = DefaultHasher::new();
        "the deploy command is cargo build --release"
            .to_ascii_lowercase()
            .hash(&mut h1);

        let mut h2 = DefaultHasher::new();
        "use clippy for linting".to_ascii_lowercase().hash(&mut h2);

        assert_ne!(h1.finish(), h2.finish());
    }

    #[test]
    fn test_remembered_fact_key_is_stable() {
        // Same content always produces the same key
        let mut h1 = DefaultHasher::new();
        "use clippy for linting".to_ascii_lowercase().hash(&mut h1);
        let k1 = format!("remembered_fact:{:016x}", h1.finish());

        let mut h2 = DefaultHasher::new();
        "use clippy for linting".to_ascii_lowercase().hash(&mut h2);
        let k2 = format!("remembered_fact:{:016x}", h2.finish());

        assert_eq!(k1, k2);
    }

    #[test]
    fn test_build_fts_query_strips_noise_words() {
        let q = build_fts_query("what do you know about the rove project");
        // Should extract "rove" (len>=3, non-stop); "project" is NOT in stop list
        assert!(q.contains("rove"), "expected 'rove' in: {q}");
        assert!(!q.contains("what"), "should strip 'what' from: {q}");
        assert!(!q.contains("know"), "should strip 'know' from: {q}");
    }

    #[test]
    fn test_build_fts_query_uses_or() {
        let q = build_fts_query("rove workspace location");
        assert!(q.contains(" OR "), "expected OR in: {q}");
    }

    #[test]
    fn test_build_fts_query_quotes_terms() {
        let q = build_fts_query("rove workspace");
        assert!(q.starts_with('"'), "terms should be quoted: {q}");
    }
}
