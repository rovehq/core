//! Memory Query Operations
//!
//! Handles querying of episodic memory and consolidation insights using domain-gated layers.
//! Supports both BM25-only and hybrid (BM25 + vector similarity) search.

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use tracing::{debug, warn};

use crate::conductor::memory_types::*;
use crate::conductor::types::TaskDomain;

/// Query episodic memory and consolidation insights using domain-gated layers.
///
/// Returns up to `query_limit` results, combining matches from
/// episodic_fts, insights_fts, and agent_events_fts based on domain.
/// Sorted by hit_type priority (Insights first) then final_score descending.
///
/// # Arguments
/// * `pool` - SQLite connection pool
/// * `question` - Query text
/// * `domain` - Task domain for layer gating
/// * `team_id` - Optional team ID for scoping
/// * `query_limit` - Maximum number of results to return
/// * `min_importance` - Minimum importance threshold for episodic memories
pub async fn query(
    pool: &SqlitePool,
    question: &str,
    domain: &TaskDomain,
    team_id: Option<&str>,
    query_limit: usize,
    min_importance: f32,
) -> Result<Vec<MemoryHit>> {
    let layers = ContextLayers::for_domain(domain);
    let mut hits: Vec<MemoryHit> = Vec::new();
    let now = crate::conductor::scorer::unix_now();

    // Query insights layer
    if layers.insights {
        let domain_str = format!("{:?}", domain).to_ascii_lowercase();
        match sqlx::query(
            r#"SELECT i.id, i.insight, i.created_at, bm25(insights_fts) as bm25_rank
               FROM insights_fts
               JOIN consolidation_insights i ON i.rowid = insights_fts.rowid
               WHERE insights_fts MATCH ? AND (i.domain = ? OR i.domain IS NULL)
               ORDER BY bm25_rank
               LIMIT 20"#,
        )
        .bind(question)
        .bind(&domain_str)
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for row in rows {
                    let id: String = row.get("id");
                    let content: String = row.get("insight");
                    let created_at: i64 = row.get("created_at");
                    let bm25_rank: f32 = row.get("bm25_rank");

                    let final_score =
                        crate::conductor::scorer::score(bm25_rank, 1.0, created_at, now);

                    hits.push(MemoryHit {
                        id,
                        source: "insight".to_string(),
                        content,
                        rank: bm25_rank as f64,
                        hit_type: HitType::Insight,
                        importance: 1.0,
                        created_at,
                        final_score,
                    });
                }
            }
            Err(e) => {
                debug!("Insights query failed, skipping layer: {}", e);
            }
        }
    }

    // Query episodic layer
    if layers.episodic {
        let domain_str = format!("{:?}", domain).to_ascii_lowercase();

        match sqlx::query(
            r#"SELECT e.id, e.summary, e.importance, e.created_at, 
                      bm25(episodic_fts) as bm25_rank
               FROM episodic_fts
               JOIN episodic_memory e ON e.rowid = episodic_fts.rowid
               WHERE episodic_fts MATCH ? 
                 AND e.importance >= ?
                 AND e.sensitive = 0
                 AND (e.domain = ? OR e.domain = 'general')
                 AND (? IS NULL OR e.team_id = ? OR e.team_id IS NULL)
               ORDER BY bm25_rank
               LIMIT 20"#,
        )
        .bind(question)
        .bind(min_importance)
        .bind(&domain_str)
        .bind(team_id)
        .bind(team_id)
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for row in rows {
                    let id: String = row.get("id");
                    let content: String = row.get("summary");
                    let importance: f32 = row.get("importance");
                    let created_at: i64 = row.get("created_at");
                    let bm25_rank: f32 = row.get("bm25_rank");

                    let final_score =
                        crate::conductor::scorer::score(bm25_rank, importance, created_at, now);

                    hits.push(MemoryHit {
                        id,
                        source: "episodic".to_string(),
                        content,
                        rank: bm25_rank as f64,
                        hit_type: HitType::Episodic,
                        importance,
                        created_at,
                        final_score,
                    });
                }
            }
            Err(e) => {
                debug!("Episodic query failed, skipping layer: {}", e);
            }
        }
    }

    // Query task_trace layer (agent_events)
    if layers.task_trace {
        let domain_str = format!("{:?}", domain).to_ascii_lowercase();
        match sqlx::query(
            r#"SELECT ae.id, ae.event_type, ae.payload, ae.created_at,
                      bm25(agent_events_fts) as bm25_rank
               FROM agent_events_fts
               JOIN agent_events ae ON ae.rowid = agent_events_fts.rowid
               WHERE agent_events_fts MATCH ?
                 AND (ae.domain = ? OR ae.domain IS NULL)
               ORDER BY bm25_rank
               LIMIT 10"#,
        )
        .bind(question)
        .bind(&domain_str)
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for row in rows {
                    let id: String = row.get("id");
                    let event_type: String = row.get("event_type");
                    let payload: String = row.get("payload");
                    let created_at: i64 = row.get("created_at");
                    let bm25_rank: f32 = row.get("bm25_rank");

                    // Format content based on event type
                    let content = format!("[{}] {}", event_type, payload);

                    let final_score =
                        crate::conductor::scorer::score(bm25_rank, 0.8, created_at, now);

                    hits.push(MemoryHit {
                        id,
                        source: "task_trace".to_string(),
                        content,
                        rank: bm25_rank as f64,
                        hit_type: HitType::TaskTrace,
                        importance: 0.8,
                        created_at,
                        final_score,
                    });
                }
            }
            Err(e) => {
                debug!("Task trace query failed, skipping layer: {}", e);
            }
        }
    }

    // Sort: Insights first, then by final_score descending
    hits.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (&a.hit_type, &b.hit_type) {
            (HitType::Insight, HitType::Insight) => b
                .final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(Ordering::Equal),
            (HitType::Insight, _) => Ordering::Less,
            (_, HitType::Insight) => Ordering::Greater,
            _ => b
                .final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(Ordering::Equal),
        }
    });

    // Truncate to limit
    hits.truncate(query_limit);

    // Update access tracking for episodic memories
    for hit in &hits {
        if matches!(hit.hit_type, HitType::Episodic) {
            let _ = sqlx::query(
                r#"UPDATE episodic_memory 
                   SET access_count = access_count + 1, last_accessed = ?
                   WHERE id = ?"#,
            )
            .bind(now)
            .bind(&hit.id)
            .execute(pool)
            .await;
        }
    }

    debug!(
        "Memory query '{}' for domain {:?} returned {} hits",
        question,
        domain,
        hits.len()
    );
    Ok(hits)
}

