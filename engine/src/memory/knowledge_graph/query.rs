//! Graph Query and Traversal
//!
//! Multi-hop reasoning and path finding in the knowledge graph

use super::{GraphNode, KnowledgeGraph, RelationType};
use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Graph query builder
pub struct GraphQuery {
    graph: KnowledgeGraph,
}

impl GraphQuery {
    pub fn new(graph: KnowledgeGraph) -> Self {
        Self { graph }
    }

    /// Find shortest path between two nodes using BFS
    ///
    /// # Arguments
    /// * `from` - Starting node ID
    /// * `to` - Target node ID
    ///
    /// # Returns
    /// Vector of nodes representing the shortest path, or empty if no path exists
    pub async fn shortest_path(&self, from: &str, to: &str) -> Result<Vec<GraphNode>> {
        // BFS to find shortest path
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut parent: HashMap<String, String> = HashMap::new();

        queue.push_back(from.to_string());
        visited.insert(from.to_string());

        while let Some(current) = queue.pop_front() {
            if current == to {
                // Found target, reconstruct path
                return self.reconstruct_path(&parent, from, to).await;
            }

            // Get outgoing edges
            let edges = self.graph.get_outgoing_edges(&current).await?;

            for edge in edges {
                if !visited.contains(&edge.to_id) {
                    visited.insert(edge.to_id.clone());
                    parent.insert(edge.to_id.clone(), current.clone());
                    queue.push_back(edge.to_id);
                }
            }
        }

        // No path found
        Ok(vec![])
    }

    /// Reconstruct path from parent map
    async fn reconstruct_path(
        &self,
        parent: &HashMap<String, String>,
        from: &str,
        to: &str,
    ) -> Result<Vec<GraphNode>> {
        let mut path = Vec::new();
        let mut current = to.to_string();

        // Build path backwards
        let mut node_ids = vec![current.clone()];
        while current != from {
            if let Some(prev) = parent.get(&current) {
                node_ids.push(prev.clone());
                current = prev.clone();
            } else {
                break;
            }
        }

        // Reverse to get forward path
        node_ids.reverse();

        // Fetch nodes
        for node_id in node_ids {
            if let Some(node) = self.graph.get_node(&node_id).await? {
                path.push(node);
            }
        }

        Ok(path)
    }

    /// Find all nodes within N hops of a starting node using BFS
    ///
    /// # Arguments
    /// * `node_id` - Starting node ID
    /// * `max_hops` - Maximum number of hops to traverse
    ///
    /// # Returns
    /// Vector of nodes within max_hops distance
    pub async fn neighbors(&self, node_id: &str, max_hops: usize) -> Result<Vec<GraphNode>> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut result = Vec::new();

        // (node_id, hop_count)
        queue.push_back((node_id.to_string(), 0));
        visited.insert(node_id.to_string());

        while let Some((current, hops)) = queue.pop_front() {
            // Fetch current node
            if let Some(node) = self.graph.get_node(&current).await? {
                result.push(node);
            }

            // Stop if we've reached max hops
            if hops >= max_hops {
                continue;
            }

            // Get outgoing edges
            let edges = self.graph.get_outgoing_edges(&current).await?;

            for edge in edges {
                if !visited.contains(&edge.to_id) {
                    visited.insert(edge.to_id.clone());
                    queue.push_back((edge.to_id, hops + 1));
                }
            }
        }

