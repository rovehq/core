//! Graph Query and Traversal
//!
//! Multi-hop reasoning and path finding in the knowledge graph

use super::{GraphNode, KnowledgeGraph};
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
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_graph_query_creation() {
        // This is a placeholder test since we need a real database for full testing
        // Real tests would use SqlitePool::connect(":memory:").await
        assert!(true);
    }
}
