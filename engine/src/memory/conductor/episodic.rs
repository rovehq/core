//! Episodic Memory Ingest
//!
//! Handles ingestion of completed tasks into episodic memory.
//! Calls LLM to extract structured information and stores in episodic_memory table.
//! Includes fire-and-forget knowledge graph extraction for high-importance memories.

use anyhow::{Context, Result};
use regex::Regex;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::conductor::memory_types::*;
use crate::conductor::memory_utils::*;
use crate::conductor::types::TaskDomain;
use crate::llm::router::LLMRouter;
use crate::security::secrets::scrub_text;

/// Ingest a completed task into episodic memory.
///
/// Calls the LLM with `INGEST_PROMPT` + task content, parses the
/// structured JSON, and INSERTs into `episodic_memory`. If the LLM
/// call fails or the response cannot be parsed, a fallback raw
/// memory is stored so no task is ever lost.
///
/// # Arguments
/// * `pool` - SQLite connection pool
/// * `router` - LLM router for extraction calls
/// * `knowledge_graph` - Knowledge graph for entity extraction
/// * `entity_extractor` - Entity extractor for knowledge graph
/// * `task_input` - The input to the task
/// * `task_result` - The result of the task
/// * `task_id` - The task identifier
/// * `domain` - The task domain for domain-gated queries
/// * `sensitive` - Whether the memory contains sensitive information
#[allow(clippy::too_many_arguments)]
pub async fn ingest(
    pool: &SqlitePool,
    router: &LLMRouter,
    knowledge_graph: &Arc<crate::knowledge_graph::KnowledgeGraph>,
    entity_extractor: &Arc<crate::knowledge_graph::EntityExtractor>,
    task_input: &str,
    task_result: &str,
    task_id: &str,
    domain: &TaskDomain,
    sensitive: bool,
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

    // Build the prompt
    let content = format!(
        "{}INPUT:\n{}\n\nRESULT:\n{}",
        crate::conductor::memory_prompts::INGEST_PROMPT,
        task_input,
        task_result
    );

    // Sensitive memories must stay local-only. Skip extractor LLM calls entirely
    // and rely on scrubbed fallback/heuristic summaries instead.
    let extraction = if sensitive {
        debug!(
            "Skipping LLM memory extraction for sensitive task {}",
            task_id
        );
        None
    } else {
        match call_llm_for_text(router, &content).await {
            Ok(text) => parse_ingest_response(&text),
            Err(e) => {
                warn!("LLM ingest call failed, storing raw fallback: {}", e);
                None
            }
        }
    };

    let extraction = extraction.unwrap_or_else(|| {
        // Fallback: store a simple raw memory so nothing is lost
        IngestExtraction {
            summary: truncate(&task_input, 200),
            entities: vec![],
            topics: vec![],
            importance: 0.3,
        }
    });
    let mut extraction = apply_fact_heuristics(&task_input, extraction);
    if sensitive {
        extraction.summary = scrub_text(&extraction.summary);
    }

    // Clamp importance
    let importance = extraction.importance.clamp(0.0, 1.0);
    let entities_json =
        serde_json::to_string(&extraction.entities).unwrap_or_else(|_| "[]".to_string());
    let topics_json =
        serde_json::to_string(&extraction.topics).unwrap_or_else(|_| "[]".to_string());

    // INSERT into episodic_memory
    let domain_str = format!("{:?}", domain);
    let sensitive_int = if sensitive { 1 } else { 0 };

    sqlx::query(
        r#"INSERT INTO episodic_memory
           (id, task_id, summary, entities, topics, importance, consolidated, created_at, domain, sensitive)
           VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, ?)"#,
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
    .execute(pool)
    .await
    .context("Failed to insert episodic memory")?;

    info!(
        task_id = %task_id,
        memory_id = %memory_id,
        importance = importance,
        "ingested memory"
    );

    // Fire-and-forget: extract entities and relationships for knowledge graph
    // Skip if sensitive (privacy) or low importance (noise reduction)
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
    })
}

/// Call the LLM router and extract the text content from FinalAnswer.
/// Uses a simple system + user message pair. 60s timeout.
async fn call_llm_for_text(router: &LLMRouter, user_content: &str) -> Result<String> {
    use crate::llm::Message;
    use std::time::Duration;
    use tokio::time::timeout;

    let messages = vec![
        Message::system("You are a structured data extraction system. Respond with ONLY valid JSON, no markdown fences, no explanation."),
        Message::user(user_content),
    ];

    let result = timeout(Duration::from_secs(60), router.call(&messages))
        .await
        .context("LLM call timed out")?
        .map_err(|e| anyhow::anyhow!("LLM call failed: {}", e))?;

    let (response, provider) = result;
    debug!("Memory LLM call answered by {}", provider);

    match response {
        crate::llm::LLMResponse::FinalAnswer(answer) => Ok(answer.content),
        crate::llm::LLMResponse::ToolCall(tc) => {
            // Some providers may return this as a tool call; extract from arguments
            warn!("Memory LLM returned tool call instead of text, using arguments");
            Ok(tc.arguments)
        }
    }
}