        Ok(result)
    }

    /// Find related concepts using graph traversal
    ///
    /// Finds nodes connected to the given concept through "related_to" edges,
    /// or nodes that share common neighbors.
    ///
    /// # Arguments
    /// * `concept` - Concept label to search for
    ///
    /// # Returns
    /// Vector of related concept nodes
    pub async fn related_concepts(&self, concept: &str) -> Result<Vec<GraphNode>> {
        // First, find nodes matching the concept label
        let matching_nodes = self.graph.search_nodes(concept).await?;

        if matching_nodes.is_empty() {
            return Ok(vec![]);
        }

        let mut related = Vec::new();
        let mut seen = HashSet::new();

        // For each matching node, find its neighbors
        for node in matching_nodes {
            seen.insert(node.id.clone());

            // Get outgoing edges
            let outgoing = self.graph.get_outgoing_edges(&node.id).await?;
            for edge in outgoing {
                if !seen.contains(&edge.to_id) {
                    if let Some(related_node) = self.graph.get_node(&edge.to_id).await? {
                        seen.insert(related_node.id.clone());
                        related.push(related_node);
                    }
                }
            }

            // Get incoming edges
            let incoming = self.graph.get_incoming_edges(&node.id).await?;
            for edge in incoming {
                if !seen.contains(&edge.from_id) {
                    if let Some(related_node) = self.graph.get_node(&edge.from_id).await? {
                        seen.insert(related_node.id.clone());
                        related.push(related_node);
                    }
                }
            }
        }

        Ok(related)
    }

    /// Build short fact strings from graph relationships that match a question.
    pub async fn related_facts(&self, question: &str, limit: usize) -> Result<Vec<String>> {
        let mut facts = Vec::new();
        let mut seen_nodes = HashSet::new();
        let mut seen_facts = HashSet::new();

        for term in question_terms(question) {
            let nodes = self.graph.search_nodes(&term).await?;
            for node in nodes {
                if !seen_nodes.insert(node.id.clone()) {
                    continue;
                }

                let outgoing = self.graph.get_outgoing_edges(&node.id).await?;
                for edge in outgoing {
                    if let Some(target) = self.graph.get_node(&edge.to_id).await? {
                        let fact = format_relationship(&node, &target, &edge.relation);
                        if seen_facts.insert(fact.clone()) {
                            facts.push(fact);
                            if facts.len() >= limit {
                                return Ok(facts);
                            }
                        }
                    }
                }

                let incoming = self.graph.get_incoming_edges(&node.id).await?;
                for edge in incoming {
                    if let Some(source) = self.graph.get_node(&edge.from_id).await? {
                        let fact = format_relationship(&source, &node, &edge.relation);
                        if seen_facts.insert(fact.clone()) {
                            facts.push(fact);
                            if facts.len() >= limit {
                                return Ok(facts);
                            }
                        }
                    }
                }
            }
        }

        Ok(facts)
    }
}

fn question_terms(question: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "what", "where", "when", "which", "that", "this", "with", "from", "into", "your", "mine",
        "about", "stored", "store", "using", "work", "project", "please", "remember", "there",
    ];

    let mut seen = HashSet::new();
    let mut terms = Vec::new();
    for token in question
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '/' && ch != '~')
        .filter(|token| token.len() >= 3)
    {
        let term = token.to_ascii_lowercase();
        if STOP_WORDS.contains(&term.as_str()) || !seen.insert(term.clone()) {
            continue;
        }
        terms.push(term);
    }

    terms
}

fn format_relationship(from: &GraphNode, to: &GraphNode, relation: &RelationType) -> String {
    match relation {
        RelationType::WorksOn => format!("{} works on {}", from.label, to.label),
        RelationType::StoredAt => format!("{} is stored at {}", from.label, to.label),
        RelationType::Uses => format!("{} uses {}", from.label, to.label),
        RelationType::UsedBy => format!("{} is used by {}", from.label, to.label),
        _ => format!("{} {} {}", from.label, relation.as_str(), to.label),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_relationship, question_terms, GraphQuery};
    use crate::conductor::types::GraphSourceKind;
    use crate::memory::knowledge_graph::{EntityType, GraphNode, RelationType};

    #[test]
    fn test_graph_query_creation() {
        assert!(std::any::type_name::<GraphQuery>().contains("GraphQuery"));
    }

    #[test]
    fn test_question_terms_filters_noise() {
        let terms = question_terms("where is my rove project stored");
        assert_eq!(terms, vec!["rove"]);
    }

    #[test]
    fn test_format_relationship_uses_human_text() {
        let from = GraphNode {
            id: "project:rove".to_string(),
            label: "rove".to_string(),
            node_type: EntityType::Project,
            properties: serde_json::json!({}),
            source_kind: GraphSourceKind::Deterministic,
            source_scope: "per_node".to_string(),
            source_ref: None,
            confidence: 1.0,
            created_at: 0,
            last_updated: 0,
            access_count: 0,
        };
        let to = GraphNode {
            id: "file:workspace_rove".to_string(),
            label: "~/workspace/rove".to_string(),
            node_type: EntityType::File,
            properties: serde_json::json!({}),
            source_kind: GraphSourceKind::Deterministic,
            source_scope: "per_node".to_string(),
            source_ref: None,
            confidence: 1.0,
            created_at: 0,
            last_updated: 0,
            access_count: 0,
        };

        assert_eq!(
            format_relationship(&from, &to, &RelationType::StoredAt),
            "rove is stored at ~/workspace/rove"
        );
    }
}