/// Query episodic memory using hybrid search (BM25 + cosine similarity).
///
/// Combines FTS5 BM25 search with vector similarity search when embeddings are available.
/// Final score = 0.6 × bm25_score + 0.4 × cosine_similarity
///
/// # Arguments
/// * `pool` - SQLite connection pool
/// * `embedding_generator` - Optional embedding generator for hybrid search
/// * `question` - Query text
/// * `domain` - Task domain for layer gating
/// * `team_id` - Optional team ID for scoping
/// * `query_limit` - Maximum number of results to return
/// * `min_importance` - Minimum importance threshold for episodic memories
///
/// # Returns
/// Vector of memory hits sorted by hybrid score
pub async fn query_hybrid(
    pool: &SqlitePool,
    embedding_generator: Option<&Arc<crate::conductor::EmbeddingGenerator>>,
    question: &str,
    domain: &TaskDomain,
    team_id: Option<&str>,
    query_limit: usize,
    min_importance: f32,
) -> Result<Vec<MemoryHit>> {
    // If no embedding generator, fall back to regular query
    let generator = match embedding_generator {
        Some(g) => g,
        None => return query(pool, question, domain, team_id, query_limit, min_importance).await,
    };

    // Generate embedding for the question
    let question_embedding = match generator.local_brain {
        Some(ref brain) => match brain.embed(question).await {
            Ok(emb) => Some(emb),
            Err(e) => {
                warn!(error = %e, "failed to generate query embedding, falling back to BM25 only");
                None
            }
        },
        None => None,
    };

    let Some(query_emb) = question_embedding else {
        return query(pool, question, domain, team_id, query_limit, min_importance).await;
    };
    let layers = ContextLayers::for_domain(domain);
    let mut hits: Vec<MemoryHit> = Vec::new();
    let now = crate::conductor::scorer::unix_now();

    // Query episodic layer with embeddings
    if layers.episodic {
        let domain_str = format!("{:?}", domain).to_ascii_lowercase();

        // Fetch memories with BM25 scores and embeddings
        match sqlx::query(
            r#"SELECT e.id, e.summary, e.importance, e.created_at, e.embedding,
                      bm25(episodic_fts) as bm25_rank
               FROM episodic_fts
               JOIN episodic_memory e ON e.rowid = episodic_fts.rowid
               WHERE episodic_fts MATCH ? 
                 AND e.importance >= ?
                 AND e.sensitive = 0
                 AND (e.domain = ? OR e.domain = 'general')
                 AND (? IS NULL OR e.team_id = ? OR e.team_id IS NULL)
               ORDER BY bm25_rank
               LIMIT 50"#,
        )
        .bind(question)
        .bind(min_importance)
        .bind(&domain_str)
        .bind(team_id)
        .bind(team_id)
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for row in rows {
                    let id: String = row.get("id");
                    let content: String = row.get("summary");
                    let importance: f32 = row.get("importance");
                    let created_at: i64 = row.get("created_at");
                    let bm25_rank: f32 = row.get("bm25_rank");
                    let embedding_bytes: Option<Vec<u8>> = row.get("embedding");

                    // Calculate hybrid score
                    let final_score = if let Some(bytes) = embedding_bytes {
                        // Deserialize embedding
                        if let Ok(memory_emb) = bincode::deserialize::<Vec<f32>>(&bytes) {
                            // Calculate cosine similarity
                            let cosine_sim =
                                crate::conductor::EmbeddingGenerator::cosine_similarity(
                                    &query_emb,
                                    &memory_emb,
                                );

                            // Normalize BM25 score (BM25 is negative, closer to 0 is better)
                            // Convert to 0-1 range where 1 is best
                            let bm25_normalized = (-bm25_rank / 10.0).clamp(0.0, 1.0);

                            // Hybrid score: 0.6 × BM25 + 0.4 × cosine
                            let hybrid = 0.6 * bm25_normalized + 0.4 * cosine_sim;

                            // Apply importance and recency decay
                            hybrid
                                * importance
                                * crate::conductor::scorer::recency_decay(created_at, now)
                        } else {
                            // Embedding deserialization failed, use BM25 only
                            crate::conductor::scorer::score(bm25_rank, importance, created_at, now)
                        }
                    } else {
                        // No embedding, use BM25 only
                        crate::conductor::scorer::score(bm25_rank, importance, created_at, now)
                    };

                    hits.push(MemoryHit {
                        id,
                        source: "episodic".to_string(),
                        content,
                        rank: bm25_rank as f64,
                        hit_type: HitType::Episodic,
                        importance,
                        created_at,
                        final_score,
                    });
                }
            }
            Err(e) => {
                debug!("Episodic hybrid query failed, skipping layer: {}", e);
            }
        }
    }

    // Query insights layer (BM25 only, insights don't have embeddings)
    if layers.insights {
        let domain_str = format!("{:?}", domain).to_ascii_lowercase();
        match sqlx::query(
            r#"SELECT i.id, i.insight, i.created_at, bm25(insights_fts) as bm25_rank
               FROM insights_fts
               JOIN consolidation_insights i ON i.rowid = insights_fts.rowid
               WHERE insights_fts MATCH ? AND (i.domain = ? OR i.domain IS NULL)
               ORDER BY bm25_rank
               LIMIT 20"#,
        )
        .bind(question)
        .bind(&domain_str)
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for row in rows {
                    let id: String = row.get("id");
                    let content: String = row.get("insight");
                    let created_at: i64 = row.get("created_at");
                    let bm25_rank: f32 = row.get("bm25_rank");

                    let final_score =
                        crate::conductor::scorer::score(bm25_rank, 1.0, created_at, now);

                    hits.push(MemoryHit {
                        id,
                        source: "insight".to_string(),
                        content,
                        rank: bm25_rank as f64,
                        hit_type: HitType::Insight,
                        importance: 1.0,
                        created_at,
                        final_score,
                    });
                }
            }
            Err(e) => {
                debug!("Insights query failed, skipping layer: {}", e);
            }
        }
    }

    // Sort: Insights first, then by final_score descending
    hits.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (&a.hit_type, &b.hit_type) {
            (HitType::Insight, HitType::Insight) => b
                .final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(Ordering::Equal),
            (HitType::Insight, _) => Ordering::Less,
            (_, HitType::Insight) => Ordering::Greater,
            _ => b
                .final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(Ordering::Equal),
        }
    });

    // Truncate to limit
    hits.truncate(query_limit);

    // Update access tracking
    for hit in &hits {
        if matches!(hit.hit_type, HitType::Episodic) {
            let _ = sqlx::query(
                r#"UPDATE episodic_memory 
                   SET access_count = access_count + 1, last_accessed = ?
                   WHERE id = ?"#,
            )
            .bind(now)
            .bind(&hit.id)
            .execute(pool)
            .await;
        }
    }

    debug!(
        "Hybrid memory query '{}' for domain {:?} returned {} hits",
        question,
        domain,
        hits.len()
    );
    Ok(hits)
}

