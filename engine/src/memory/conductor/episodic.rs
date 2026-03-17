//! Episodic Memory Ingest
//!
//! Handles ingestion of completed tasks into episodic memory.
//! Calls LLM to extract structured information and stores in episodic_memory table.
//! Includes fire-and-forget knowledge graph extraction for high-importance memories.

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::conductor::memory_types::*;
use crate::conductor::memory_utils::*;
use crate::conductor::types::TaskDomain;
use crate::llm::router::LLMRouter;

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
    let memory_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Build the prompt
    let content = format!(
        "{}INPUT:\n{}\n\nRESULT:\n{}",
        crate::conductor::memory_prompts::INGEST_PROMPT,
        task_input,
        task_result
    );

    // Call LLM with a 30s timeout
    let extraction = match call_llm_for_text(router, &content).await {
        Ok(text) => parse_ingest_response(&text),
        Err(e) => {
            warn!("LLM ingest call failed, storing raw fallback: {}", e);
            None
        }
    };

    let extraction = extraction.unwrap_or_else(|| {
        // Fallback: store a simple raw memory so nothing is lost
        IngestExtraction {
            summary: truncate(task_input, 200),
            entities: vec![],
            topics: vec![],
            importance: 0.3,
        }
    });

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
        let extractor = Arc::clone(entity_extractor);
        let graph = Arc::clone(knowledge_graph);
        let summary = extraction.summary.clone();
        let mem_id = memory_id.clone();

        tokio::spawn(async move {
            match extractor.extract(&summary).await {
                Ok(result) if !result.entities.is_empty() => {
                    let entity_count = result.entities.len();
                    let rel_count = result.relationships.len();

                    // Store entities and relationships in knowledge graph
                    for entity in &result.entities {
                        let node = crate::knowledge_graph::GraphNode {
                            id: format!("{}:{}", entity.entity_type.as_str(), entity.label),
                            label: entity.label.clone(),
                            node_type: entity.entity_type.clone(),
                            properties: entity.properties.clone(),
                            created_at: now,
                            last_updated: now,
                            access_count: 0,
                        };

                        if let Err(e) = graph.upsert_node(&node).await {
                            warn!(error = %e, entity = %entity.label, "failed to store entity");
                        }
                    }

                    for rel in &result.relationships {
                        let edge = crate::knowledge_graph::GraphEdge {
                            id: format!(
                                "{}:{}:{}",
                                rel.from_label,
                                rel.relation.as_str(),
                                rel.to_label
                            ),
                            from_id: format!("{}:{}", "unknown", rel.from_label), // Type unknown, will be resolved
                            to_id: format!("{}:{}", "unknown", rel.to_label),
                            relation: rel.relation.clone(),
                            weight: rel.weight,
                            properties: None,
                            created_at: now,
                        };

                        if let Err(e) = graph.add_edge(&edge).await {
                            warn!(error = %e, from = %rel.from_label, to = %rel.to_label, "failed to store relationship");
                        }
                    }

                    debug!(memory_id = %mem_id, entities = entity_count, relationships = rel_count, "extracted entities for knowledge graph");
                }
                Ok(_) => {
                    debug!(memory_id = %mem_id, "no entities extracted");
                }
                Err(e) => {
                    warn!(error = %e, memory_id = %mem_id, "entity extraction failed (non-fatal)");
                }
            }
        });
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
}
