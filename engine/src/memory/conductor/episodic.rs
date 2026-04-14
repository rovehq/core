//! Episodic Memory Ingest
//!
//! Handles ingestion of completed tasks into episodic memory.
//! Uses the `MemoryExtractor` trait to extract structured information —
//! no hard dependency on an LLM; the heuristic backend always works offline.
//!
//! Three optional enhancements (all controlled by `MemoryConfig`):
//!   - `fact_store_enabled` — writes Fact/Preference kinds to `memory_facts`
//!   - `embed_at_ingest`    — generates vector embedding inline (requires LocalBrain)
//!   - knowledge graph extraction (fire-and-forget, importance ≥ 0.5)

use anyhow::{Context, Result};
use regex::Regex;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::conductor::extract::MemoryExtractor;
use crate::conductor::types::{GraphSourceKind, IngestResult, TaskDomain};
use crate::security::secrets::scrub_text;

// ─────────────────────────────────────────────────────────────────────────────
// ingest() — public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Ingest a completed task into episodic memory.
///
/// Delegates extraction to `extractor` (heuristic, LLM, or auto).
/// Sensitive tasks always use the heuristic extractor to keep data local.
#[allow(clippy::too_many_arguments)]
pub async fn ingest(
    pool: &SqlitePool,
    extractor: &Arc<dyn MemoryExtractor>,
    knowledge_graph: &Arc<crate::knowledge_graph::KnowledgeGraph>,
    entity_extractor: &Arc<crate::knowledge_graph::EntityExtractor>,
    task_input: &str,
    task_result: &str,
    task_id: &str,
    domain: &TaskDomain,
    sensitive: bool,
    fact_store_enabled: bool,
    embed_at_ingest: bool,
    embedding_generator: Option<&Arc<crate::conductor::EmbeddingGenerator>>,
) -> Result<IngestResult> {
    let task_input = if sensitive {
        scrub_text(task_input)
    } else {
        task_input.to_string()
    };
    let task_result = if sensitive {
        scrub_text(task_result)
    } else {
        task_result.to_string()
    };

    let memory_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Sensitive tasks always use heuristic to avoid leaking data to any LLM
    let extraction = if sensitive {
        debug!("Using heuristic extraction for sensitive task {}", task_id);
        crate::conductor::extract::extract_heuristic_combined(&task_input, &task_result)
    } else {
        extractor.extract(&task_input, &task_result).await
    };

    let importance = extraction.importance.clamp(0.0, 1.0);
    let entities_json =
        serde_json::to_string(&extraction.entities).unwrap_or_else(|_| "[]".to_string());
    let topics_json =
        serde_json::to_string(&extraction.topics).unwrap_or_else(|_| "[]".to_string());
    let domain_str = format!("{:?}", domain).to_lowercase();
    let kind_str = extraction.kind.as_str();
    let sensitive_int = if sensitive { 1i64 } else { 0i64 };

    // Optionally generate an embedding immediately (requires LocalBrain)
    let embedding_blob: Option<Vec<u8>> = if embed_at_ingest {
        generate_embedding_blob(embedding_generator, &extraction.summary).await
    } else {
        None
    };

    // INSERT base row
    sqlx::query(
        r#"INSERT INTO episodic_memory
           (id, task_id, summary, entities, topics, importance,
            consolidated, created_at, domain, sensitive, memory_kind)
           VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)"#,
    )
    .bind(&memory_id)
    .bind(task_id)
    .bind(&extraction.summary)
    .bind(&entities_json)
    .bind(&topics_json)
    .bind(importance)
    .bind(now)
    .bind(&domain_str)
    .bind(sensitive_int)
    .bind(kind_str)
    .execute(pool)
    .await
    .context("Failed to insert episodic memory")?;

    // Store embedding if generated
    if let Some(blob) = embedding_blob {
        let _ = sqlx::query(
            r#"UPDATE episodic_memory
               SET embedding = ?, embedding_model = ?, embedding_generated_at = ?
               WHERE id = ?"#,
        )
        .bind(&blob)
        .bind("local-brain")
        .bind(now)
        .bind(&memory_id)
        .execute(pool)
        .await;
    }

    // Write structured facts to the fact store
    if fact_store_enabled && !extraction.facts.is_empty() {
        for (key, value) in &extraction.facts {
            if let Err(e) =
                crate::conductor::fact_store::upsert_fact(pool, key, value, task_id, &memory_id)
                    .await
            {
                warn!("Failed to upsert fact '{}': {}", key, e);
            }
        }
    }

    info!(
        task_id = %task_id,
        memory_id = %memory_id,
        importance = importance,
        kind = %kind_str,
        extractor = %extractor.name(),
        "ingested memory"
    );

    // Build memory-to-memory graph edges (deterministic, no LLM required)
    let _ = crate::conductor::memory_graph::build_edges_for_memory(
        pool,
        &memory_id,
        task_id,
        &extraction.entities,
        &domain_str,
        now,
    )
    .await;

    // Fire-and-forget: extract entities for the knowledge graph
    if !sensitive && importance >= 0.5 {
        persist_graph_facts(
            entity_extractor.as_ref(),
            knowledge_graph.as_ref(),
            &task_input,
            &extraction.summary,
            &memory_id,
            now,
        )
        .await;
    }

    Ok(IngestResult {
        memory_id,
        summary: extraction.summary,
        entities: extraction.entities,
        topics: extraction.topics,
        importance,
        kind: extraction.kind,
    })
}