// ─────────────────────────────────────────────────────────────────────────────
// Typed query functions — load only the relevant memory subset
// ─────────────────────────────────────────────────────────────────────────────

/// Query episodic memories by `MemoryKind` (typed/scoped query).
///
/// Does a direct `WHERE memory_kind = ?` scan — no FTS — so it returns
/// all matching memories above the importance threshold, sorted by importance.
/// Use this when you want all warnings, all preferences, etc.
pub async fn query_by_kind(
    pool: &SqlitePool,
    kind: &crate::conductor::types::MemoryKind,
    domain: &TaskDomain,
    limit: usize,
    min_importance: f32,
) -> Result<Vec<MemoryHit>> {
    let kind_str = kind.as_str();
    let domain_str = format!("{:?}", domain).to_lowercase();
    let now = crate::conductor::scorer::unix_now();

    let rows = match sqlx::query(
        r#"SELECT id, summary, importance, created_at
           FROM episodic_memory
           WHERE memory_kind = ?
             AND (domain = ? OR domain = 'general')
             AND sensitive = 0
             AND importance >= ?
           ORDER BY importance DESC, created_at DESC
           LIMIT ?"#,
    )
    .bind(kind_str)
    .bind(&domain_str)
    .bind(min_importance)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            debug!("query_by_kind failed: {}", e);
            return Ok(vec![]);
        }
    };

    Ok(rows
        .iter()
        .map(|row| {
            let id: String = row.get("id");
            let content: String = row.get("summary");
            let importance: f32 = row.get("importance");
            let created_at: i64 = row.get("created_at");
            let final_score = importance * crate::conductor::scorer::recency_decay(created_at, now);
            MemoryHit {
                id,
                source: format!("episodic:{}", kind_str),
                content,
                rank: 0.0,
                hit_type: HitType::Episodic,
                importance,
                created_at,
                final_score,
            }
        })
        .collect())
}

