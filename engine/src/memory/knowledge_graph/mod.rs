//! Knowledge Graph Module
//!
//! Extracts entities and relationships from episodic memory to build
//! a semantic knowledge graph. Enables multi-hop reasoning and discovery
//! of connections between concepts, files, functions, and other entities.

pub mod extractor;
pub mod graph;
pub mod query;

pub use extractor::EntityExtractor;
pub use graph::{GraphNode, GraphEdge, KnowledgeGraph};
pub use query::GraphQuery;

use serde::{Deserialize, Serialize};

/// Entity types in the knowledge graph
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    File,
    Function,
    Class,
    Module,
    Concept,
    Person,
    Tool,
    Error,
    Bug,
    Feature,
    Other(String),
}

impl EntityType {
    pub fn as_str(&self) -> &str {
        match self {
            EntityType::File => "file",
            EntityType::Function => "function",
            EntityType::Class => "class",
            EntityType::Module => "module",
            EntityType::Concept => "concept",
            EntityType::Person => "person",
            EntityType::Tool => "tool",
            EntityType::Error => "error",
            EntityType::Bug => "bug",
            EntityType::Feature => "feature",
            EntityType::Other(s) => s,
        }
    }
}

/// Relationship types between entities
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationType {
    Calls,
    Imports,
    References,
    DependsOn,
    ImplementsFor,
    UsedBy,
    RelatedTo,
    CausedBy,
    FixedBy,
    DocumentedIn,
    Other(String),
}

impl RelationType {
    pub fn as_str(&self) -> &str {
        match self {
            RelationType::Calls => "calls",
            RelationType::Imports => "imports",
            RelationType::References => "references",
            RelationType::DependsOn => "depends_on",
            RelationType::ImplementsFor => "implements",
            RelationType::UsedBy => "used_by",
            RelationType::RelatedTo => "related_to",
            RelationType::CausedBy => "caused_by",
            RelationType::FixedBy => "fixed_by",
            RelationType::DocumentedIn => "documented_in",
            RelationType::Other(s) => s,
        }
    }
}

/// Extracted entity from memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub label: String,
    pub entity_type: EntityType,
    pub properties: serde_json::Value,
}

/// Extracted relationship between entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub from_label: String,
    pub to_label: String,
    pub relation: RelationType,
    pub weight: f32,
}

/// Result of entity extraction from a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
}
