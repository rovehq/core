//! Entity Extraction from Episodic Memory
//!
//! Extracts entities and relationships from memory entries using LLM

use super::{Entity, EntityType, ExtractionResult, RelationType, Relationship};
use crate::conductor::types::GraphSourceKind;
use crate::llm::router::LLMRouter;
use crate::llm::Message;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, warn};

/// LLM prompt for entity extraction
const EXTRACTION_SYSTEM_PROMPT: &str = r#"You extract entities and relationships from text.
Return JSON only. No markdown fences. No preamble.

{
  "entities": [
    {
      "label": "entity name",
      "entity_type": "file|function|class|module|project|concept|person|tool|error|bug|feature",
      "properties": {}
    }
  ],
  "relationships": [
    {
      "from_label": "entity1",
      "to_label": "entity2",
      "relation": "calls|imports|references|depends_on|implements|works_on|stored_at|uses|used_by|related_to|caused_by|fixed_by|documented_in",
      "weight": 0.8
    }
  ]
}

Entity types:
  file      - source files, documents
  function  - functions, methods
  class     - classes, types
  module    - packages, modules
  project   - repositories, apps, named projects
  concept   - abstract concepts, patterns
  person    - people, users, teams
  tool      - tools, libraries, frameworks
  error     - errors, exceptions
  bug       - bugs, issues
  feature   - features, capabilities

Relationship types:
  calls         - function A calls function B
  imports       - module A imports module B
  references    - entity A references entity B
  depends_on    - entity A depends on entity B
  implements    - class A implements interface B
  works_on      - person A works on project B
  stored_at     - project A is stored at path B
  uses          - project or file A uses tool B
  used_by       - tool A is used by entity B
  related_to    - general relationship
  caused_by     - error A caused by entity B
  fixed_by      - bug A fixed by entity B
  documented_in - entity A documented in file B

Weight guide:
  1.0 = direct, strong relationship
  0.7 = indirect relationship
  0.5 = weak or inferred relationship

Rules:
  Only extract entities explicitly mentioned in the text.
  Only extract relationships that are clearly stated or strongly implied.
  Zero entities/relationships is valid if text is not informative.
  Use "concept" for abstract ideas, patterns, or architectural concepts.
"#;

/// Structured extraction from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LLMExtraction {
    entities: Vec<LLMEntity>,
    relationships: Vec<LLMRelationship>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LLMEntity {
    label: String,
    entity_type: String,
    #[serde(default)]
    properties: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LLMRelationship {
    from_label: String,
    to_label: String,
    relation: String,
    #[serde(default = "default_weight")]
    weight: f32,
}

fn default_weight() -> f32 {
    0.7
}

/// Entity extractor using LLM
pub struct EntityExtractor {
    router: Arc<LLMRouter>,
}

impl EntityExtractor {
    pub fn new(router: Arc<LLMRouter>) -> Self {
        Self { router }
    }

    /// Extract entities and relationships from text
    pub async fn extract(&self, text: &str) -> Result<ExtractionResult> {
        // Skip extraction for very short text
        if text.len() < 20 {
            return Ok(ExtractionResult {
                entities: vec![],
                relationships: vec![],
            });
        }

        // Call LLM for extraction
        let messages = vec![
            Message::system(EXTRACTION_SYSTEM_PROMPT),
            Message::user(text),
        ];

        let (response, _provider) = match self.router.call(&messages).await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "entity extraction LLM call failed");
                return Ok(ExtractionResult {
                    entities: vec![],
                    relationships: vec![],
                });
            }
        };

        // Extract content from LLMResponse enum
        let content = match response {
            crate::llm::LLMResponse::FinalAnswer(answer) => answer.content,
            crate::llm::LLMResponse::ToolCall(_) => {
                warn!("unexpected tool call response from entity extraction");
                return Ok(ExtractionResult {
                    entities: vec![],
                    relationships: vec![],
                });
            }
        };

        // Parse LLM response
        let extraction: LLMExtraction = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, response = %content, "failed to parse extraction JSON");
                return Ok(ExtractionResult {
                    entities: vec![],
                    relationships: vec![],
                });
            }
        };

        debug!(
            entities = extraction.entities.len(),
            relationships = extraction.relationships.len(),
            "extracted entities and relationships"
        );

        // Convert to our types
        let entities = extraction
            .entities
            .into_iter()
            .map(|e| Entity {
                label: e.label,
                entity_type: parse_entity_type(&e.entity_type),
                properties: e.properties,
            })
            .collect();

        let relationships = extraction
            .relationships
            .into_iter()
            .map(|r| Relationship {
                from_label: r.from_label,
                to_label: r.to_label,
                relation: parse_relation_type(&r.relation),
                weight: r.weight.clamp(0.0, 1.0),
            })
            .collect();

        Ok(ExtractionResult {
            entities,
            relationships,
        })
    }
}