/// Query episodic memories mentioning a specific entity or concept.
///
/// Uses FTS5 to search the `entities` and `summary` columns. Good for
/// queries like "find all memories about tokio" or "find memories about E0499".
pub async fn query_entity_scoped(
    pool: &SqlitePool,
    entity_name: &str,
    domain: &TaskDomain,
    limit: usize,
) -> Result<Vec<MemoryHit>> {
    if entity_name.trim().is_empty() {
        return Ok(vec![]);
    }

    let domain_str = format!("{:?}", domain).to_lowercase();
    let now = crate::conductor::scorer::unix_now();

    let rows = match sqlx::query(
        r#"SELECT e.id, e.summary, e.importance, e.created_at,
                  bm25(episodic_fts) as bm25_rank
           FROM episodic_fts
           JOIN episodic_memory e ON e.rowid = episodic_fts.rowid
           WHERE episodic_fts MATCH ?
             AND e.sensitive = 0
             AND (e.domain = ? OR e.domain = 'general')
           ORDER BY bm25_rank
           LIMIT ?"#,
    )
    .bind(entity_name)
    .bind(&domain_str)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            debug!("query_entity_scoped failed: {}", e);
            return Ok(vec![]);
        }
    };

    Ok(rows
        .iter()
        .map(|row| {
            let id: String = row.get("id");
            let content: String = row.get("summary");
            let importance: f32 = row.get("importance");
            let created_at: i64 = row.get("created_at");
            let bm25_rank: f32 = row.get("bm25_rank");
            let final_score =
                crate::conductor::scorer::score(bm25_rank, importance, created_at, now);
            MemoryHit {
                id,
                source: "episodic".to_string(),
                content,
                rank: bm25_rank as f64,
                hit_type: HitType::Episodic,
                importance,
                created_at,
                final_score,
            }
        })
        .collect())
}

