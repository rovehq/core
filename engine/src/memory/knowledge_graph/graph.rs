//! Knowledge Graph Storage and Management

use super::{EntityType, RelationType};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

use crate::conductor::types::GraphSourceKind;

/// A node in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub node_type: EntityType,
    pub properties: serde_json::Value,
    pub source_kind: GraphSourceKind,
    pub source_scope: String,
    pub source_ref: Option<String>,
    pub confidence: f32,
    pub created_at: i64,
    pub last_updated: i64,
    pub access_count: i64,
}

/// An edge in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub from_id: String,
    pub to_id: String,
    pub relation: RelationType,
    pub weight: f32,
    pub properties: Option<serde_json::Value>,
    pub source_kind: GraphSourceKind,
    pub source_scope: String,
    pub source_ref: Option<String>,
    pub confidence: f32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Knowledge graph manager
#[derive(Clone)]
pub struct KnowledgeGraph {
    pool: SqlitePool,
}

impl KnowledgeGraph {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn canonical_node_id(node_type: &EntityType, label: &str) -> String {
        let normalized = normalize_identifier(label);
        format!("{}:{}", node_type.as_str(), normalized)
    }

    pub fn canonical_edge_id(from_id: &str, relation: &RelationType, to_id: &str) -> String {
        format!("{}:{}:{}", from_id, relation.as_str(), to_id)
    }