/// Lightweight ingest used when `memory.mode = graph_only`.
///
/// This persists only explicit/pinned facts into `memory_facts`. It does not
/// write broad episodic rows or trigger consolidation-oriented storage.
#[allow(clippy::too_many_arguments)]
pub async fn ingest_pinned_facts_only(
    pool: &SqlitePool,
    extractor: &Arc<dyn MemoryExtractor>,
    task_input: &str,
    task_result: &str,
    task_id: &str,
    _domain: &TaskDomain,
    sensitive: bool,
    fact_store_enabled: bool,
) -> Result<IngestResult> {
    let task_input = if sensitive {
        scrub_text(task_input)
    } else {
        task_input.to_string()
    };
    let task_result = if sensitive {
        scrub_text(task_result)
    } else {
        task_result.to_string()
    };

    let extraction = if sensitive {
        crate::conductor::extract::extract_heuristic_combined(&task_input, &task_result)
    } else {
        extractor.extract(&task_input, &task_result).await
    };

    if fact_store_enabled {
        let memory_id = format!("graph-only:{task_id}");
        for (key, value) in &extraction.facts {
            if let Err(error) =
                crate::conductor::fact_store::upsert_fact(pool, key, value, task_id, &memory_id)
                    .await
            {
                warn!("Failed to upsert pinned fact '{}': {}", key, error);
            }
        }
    }

    let summary = if extraction.facts.is_empty() {
        "graph_only mode keeps explicit pinned facts only; no fact was extracted.".to_string()
    } else {
        extraction.summary.clone()
    };

    Ok(IngestResult {
        memory_id: format!("graph-only:{task_id}"),
        summary,
        entities: extraction.entities,
        topics: extraction.topics,
        importance: extraction.importance,
        kind: extraction.kind,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Embedding helper
// ─────────────────────────────────────────────────────────────────────────────

async fn generate_embedding_blob(
    generator: Option<&Arc<crate::conductor::EmbeddingGenerator>>,
    text: &str,
) -> Option<Vec<u8>> {
    let gen = generator?;
    let brain = gen.local_brain.as_ref()?;
    match brain.embed(text).await {
        Ok(emb) => match bincode::serialize(&emb) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                warn!("Failed to serialize embedding: {}", e);
                None
            }
        },
        Err(e) => {
            warn!("Failed to generate embedding at ingest: {}", e);
            None
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Knowledge graph persistence (fire-and-forget)
// ─────────────────────────────────────────────────────────────────────────────

struct GraphFacts {
    entities: Vec<crate::knowledge_graph::Entity>,
    relationships: Vec<crate::knowledge_graph::Relationship>,
}

fn extract_graph_facts(task_input: &str) -> GraphFacts {
    let mut facts = GraphFacts {
        entities: Vec::new(),
        relationships: Vec::new(),
    };

    let Some(summary) = project_workspace_summary(task_input) else {
        return facts;
    };
    let pattern =
        Regex::new(r"(?i)^The\s+(.+?)\s+project is stored at\s+(\S+)\s+and uses\s+(.+?)\.$").ok();
    let Some(pattern) = pattern else {
        return facts;
    };
    let Some(captures) = pattern.captures(&summary) else {
        return facts;
    };

    let project = captures
        .get(1)
        .map(|m| m.as_str().trim())
        .unwrap_or_default();
    let path = captures
        .get(2)
        .map(|m| m.as_str().trim())
        .unwrap_or_default();
    let language = captures
        .get(3)
        .map(|m| m.as_str().trim())
        .unwrap_or_default();
    if project.is_empty() || path.is_empty() || language.is_empty() {
        return facts;
    }

    facts.entities.push(crate::knowledge_graph::Entity {
        label: "User".to_string(),
        entity_type: crate::knowledge_graph::EntityType::Person,
        properties: serde_json::json!({"role": "owner"}),
    });
    facts.entities.push(crate::knowledge_graph::Entity {
        label: project.to_string(),
        entity_type: crate::knowledge_graph::EntityType::Project,
        properties: serde_json::json!({"summary": summary}),
    });
    facts.entities.push(crate::knowledge_graph::Entity {
        label: path.to_string(),
        entity_type: crate::knowledge_graph::EntityType::File,
        properties: serde_json::json!({"kind": "workspace_path"}),
    });
    facts.entities.push(crate::knowledge_graph::Entity {
        label: language.to_string(),
        entity_type: crate::knowledge_graph::EntityType::Tool,
        properties: serde_json::json!({"kind": "language"}),
    });

    facts
        .relationships
        .push(crate::knowledge_graph::Relationship {
            from_label: "User".to_string(),
            to_label: project.to_string(),
            relation: crate::knowledge_graph::RelationType::WorksOn,
            weight: 1.0,
        });
    facts
        .relationships
        .push(crate::knowledge_graph::Relationship {
            from_label: project.to_string(),
            to_label: path.to_string(),
            relation: crate::knowledge_graph::RelationType::StoredAt,
            weight: 1.0,
        });
    facts
        .relationships
        .push(crate::knowledge_graph::Relationship {
            from_label: project.to_string(),
            to_label: language.to_string(),
            relation: crate::knowledge_graph::RelationType::Uses,
            weight: 0.9,
        });

    facts
}

fn project_workspace_summary(task_input: &str) -> Option<String> {
    let pattern = Regex::new(
        r"(?i)^\s*i work on(?:\s+the)?\s+(.+?)(?:\s+project)?\s+in\s+(\S+)\s+using\s+(.+?)\s*$",
    )
    .ok()?;
    let captures = pattern.captures(task_input)?;
    let project = captures.get(1)?.as_str().trim().trim_start_matches("the ");
    let path = captures.get(2)?.as_str().trim();
    let language = captures.get(3)?.as_str().trim().trim_end_matches('.');
    if project.is_empty() || path.is_empty() || language.is_empty() {
        return None;
    }
    Some(format!(
        "The {} project is stored at {} and uses {}.",
        project, path, language
    ))
}

fn dedupe_entities(
    entities: Vec<crate::knowledge_graph::Entity>,
) -> Vec<crate::knowledge_graph::Entity> {
    let mut deduped = Vec::new();
    let mut seen = HashMap::new();
    for entity in entities {
        let key = format!(
            "{}:{}",
            entity.entity_type.as_str(),
            entity.label.to_ascii_lowercase()
        );
        if seen.insert(key, ()).is_none() {
            deduped.push(entity);
        }
    }
    deduped
}

fn dedupe_relationships(
    relationships: Vec<crate::knowledge_graph::Relationship>,
) -> Vec<crate::knowledge_graph::Relationship> {
    let mut deduped = Vec::new();
    let mut seen = HashMap::new();
    for relationship in relationships {
        let key = format!(
            "{}:{}:{}",
            relationship.from_label.to_ascii_lowercase(),
            relationship.relation.as_str(),
            relationship.to_label.to_ascii_lowercase()
        );
        if seen.insert(key, ()).is_none() {
            deduped.push(relationship);
        }
    }
    deduped
}

async fn resolve_node_id(
    graph: &crate::knowledge_graph::KnowledgeGraph,
    node_ids: &HashMap<String, String>,
    label: &str,
) -> Result<Option<String>> {
    if let Some(node_id) = node_ids.get(&label.to_ascii_lowercase()) {
        return Ok(Some(node_id.clone()));
    }
    Ok(graph.find_node_by_label(label).await?.map(|node| node.id))
}

async fn persist_graph_facts(
    extractor: &crate::knowledge_graph::EntityExtractor,
    graph: &crate::knowledge_graph::KnowledgeGraph,
    task_input: &str,
    summary: &str,
    memory_id: &str,
    created_at: i64,
) {
    let mut entities = Vec::new();
    let mut relationships = Vec::new();

    match extractor.extract(summary).await {
        Ok(result) => {
            entities.extend(result.entities);
            relationships.extend(result.relationships);
        }
        Err(error) => {
            warn!(
                error = %error,
                memory_id = %memory_id,
                "entity extraction failed (non-fatal)"
            );
        }
    }

    let heuristic = extract_graph_facts(task_input);
    entities.extend(heuristic.entities);
    relationships.extend(heuristic.relationships);

    if entities.is_empty() {
        debug!(memory_id = %memory_id, "no graph entities extracted");
        return;
    }

    let entity_count = entities.len();
    let rel_count = relationships.len();
    let mut node_ids = HashMap::new();

    for entity in dedupe_entities(entities) {
        let node_id = crate::knowledge_graph::KnowledgeGraph::canonical_node_id(
            &entity.entity_type,
            &entity.label,
        );
        let node = crate::knowledge_graph::GraphNode {
            id: node_id.clone(),
            label: entity.label.clone(),
            node_type: entity.entity_type.clone(),
            properties: entity.properties.clone(),
            source_kind: GraphSourceKind::TaskTrace,
            source_scope: "per_node".to_string(),
            source_ref: Some(memory_id.to_string()),
            confidence: 1.0,
            created_at,
            last_updated: created_at,
            access_count: 0,
        };

        if let Err(error) = graph.upsert_node(&node).await {
            warn!(error = %error, entity = %entity.label, "failed to store entity");
            continue;
        }

        if let Err(error) = graph
            .link_memory(memory_id, &node_id, 1.0, created_at)
            .await
        {
            warn!(error = %error, entity = %entity.label, "failed to link memory to entity");
        }

        node_ids.insert(entity.label.to_lowercase(), node_id);
    }

    for relationship in dedupe_relationships(relationships) {
        let from_id = match resolve_node_id(graph, &node_ids, &relationship.from_label).await {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(error) => {
                warn!(error = %error, label = %relationship.from_label, "failed to resolve source");
                continue;
            }
        };
        let to_id = match resolve_node_id(graph, &node_ids, &relationship.to_label).await {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(error) => {
                warn!(error = %error, label = %relationship.to_label, "failed to resolve target");
                continue;
            }
        };

        let edge = crate::knowledge_graph::GraphEdge {
            id: crate::knowledge_graph::KnowledgeGraph::canonical_edge_id(
                &from_id,
                &relationship.relation,
                &to_id,
            ),
            from_id,
            to_id,
            relation: relationship.relation.clone(),
            weight: relationship.weight,
            properties: None,
            source_kind: GraphSourceKind::TaskTrace,
            source_scope: "per_node".to_string(),
            source_ref: Some(memory_id.to_string()),
            confidence: 1.0,
            created_at,
            updated_at: created_at,
        };

        if let Err(error) = graph.add_edge(&edge).await {
            warn!(
                error = %error,
                from = %relationship.from_label,
                to = %relationship.to_label,
                "failed to store relationship"
            );
        }
    }

    debug!(
        memory_id = %memory_id,
        entities = entity_count,
        relationships = rel_count,
        "extracted entities for knowledge graph"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_graph_facts_for_workspace_statement() {
        let facts =
            extract_graph_facts("I work on the rove project in ~/workspace/rove using Rust");
        assert_eq!(facts.entities.len(), 4);
        assert_eq!(facts.relationships.len(), 3);
        assert!(facts
            .relationships
            .iter()
            .any(|rel| matches!(rel.relation, crate::knowledge_graph::RelationType::StoredAt)));
    }

    #[test]
    fn test_project_workspace_summary_match() {
        let s =
            project_workspace_summary("I work on the rove project in ~/workspace/rove using Rust");
        assert!(s.is_some());
        assert!(s.unwrap().contains("rove"));
    }

    #[test]
    fn test_project_workspace_summary_no_match() {
        assert!(project_workspace_summary("just a regular message").is_none());
    }
}