fn parse_entity_type(s: &str) -> EntityType {
    match s.to_lowercase().as_str() {
        "file" => EntityType::File,
        "function" => EntityType::Function,
        "class" => EntityType::Class,
        "module" => EntityType::Module,
        "project" => EntityType::Project,
        "concept" => EntityType::Concept,
        "person" => EntityType::Person,
        "tool" => EntityType::Tool,
        "error" => EntityType::Error,
        "bug" => EntityType::Bug,
        "feature" => EntityType::Feature,
        other => EntityType::Other(other.to_string()),
    }
}

fn parse_relation_type(s: &str) -> RelationType {
    match s.to_lowercase().as_str() {
        "calls" => RelationType::Calls,
        "imports" => RelationType::Imports,
        "references" => RelationType::References,
        "depends_on" => RelationType::DependsOn,
        "implements" => RelationType::ImplementsFor,
        "works_on" => RelationType::WorksOn,
        "stored_at" => RelationType::StoredAt,
        "uses" => RelationType::Uses,
        "used_by" => RelationType::UsedBy,
        "related_to" => RelationType::RelatedTo,
        "caused_by" => RelationType::CausedBy,
        "fixed_by" => RelationType::FixedBy,
        "documented_in" => RelationType::DocumentedIn,
        other => RelationType::Other(other.to_string()),
    }
}

/// Extract entities and relationships from task result and store in knowledge graph
///
/// This function is called fire-and-forget after task completion.
/// Failures are logged but don't affect task success.
///
/// # Arguments
///
/// * `db` - Database connection pool
/// * `router` - LLM router for extraction
/// * `task_result` - The completed task result
/// * `task_id` - The task ID
///
/// # Returns
///
/// Ok(()) on success, Err on failure (logged, not propagated)
pub async fn extract_and_store(
    db: &crate::db::Database,
    router: &Arc<LLMRouter>,
    task_result: &str,
    task_id: &str,
) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Create extractor
    let extractor = EntityExtractor::new(Arc::clone(router));

    // Extract entities and relationships
    let extraction = extractor.extract(task_result).await?;

    // If nothing extracted, return early
    if extraction.entities.is_empty() && extraction.relationships.is_empty() {
        debug!(task_id = %task_id, "no entities or relationships extracted");
        return Ok(());
    }

    // Get knowledge graph instance
    let graph = crate::knowledge_graph::KnowledgeGraph::new(db.pool().clone());
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

    // Store entities
    for entity in &extraction.entities {
        let node_id = uuid::Uuid::new_v4().to_string();
        let node = crate::knowledge_graph::GraphNode {
            id: node_id,
            label: entity.label.clone(),
            node_type: entity.entity_type.clone(),
            properties: entity.properties.clone(),
            source_kind: GraphSourceKind::LlmInferred,
            source_scope: "per_node".to_string(),
            source_ref: Some(task_id.to_string()),
            confidence: 0.7,
            created_at: now,
            last_updated: now,
            access_count: 0,
        };
        graph
            .upsert_node(&node)
            .await
            .context("Failed to upsert graph node")?;
    }

    // Store relationships
    for rel in &extraction.relationships {
        // Find node IDs by label (simplified - in production, use proper lookup)
        // For now, we'll skip relationships if nodes don't exist
        // TODO: Implement proper node lookup by label
        debug!(
            from = %rel.from_label,
            to = %rel.to_label,
            relation = %rel.relation.as_str(),
            "relationship extracted (storage TODO)"
        );
    }

    debug!(
        task_id = %task_id,
        entities = extraction.entities.len(),
        relationships = extraction.relationships.len(),
        "knowledge graph extraction complete"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_entity_type() {
        assert_eq!(parse_entity_type("file"), EntityType::File);
        assert_eq!(parse_entity_type("FILE"), EntityType::File);
        assert_eq!(parse_entity_type("concept"), EntityType::Concept);
        assert_eq!(
            parse_entity_type("unknown"),
            EntityType::Other("unknown".to_string())
        );
    }

    #[test]
    fn test_parse_relation_type() {
        assert_eq!(parse_relation_type("calls"), RelationType::Calls);
        assert_eq!(parse_relation_type("CALLS"), RelationType::Calls);
        assert_eq!(parse_relation_type("depends_on"), RelationType::DependsOn);
        assert_eq!(
            parse_relation_type("unknown"),
            RelationType::Other("unknown".to_string())
        );
    }

    #[test]
    fn test_default_weight() {
        assert_eq!(default_weight(), 0.7);
    }
}