/// Return the N most recent episodic memories for a domain, sorted by time.
///
/// Useful for injecting recent context at the top of a prompt without
/// requiring a specific search query.
pub async fn query_recent(
    pool: &SqlitePool,
    domain: &TaskDomain,
    limit: usize,
    min_importance: f32,
) -> Result<Vec<MemoryHit>> {
    let domain_str = format!("{:?}", domain).to_lowercase();
    let now = crate::conductor::scorer::unix_now();

    let rows = match sqlx::query(
        r#"SELECT id, summary, importance, created_at
           FROM episodic_memory
           WHERE (domain = ? OR domain = 'general')
             AND sensitive = 0
             AND importance >= ?
           ORDER BY created_at DESC
           LIMIT ?"#,
    )
    .bind(&domain_str)
    .bind(min_importance)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            debug!("query_recent failed: {}", e);
            return Ok(vec![]);
        }
    };

    Ok(rows
        .iter()
        .map(|row| {
            let id: String = row.get("id");
            let content: String = row.get("summary");
            let importance: f32 = row.get("importance");
            let created_at: i64 = row.get("created_at");
            let final_score = importance * crate::conductor::scorer::recency_decay(created_at, now);
            MemoryHit {
                id,
                source: "episodic".to_string(),
                content,
                rank: 0.0,
                hit_type: HitType::Episodic,
                importance,
                created_at,
                final_score,
            }
        })
        .collect())
}

/// Query task traces relevant to a question using FTS over agent events.
pub async fn query_task_traces(
    pool: &SqlitePool,
    question: &str,
    domain: &TaskDomain,
    limit: usize,
) -> Result<Vec<MemoryHit>> {
    if question.trim().is_empty() || limit == 0 {
        return Ok(vec![]);
    }

    let domain_str = format!("{:?}", domain).to_lowercase();
    let now = crate::conductor::scorer::unix_now();

    let rows = match sqlx::query(
        r#"SELECT ae.id, ae.event_type, ae.payload, ae.created_at,
                  bm25(agent_events_fts) as bm25_rank
           FROM agent_events_fts
           JOIN agent_events ae ON ae.rowid = agent_events_fts.rowid
           WHERE agent_events_fts MATCH ?
             AND (ae.domain = ? OR ae.domain IS NULL)
           ORDER BY bm25_rank
           LIMIT ?"#,
    )
    .bind(question)
    .bind(&domain_str)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            debug!("query_task_traces failed: {}", e);
            return Ok(vec![]);
        }
    };

    Ok(rows
        .iter()
        .map(|row| {
            let id: String = row.get("id");
            let event_type: String = row.get("event_type");
            let payload: String = row.get("payload");
            let created_at: i64 = row.get("created_at");
            let bm25_rank: f32 = row.get("bm25_rank");
            let content = format!("[{}] {}", event_type, payload);
            let final_score = crate::conductor::scorer::score(bm25_rank, 0.8, created_at, now);

            MemoryHit {
                id,
                source: "task_trace".to_string(),
                content,
                rank: bm25_rank as f64,
                hit_type: HitType::TaskTrace,
                importance: 0.8,
                created_at,
                final_score,
            }
        })
        .collect())
}

/// Return the N most recent task traces for a domain.
pub async fn query_recent_task_traces(
    pool: &SqlitePool,
    domain: &TaskDomain,
    limit: usize,
) -> Result<Vec<MemoryHit>> {
    if limit == 0 {
        return Ok(vec![]);
    }

    let domain_str = format!("{:?}", domain).to_lowercase();
    let now = crate::conductor::scorer::unix_now();

    let rows = match sqlx::query(
        r#"SELECT id, event_type, payload, created_at
           FROM agent_events
           WHERE (domain = ? OR domain IS NULL)
           ORDER BY created_at DESC
           LIMIT ?"#,
    )
    .bind(&domain_str)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            debug!("query_recent_task_traces failed: {}", e);
            return Ok(vec![]);
        }
    };

    Ok(rows
        .iter()
        .map(|row| {
            let id: String = row.get("id");
            let event_type: String = row.get("event_type");
            let payload: String = row.get("payload");
            let created_at: i64 = row.get("created_at");
            let content = format!("[{}] {}", event_type, payload);
            let final_score = 0.8 * crate::conductor::scorer::recency_decay(created_at, now);

            MemoryHit {
                id,
                source: "task_trace".to_string(),
                content,
                rank: 0.0,
                hit_type: HitType::TaskTrace,
                importance: 0.8,
                created_at,
                final_score,
            }
        })
        .collect())
}
