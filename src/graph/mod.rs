//! Knowledge graph operations for analyzing note connections.
//!
//! This module builds and analyzes a directed graph of note links
//! using petgraph, providing backlink detection, hub identification,
//! and orphan detection.

use std::collections::HashMap;

use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

use crate::vault::Vault;

/// A knowledge graph built from vault links.
#[derive(Debug, Clone)]
pub struct KnowledgeGraph {
    /// The directed graph of note connections.
    graph: DiGraph<String, ()>,
    /// Mapping from note name (lowercase) to node index.
    node_indices: HashMap<String, NodeIndex>,
}

impl KnowledgeGraph {
    /// Build a knowledge graph from a vault.
    pub fn from_vault(vault: &Vault) -> Self {
        let mut graph = DiGraph::new();
        let mut node_indices = HashMap::new();

        // First pass: add all notes as nodes
        for note in vault.notes() {
            let idx = graph.add_node(note.name.clone());
            node_indices.insert(note.name.to_lowercase(), idx);
        }

        // Second pass: add edges for links
        for note in vault.notes() {
            if let Some(&source_idx) = node_indices.get(&note.name.to_lowercase()) {
                for link in &note.links {
                    // Skip same-file links
                    if link.target.is_empty() {
                        continue;
                    }

                    // Only add edge if target exists
                    if let Some(&target_idx) = node_indices.get(&link.target.to_lowercase()) {
                        // Avoid self-loops
                        if source_idx != target_idx {
                            graph.add_edge(source_idx, target_idx, ());
                        }
                    }
                }
            }
        }

        Self {
            graph,
            node_indices,
        }
    }

    /// Get the number of notes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Get the number of links in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Get notes that link TO the given note (backlinks).
    pub fn backlinks(&self, note_name: &str) -> Vec<String> {
        self.neighbors(note_name, Direction::Incoming)
    }

    /// Get notes that the given note links TO (outgoing links).
    #[allow(dead_code)]
    pub fn outgoing_links(&self, note_name: &str) -> Vec<String> {
        self.neighbors(note_name, Direction::Outgoing)
    }

    /// Get neighbors in a specific direction.
    fn neighbors(&self, note_name: &str, direction: Direction) -> Vec<String> {
        let key = note_name.to_lowercase();
        if let Some(&idx) = self.node_indices.get(&key) {
            self.graph
                .neighbors_directed(idx, direction)
                .map(|neighbor_idx| self.graph[neighbor_idx].clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get the degree (total connections) for a note.
    #[allow(dead_code)]
    pub fn degree(&self, note_name: &str) -> usize {
        let key = note_name.to_lowercase();
        if let Some(&idx) = self.node_indices.get(&key) {
            self.graph.neighbors_undirected(idx).count()
        } else {
            0
        }
    }

    /// Get notes with no connections (orphans).
    pub fn orphans(&self) -> Vec<String> {
        self.graph
            .node_indices()
            .filter(|&idx| self.graph.neighbors_undirected(idx).count() == 0)
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    /// Get the most connected notes (hubs), sorted by connection count.
    pub fn hubs(&self, limit: usize) -> Vec<(String, usize)> {
        let mut nodes_with_degree: Vec<_> = self
            .graph
            .node_indices()
            .map(|idx| {
                let name = self.graph[idx].clone();
                let degree = self.graph.neighbors_undirected(idx).count();
                (name, degree)
            })
            .collect();

        nodes_with_degree.sort_by(|a, b| b.1.cmp(&a.1));
        nodes_with_degree.truncate(limit);
        nodes_with_degree
    }

    /// Get graph statistics.
    pub fn stats(&self) -> GraphStats {
        let orphans = self.orphans();
        let hubs = self.hubs(10);

        GraphStats {
            total_notes: self.node_count(),
            total_links: self.edge_count(),
            orphan_count: orphans.len(),
            orphan_notes: orphans,
            hub_notes: hubs,
        }
    }
}

/// Statistics about the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    /// Total number of notes.
    pub total_notes: usize,
    /// Total number of links (edges).
    pub total_links: usize,
    /// Number of orphan notes (no connections).
    pub orphan_count: usize,
    /// List of orphan note names.
    pub orphan_notes: Vec<String>,
    /// Most connected notes (name, connection count).
    pub hub_notes: Vec<(String, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Vault;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn create_test_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempdir().unwrap();

        let notes = [
            (
                "Hub Note.md",
                "# Hub Note\n\nLinks to [[Note A]], [[Note B]], and [[Note C]].",
            ),
            ("Note A.md", "# Note A\n\nLinks back to [[Hub Note]]."),
            (
                "Note B.md",
                "# Note B\n\nLinks to [[Note A]] and [[Hub Note]].",
            ),
            ("Note C.md", "# Note C\n\nLinks to [[Hub Note]]."),
            ("Orphan.md", "# Orphan\n\nNo links here."),
        ];

        for (name, content) in notes {
            let path = dir.path().join(name);
            let mut file = File::create(&path).unwrap();
            file.write_all(content.as_bytes()).unwrap();
        }

        let mut vault = Vault::new(dir.path());
        vault.index().unwrap();

        (dir, vault)
    }

    #[test]
    fn test_graph_from_vault() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        assert_eq!(graph.node_count(), 5);
        assert!(graph.edge_count() > 0);
    }

    #[test]
    fn test_graph_backlinks() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        let backlinks = graph.backlinks("Hub Note");
        assert_eq!(backlinks.len(), 3); // Note A, Note B, Note C all link to Hub
    }

    #[test]
    fn test_graph_outgoing_links() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        let outgoing = graph.outgoing_links("Hub Note");
        assert_eq!(outgoing.len(), 3); // Hub links to A, B, C
    }

    #[test]
    fn test_graph_orphans() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        let orphans = graph.orphans();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0], "Orphan");
    }

    #[test]
    fn test_graph_hubs() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        let hubs = graph.hubs(3);
        assert!(!hubs.is_empty());

        // Hub Note should be the most connected
        assert_eq!(hubs[0].0, "Hub Note");
    }

    #[test]
    fn test_graph_degree() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        // Hub Note: 3 outgoing + 3 incoming = 6 connections (but undirected counts unique neighbors)
        let degree = graph.degree("Hub Note");
        assert!(degree >= 3);

        // Orphan: 0 connections
        let orphan_degree = graph.degree("Orphan");
        assert_eq!(orphan_degree, 0);
    }

    #[test]
    fn test_graph_stats() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        let stats = graph.stats();
        assert_eq!(stats.total_notes, 5);
        assert!(stats.total_links > 0);
        assert_eq!(stats.orphan_count, 1);
        assert_eq!(stats.orphan_notes, vec!["Orphan"]);
    }

    #[test]
    fn test_graph_case_insensitive() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        // Should work with different cases
        let backlinks1 = graph.backlinks("Hub Note");
        let backlinks2 = graph.backlinks("hub note");
        let backlinks3 = graph.backlinks("HUB NOTE");

        assert_eq!(backlinks1.len(), backlinks2.len());
        assert_eq!(backlinks2.len(), backlinks3.len());
    }

    #[test]
    fn test_graph_nonexistent_note() {
        let (_dir, vault) = create_test_vault();
        let graph = KnowledgeGraph::from_vault(&vault);

        let backlinks = graph.backlinks("Does Not Exist");
        assert!(backlinks.is_empty());

        let outgoing = graph.outgoing_links("Does Not Exist");
        assert!(outgoing.is_empty());
    }
}