    /// Add or update a node in the graph
    pub async fn upsert_node(&self, node: &GraphNode) -> Result<()> {
        let properties_json = serde_json::to_string(&node.properties)?;
        let node_type = node.node_type.as_str();

        sqlx::query(
            r#"
            INSERT INTO graph_nodes (
                id, label, type, properties, source_kind, source_scope, source_ref, confidence,
                created_at, last_updated, access_count
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                label = excluded.label,
                type = excluded.type,
                properties = excluded.properties,
                source_kind = excluded.source_kind,
                source_scope = excluded.source_scope,
                source_ref = excluded.source_ref,
                confidence = excluded.confidence,
                last_updated = excluded.last_updated
            "#,
        )
        .bind(&node.id)
        .bind(&node.label)
        .bind(node_type)
        .bind(&properties_json)
        .bind(node.source_kind.as_str())
        .bind(&node.source_scope)
        .bind(&node.source_ref)
        .bind(node.confidence)
        .bind(node.created_at)
        .bind(node.last_updated)
        .bind(node.access_count)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Link an episodic memory to a graph node.
    pub async fn link_memory(
        &self,
        memory_id: &str,
        node_id: &str,
        relevance: f32,
        created_at: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO memory_graph_links (memory_id, node_id, relevance, created_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(memory_id, node_id) DO UPDATE SET
                relevance = excluded.relevance
            "#,
        )
        .bind(memory_id)
        .bind(node_id)
        .bind(relevance)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Add an edge between two nodes
    pub async fn add_edge(&self, edge: &GraphEdge) -> Result<()> {
        let properties_json = edge
            .properties
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let relation = edge.relation.as_str();

        sqlx::query(
            r#"
            INSERT INTO graph_edges (
                id, from_id, to_id, relation, weight, properties, source_kind, source_scope,
                source_ref, confidence, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                weight = excluded.weight,
                properties = excluded.properties,
                source_kind = excluded.source_kind,
                source_scope = excluded.source_scope,
                source_ref = excluded.source_ref,
                confidence = excluded.confidence,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&edge.id)
        .bind(&edge.from_id)
        .bind(&edge.to_id)
        .bind(relation)
        .bind(edge.weight)
        .bind(properties_json)
        .bind(edge.source_kind.as_str())
        .bind(&edge.source_scope)
        .bind(&edge.source_ref)
        .bind(edge.confidence)
        .bind(edge.created_at)
        .bind(edge.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a node by ID
    pub async fn get_node(&self, id: &str) -> Result<Option<GraphNode>> {
        let row = sqlx::query(
            r#"
            SELECT id, label, type, properties, source_kind, source_scope, source_ref, confidence,
                   created_at, last_updated, access_count
            FROM graph_nodes
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let properties: serde_json::Value = serde_json::from_str(row.get("properties"))?;
            let type_str: String = row.get("type");
            let node_type = parse_entity_type(&type_str);

            Ok(Some(GraphNode {
                id: row.get("id"),
                label: row.get("label"),
                node_type,
                properties,
                source_kind: parse_source_kind(row.get("source_kind")),
                source_scope: row.get("source_scope"),
                source_ref: row.get("source_ref"),
                confidence: row.get("confidence"),
                created_at: row.get("created_at"),
                last_updated: row.get("last_updated"),
                access_count: row.get("access_count"),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get graph statistics
    pub async fn get_stats(&self) -> Result<HashMap<String, i64>> {
        let node_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM graph_nodes")
            .fetch_one(&self.pool)
            .await?;

        let edge_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM graph_edges")
            .fetch_one(&self.pool)
            .await?;

        let mut stats = HashMap::new();
        stats.insert("nodes".to_string(), node_count);
        stats.insert("edges".to_string(), edge_count);

        Ok(stats)
    }

    /// Get all outgoing edges from a node
    pub async fn get_outgoing_edges(&self, node_id: &str) -> Result<Vec<GraphEdge>> {
        let rows = sqlx::query(
            r#"
            SELECT id, from_id, to_id, relation, weight, properties, source_kind, source_scope,
                   source_ref, confidence, created_at, updated_at
            FROM graph_edges
            WHERE from_id = ?
            "#,
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;

        let mut edges = Vec::new();
        for row in rows {
            let properties_str: Option<String> = row.get("properties");
            let properties = properties_str.and_then(|s| serde_json::from_str(&s).ok());
            let relation_str: String = row.get("relation");
            let relation = parse_relation_type(&relation_str);

            edges.push(GraphEdge {
                id: row.get("id"),
                from_id: row.get("from_id"),
                to_id: row.get("to_id"),
                relation,
                weight: row.get("weight"),
                properties,
                source_kind: parse_source_kind(row.get("source_kind")),
                source_scope: row.get("source_scope"),
                source_ref: row.get("source_ref"),
                confidence: row.get("confidence"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }

        Ok(edges)
    }

    /// Get all incoming edges to a node
    pub async fn get_incoming_edges(&self, node_id: &str) -> Result<Vec<GraphEdge>> {
        let rows = sqlx::query(
            r#"
            SELECT id, from_id, to_id, relation, weight, properties, source_kind, source_scope,
                   source_ref, confidence, created_at, updated_at
            FROM graph_edges
            WHERE to_id = ?
            "#,
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;

        let mut edges = Vec::new();
        for row in rows {
            let properties_str: Option<String> = row.get("properties");
            let properties = properties_str.and_then(|s| serde_json::from_str(&s).ok());
            let relation_str: String = row.get("relation");
            let relation = parse_relation_type(&relation_str);

            edges.push(GraphEdge {
                id: row.get("id"),
                from_id: row.get("from_id"),
                to_id: row.get("to_id"),
                relation,
                weight: row.get("weight"),
                properties,
                source_kind: parse_source_kind(row.get("source_kind")),
                source_scope: row.get("source_scope"),
                source_ref: row.get("source_ref"),
                confidence: row.get("confidence"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }

        Ok(edges)
    }

    /// Search nodes by label (fuzzy match)
    pub async fn search_nodes(&self, query: &str) -> Result<Vec<GraphNode>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query(
            r#"
            SELECT id, label, type, properties, source_kind, source_scope, source_ref, confidence,
                   created_at, last_updated, access_count
            FROM graph_nodes
            WHERE label LIKE ?
            LIMIT 20
            "#,
        )
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        let mut nodes = Vec::new();
        for row in rows {
            let properties: serde_json::Value = serde_json::from_str(row.get("properties"))?;
            let type_str: String = row.get("type");
            let node_type = parse_entity_type(&type_str);

            nodes.push(GraphNode {
                id: row.get("id"),
                label: row.get("label"),
                node_type,
                properties,
                source_kind: parse_source_kind(row.get("source_kind")),
                source_scope: row.get("source_scope"),
                source_ref: row.get("source_ref"),
                confidence: row.get("confidence"),
                created_at: row.get("created_at"),
                last_updated: row.get("last_updated"),
                access_count: row.get("access_count"),
            });
        }

        Ok(nodes)
    }

    /// Look up a node by label using a case-insensitive exact match first.
    pub async fn find_node_by_label(&self, label: &str) -> Result<Option<GraphNode>> {
        let row = sqlx::query(
            r#"
            SELECT id, label, type, properties, source_kind, source_scope, source_ref, confidence,
                   created_at, last_updated, access_count
            FROM graph_nodes
            WHERE lower(label) = lower(?)
            ORDER BY last_updated DESC
            LIMIT 1
            "#,
        )
        .bind(label)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let properties: serde_json::Value = serde_json::from_str(row.get("properties"))?;
            let type_str: String = row.get("type");
            let node_type = parse_entity_type(&type_str);

            return Ok(Some(GraphNode {
                id: row.get("id"),
                label: row.get("label"),
                node_type,
                properties,
                source_kind: parse_source_kind(row.get("source_kind")),
                source_scope: row.get("source_scope"),
                source_ref: row.get("source_ref"),
                confidence: row.get("confidence"),
                created_at: row.get("created_at"),
                last_updated: row.get("last_updated"),
                access_count: row.get("access_count"),
            }));
        }

        Ok(None)
    }
}

fn parse_entity_type(s: &str) -> EntityType {
    match s {
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
    match s {
        "calls" => RelationType::Calls,
        "imports" => RelationType::Imports,
        "references" => RelationType::References,
        "depends_on" => RelationType::DependsOn,
        "implements" => RelationType::ImplementsFor,
        "contains" => RelationType::Contains,
        "tested_by" => RelationType::TestedBy,
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

fn parse_source_kind(value: String) -> GraphSourceKind {
    GraphSourceKind::from_str(&value)
}

fn normalize_identifier(label: &str) -> String {
    let mut normalized = String::with_capacity(label.len());
    let mut last_was_sep = false;

    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            normalized.push('_');
            last_was_sep = true;
        }
    }

    normalized.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_type_as_str() {
        assert_eq!(EntityType::File.as_str(), "file");
        assert_eq!(EntityType::Function.as_str(), "function");
        assert_eq!(EntityType::Project.as_str(), "project");
        assert_eq!(EntityType::Concept.as_str(), "concept");
    }

    #[test]
    fn test_relation_type_as_str() {
        assert_eq!(RelationType::Calls.as_str(), "calls");
        assert_eq!(RelationType::Imports.as_str(), "imports");
        assert_eq!(RelationType::StoredAt.as_str(), "stored_at");
        assert_eq!(RelationType::RelatedTo.as_str(), "related_to");
    }

    #[test]
    fn test_canonical_ids_are_stable() {
        let node_id = KnowledgeGraph::canonical_node_id(&EntityType::Project, "Rove Project");
        assert_eq!(node_id, "project:rove_project");

        let edge_id = KnowledgeGraph::canonical_edge_id(
            "project:rove_project",
            &RelationType::StoredAt,
            "file:workspace_rove",
        );
        assert_eq!(
            edge_id,
            "project:rove_project:stored_at:file:workspace_rove"
        );
    }
}
