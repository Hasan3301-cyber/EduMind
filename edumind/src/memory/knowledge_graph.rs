use std::collections::{BTreeMap, BTreeSet};

use edumind_core::{Community, GraphData, GraphEdge, GraphNode};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::infra::Result;

use super::{MemoryRecord, MemoryStore};

const MAX_CONCEPTS_PER_MEMORY: usize = 8;
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "into", "is", "it", "of",
    "on", "or", "that", "the", "this", "to", "was", "were", "with", "your",
];

/// One graph search result with its deterministic lexical overlap score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphSearchHit {
    pub node: GraphNode,
    pub score: f64,
}

/// The local neighborhood around a selected knowledge-graph node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphNeighborhood {
    pub center: GraphNode,
    #[serde(default)]
    pub neighbors: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub community: Option<Community>,
}

/// Builds queryable knowledge-graph views from the durable memory store.
#[derive(Clone)]
pub struct KnowledgeGraphService {
    store: MemoryStore,
}

impl KnowledgeGraphService {
    /// Creates a graph service whose views are always derived from current memory records.
    #[must_use]
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }

    /// Builds the complete local knowledge graph and deterministic connected communities.
    pub fn graph(&self) -> Result<GraphData> {
        Ok(build_knowledge_graph(&self.store.list()?))
    }

    /// Searches graph labels and excerpts using normalized concept overlap.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<GraphSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query_terms = concept_terms(query);
        if query_terms.is_empty() {
            return Ok(Vec::new());
        }
        let graph = self.graph()?;
        let mut hits = graph
            .nodes
            .into_iter()
            .filter_map(|node| {
                let terms = concept_terms(&node.label);
                let overlap = query_terms.intersection(&terms).count();
                let label_match = node
                    .label
                    .to_lowercase()
                    .contains(query.trim().to_lowercase().as_str());
                let score = overlap as f64 / query_terms.len() as f64
                    + if label_match { 0.25 } else { 0.0 };
                (score > 0.0).then_some(GraphSearchHit { node, score })
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.node.id.cmp(&right.node.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    /// Returns direct graph neighbors and the containing community for a node ID.
    pub fn neighbors(&self, node_id: &str) -> Result<Option<GraphNeighborhood>> {
        let graph = self.graph()?;
        let Some(center) = graph.nodes.iter().find(|node| node.id == node_id).cloned() else {
            return Ok(None);
        };
        let mut neighbor_ids = BTreeSet::new();
        let edges = graph
            .edges
            .iter()
            .filter_map(|edge| {
                if edge.source == node_id {
                    neighbor_ids.insert(edge.target.clone());
                    Some(edge.clone())
                } else if edge.target == node_id {
                    neighbor_ids.insert(edge.source.clone());
                    Some(edge.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let neighbors = graph
            .nodes
            .into_iter()
            .filter(|node| neighbor_ids.contains(&node.id))
            .collect::<Vec<_>>();
        let community = graph
            .communities
            .into_iter()
            .find(|community| community.node_ids.iter().any(|id| id == node_id));
        Ok(Some(GraphNeighborhood {
            center,
            neighbors,
            edges,
            community,
        }))
    }
}

/// Extracts stable, non-stopword concepts from a memory body for graph and wiki generation.
#[must_use]
pub fn extract_concepts(content: &str) -> Vec<String> {
    let mut frequencies = BTreeMap::<String, usize>::new();
    for term in normalized_terms(content) {
        *frequencies.entry(term).or_insert(0) += 1;
    }
    let mut ranked = frequencies.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked
        .into_iter()
        .take(MAX_CONCEPTS_PER_MEMORY)
        .map(|(concept, _)| concept)
        .collect()
}

/// Creates a deterministic graph of memory excerpts, extracted concepts, and co-occurrence links.
#[must_use]
pub fn build_knowledge_graph(records: &[MemoryRecord]) -> GraphData {
    let mut records = records.to_vec();
    records.sort_by(|left, right| left.id.cmp(&right.id));
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut concept_counts = BTreeMap::<String, usize>::new();
    let mut co_occurrences = BTreeMap::<(String, String), usize>::new();

    for record in records {
        let memory_id = memory_node_id(&record);
        let concepts = extract_concepts(&record.content);
        nodes.push(GraphNode {
            id: memory_id.clone(),
            label: truncate_excerpt(&record.content, 96),
            kind: "memory".to_owned(),
            weight: concepts.len().max(1) as f64,
            metadata: json!({
                "memory_id": record.id.to_string(),
                "module_id": record.module_id,
                "content_type": record.content_type,
                "updated_at": record.updated_at,
            }),
        });
        for concept in &concepts {
            *concept_counts.entry(concept.clone()).or_insert(0) += 1;
            edges.push(GraphEdge {
                source: memory_id.clone(),
                target: concept_node_id(concept),
                relation: "mentions".to_owned(),
                weight: 1.0,
            });
        }
        for (index, left) in concepts.iter().enumerate() {
            for right in concepts.iter().skip(index + 1) {
                let pair = if left <= right {
                    (left.clone(), right.clone())
                } else {
                    (right.clone(), left.clone())
                };
                *co_occurrences.entry(pair).or_insert(0) += 1;
            }
        }
    }

    nodes.extend(concept_counts.iter().map(|(concept, count)| GraphNode {
        id: concept_node_id(concept),
        label: concept.clone(),
        kind: "concept".to_owned(),
        weight: *count as f64,
        metadata: json!({"memory_count": count}),
    }));
    edges.extend(
        co_occurrences
            .into_iter()
            .map(|((left, right), count)| GraphEdge {
                source: concept_node_id(&left),
                target: concept_node_id(&right),
                relation: "co_occurs".to_owned(),
                weight: count as f64,
            }),
    );
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.relation.cmp(&right.relation))
    });
    GraphData {
        communities: build_communities(&nodes, &edges),
        nodes,
        edges,
    }
}

fn build_communities(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<Community> {
    let mut adjacency = BTreeMap::<String, BTreeSet<String>>::new();
    for node in nodes {
        adjacency.entry(node.id.clone()).or_default();
    }
    for edge in edges {
        adjacency
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.target.clone());
        adjacency
            .entry(edge.target.clone())
            .or_default()
            .insert(edge.source.clone());
    }
    let node_labels = nodes
        .iter()
        .map(|node| (node.id.as_str(), node.label.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut unseen = adjacency.keys().cloned().collect::<BTreeSet<_>>();
    let mut communities = Vec::new();

    while let Some(start) = unseen.first().cloned() {
        let mut frontier = vec![start.clone()];
        let mut component = BTreeSet::new();
        unseen.remove(&start);
        while let Some(node_id) = frontier.pop() {
            if !component.insert(node_id.clone()) {
                continue;
            }
            for neighbor in adjacency.get(&node_id).into_iter().flatten() {
                if unseen.remove(neighbor) {
                    frontier.push(neighbor.clone());
                }
            }
        }
        let concept_label = component
            .iter()
            .find(|node_id| node_id.starts_with("concept:"))
            .and_then(|node_id| node_labels.get(node_id.as_str()))
            .copied()
            .unwrap_or("memory");
        communities.push(Community {
            id: format!("memory-community-{}", communities.len() + 1),
            label: format!("Memory community: {concept_label}"),
            node_ids: component.into_iter().collect(),
        });
    }

    communities
}

fn concept_terms(value: &str) -> BTreeSet<String> {
    normalized_terms(value).into_iter().collect()
}

fn normalized_terms(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| {
            term.len() >= 2 && !STOP_WORDS.contains(term) && term.chars().any(char::is_alphabetic)
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn memory_node_id(record: &MemoryRecord) -> String {
    format!("memory:{}", record.id)
}

fn concept_node_id(concept: &str) -> String {
    format!("concept:{concept}")
}

fn truncate_excerpt(value: &str, max_chars: usize) -> String {
    let excerpt = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{KnowledgeGraphService, build_knowledge_graph, extract_concepts};
    use crate::memory::{MemoryStore, NewMemory};

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap()
    }

    #[test]
    fn extracts_stable_high_signal_concepts() {
        assert_eq!(
            extract_concepts("Retrieval retrieval practice improves learning outcomes."),
            vec!["retrieval", "improves", "learning", "outcomes", "practice"]
        );
    }

    #[test]
    fn builds_memory_concept_graph_with_communities() {
        let store = MemoryStore::in_memory().unwrap();
        let first = store
            .store(
                NewMemory::new("notes", "Retrieval practice improves learning", "note"),
                timestamp(),
            )
            .unwrap();
        let second = store
            .store(
                NewMemory::new("notes", "Learning outcomes need retrieval", "note"),
                timestamp(),
            )
            .unwrap();

        let graph = build_knowledge_graph(&[first, second]);

        assert!(
            graph
                .nodes
                .iter()
                .any(|node| node.id == "concept:retrieval")
        );
        assert!(graph.edges.iter().any(|edge| edge.relation == "co_occurs"));
        assert!(!graph.communities.is_empty());
    }

    #[test]
    fn searches_and_reads_graph_neighbors() {
        let store = MemoryStore::in_memory().unwrap();
        store
            .store(
                NewMemory::new("notes", "Calculus limits and derivatives", "note"),
                timestamp(),
            )
            .unwrap();
        let graph = KnowledgeGraphService::new(store);

        let hits = graph.search("calculus", 5).unwrap();
        let neighborhood = graph.neighbors(&hits[0].node.id).unwrap().unwrap();

        assert!(!hits.is_empty());
        assert!(!neighborhood.neighbors.is_empty());
    }
}