/// Parse LLM response from ingest prompt into IngestExtraction.
/// Returns None if parsing fails — caller handles fallback.
fn parse_ingest_response(text: &str) -> Option<IngestExtraction> {
    let cleaned = strip_markdown_fences(text);

    match serde_json::from_str::<IngestExtraction>(&cleaned) {
        Ok(mut extraction) => {
            // Clamp and validate
            extraction.importance = extraction.importance.clamp(0.0, 1.0);
            extraction.entities.truncate(10);
            extraction.topics.truncate(5);
            if extraction.summary.is_empty() {
                warn!("LLM returned empty summary");
                return None;
            }
            Some(extraction)
        }
        Err(e) => {
            warn!(
                "Failed to parse ingest LLM response: {} — raw: {}",
                e,
                truncate(text, 200)
            );
            None
        }
    }
}

fn apply_fact_heuristics(task_input: &str, mut extraction: IngestExtraction) -> IngestExtraction {
    if let Some(fact) = remembered_fact(task_input) {
        extraction.summary = fact;
        extraction.importance = extraction.importance.max(0.85);
        push_topic(&mut extraction.topics, "remembered_fact");
    }

    if let Some((key, value)) = user_property(task_input) {
        extraction.summary = format!("User's {} is {}.", key, value);
        extraction.importance = extraction.importance.max(0.9);
        push_topic(&mut extraction.topics, "user_preference");
    }

    if let Some(preference) = verification_preference(task_input) {
        extraction.summary = preference;
        extraction.importance = extraction.importance.max(0.85);
        push_topic(&mut extraction.topics, "verification_preference");
    }

    if let Some(workspace_fact) = project_workspace_summary(task_input) {
        extraction.summary = workspace_fact;
        extraction.importance = extraction.importance.max(0.8);
        push_topic(&mut extraction.topics, "project_context");
    }

    extraction
}

fn push_topic(topics: &mut Vec<String>, topic: &str) {
    if !topics.iter().any(|existing| existing == topic) {
        topics.push(topic.to_string());
    }
}

fn remembered_fact(task_input: &str) -> Option<String> {
    let pattern = Regex::new(r"(?i)^\s*remember(?:\s+that)?\s+(.+?)\s*$").ok()?;
    let captures = pattern.captures(task_input)?;
    let fact = captures.get(1)?.as_str().trim().trim_end_matches('.');
    if fact.is_empty() {
        return None;
    }

    Some(format!("Remembered fact: {}.", fact))
}

fn user_property(task_input: &str) -> Option<(String, String)> {
    let pattern = Regex::new(r"(?i)^\s*my\s+(.+?)\s+is\s+(.+?)\s*$").ok()?;
    let captures = pattern.captures(task_input)?;
    let key = captures.get(1)?.as_str().trim().to_ascii_lowercase();
    let value = captures
        .get(2)?
        .as_str()
        .trim()
        .trim_end_matches('.')
        .to_string();
    if key.is_empty() || value.is_empty() {
        return None;
    }

    Some((key, value))
}

fn verification_preference(task_input: &str) -> Option<String> {
    let input = task_input.to_ascii_lowercase();
    if input.contains("cargo test before saying done") {
        return Some("User prefers cargo test before saying done.".to_string());
    }
    if input.contains("test it") || input.contains("run tests") || input.contains("cargo test") {
        return Some("User prefers running cargo test before completion.".to_string());
    }
    None
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
                warn!(
                    error = %error,
                    label = %relationship.from_label,
                    "failed to resolve relationship source"
                );
                continue;
            }
        };
        let to_id = match resolve_node_id(graph, &node_ids, &relationship.to_label).await {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(error) => {
                warn!(
                    error = %error,
                    label = %relationship.to_label,
                    "failed to resolve relationship target"
                );
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
            created_at,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ingest_response_valid() {
        let json = r#"{"summary":"Fixed a bug","entities":["Rust","tokio"],"topics":["debugging"],"importance":0.8}"#;
        let result = parse_ingest_response(json);
        assert!(result.is_some());
        let ext = result.unwrap();
        assert_eq!(ext.summary, "Fixed a bug");
        assert_eq!(ext.entities, vec!["Rust", "tokio"]);
        assert_eq!(ext.importance, 0.8);
    }

    #[test]
    fn test_parse_ingest_response_with_fences() {
        let json =
            "```json\n{\"summary\":\"Test\",\"entities\":[],\"topics\":[],\"importance\":0.5}\n```";
        let result = parse_ingest_response(json);
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_ingest_response_invalid() {
        let result = parse_ingest_response("not json at all");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_ingest_response_empty_summary() {
        let json = r#"{"summary":"","entities":[],"topics":[],"importance":0.5}"#;
        let result = parse_ingest_response(json);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_ingest_response_clamps_importance() {
        let json = r#"{"summary":"Test","entities":[],"topics":[],"importance":5.0}"#;
        let result = parse_ingest_response(json);
        assert!(result.is_some());
        assert_eq!(result.unwrap().importance, 1.0);
    }

    #[test]
    fn test_apply_fact_heuristics_for_remembered_fact() {
        let extraction = IngestExtraction {
            summary: "fallback".to_string(),
            entities: vec![],
            topics: vec![],
            importance: 0.3,
        };

        let updated = apply_fact_heuristics(
            "remember that the deploy command is: cargo build --release",
            extraction,
        );

        assert!(updated.summary.contains("cargo build --release"));
        assert!(updated.importance >= 0.85);
        assert!(updated.topics.contains(&"remembered_fact".to_string()));
    }

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
}
