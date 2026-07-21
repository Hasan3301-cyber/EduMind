use std::collections::{BTreeMap, BTreeSet};

use edumind_core::{
    ClaimAssessment, ClaimSupport, ClaimValidationRequest, Community, EvidenceSource, GraphData,
    GraphEdge, GraphNode, LiteratureGraphRequest, PaperMetadata, ValidationIssue, ValidationReport,
};
use serde_json::json;

use super::analysis::{normalize_terms, paper_concepts};

/// Validates draft claims against supplied evidence using deterministic lexical support scores.
#[must_use]
pub fn validate_claims(request: &ClaimValidationRequest) -> ValidationReport {
    let threshold = request.support_threshold.clamp(0.05, 1.0);
    let known_citations = request
        .evidence
        .iter()
        .flat_map(|source| {
            [Some(source.id.as_str()), source.citation_label.as_deref()]
                .into_iter()
                .flatten()
                .map(ToOwned::to_owned)
        })
        .collect::<BTreeSet<_>>();
    let mut report = ValidationReport::default();
    for (claim_index, raw_claim) in request.claims.iter().enumerate() {
        let claim = raw_claim.trim().to_owned();
        let terms = normalize_terms(&claim).into_iter().collect::<BTreeSet<_>>();
        let matches = evidence_matches(&terms, &request.evidence);
        let best_score = matches.first().map_or(0.0, |(_, score)| *score);
        let evidence_ids = matches
            .iter()
            .filter(|(_, score)| *score >= threshold * 0.5)
            .take(3)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let support = if best_score >= threshold {
            ClaimSupport::Supported
        } else if best_score >= threshold * 0.5 {
            ClaimSupport::Partial
        } else {
            ClaimSupport::Unsupported
        };
        if support == ClaimSupport::Unsupported {
            report.hallucinations.push(ValidationIssue {
                claim_index: Some(claim_index),
                code: "unsupported_claim".to_owned(),
                message: format!(
                    "The claim has only {:.0}% lexical evidence support, below the {:.0}% threshold.",
                    best_score * 100.0,
                    threshold * 100.0
                ),
            });
        }
        for citation in cited_labels(&claim) {
            if !known_citations.contains(&citation) {
                report.citation_errors.push(ValidationIssue {
                    claim_index: Some(claim_index),
                    code: "unknown_citation".to_owned(),
                    message: format!("The cited evidence label `{citation}` was not supplied."),
                });
            }
        }
        if contains_logical_overreach(&claim) {
            report.logical_issues.push(ValidationIssue {
                claim_index: Some(claim_index),
                code: "overstated_inference".to_owned(),
                message: "The claim uses causal or proof language that metadata evidence alone cannot establish."
                    .to_owned(),
            });
        }
        if contains_bias_language(&claim) {
            report.bias_flags.push(ValidationIssue {
                claim_index: Some(claim_index),
                code: "absolute_language".to_owned(),
                message:
                    "The claim uses absolute language; qualify it with scope and evidence limits."
                        .to_owned(),
            });
        }
        report.claims.push(ClaimAssessment {
            claim,
            support,
            support_score: best_score,
            evidence_ids,
        });
    }
    report.overall_score = overall_score(&report);
    report
}

/// Converts normalized paper metadata into evidence snippets for the critic plugin.
#[must_use]
pub fn evidence_from_papers(papers: &[PaperMetadata]) -> Vec<EvidenceSource> {
    papers
        .iter()
        .map(|paper| EvidenceSource {
            id: paper.id.clone(),
            title: paper.title.clone(),
            text: format!(
                "{} {} {} {}",
                paper.title,
                paper.abstract_text,
                paper.keywords.join(" "),
                paper.fields_of_study.join(" ")
            ),
            citation_label: paper.doi.clone().or_else(|| Some(paper.id.clone())),
            source_url: paper.source_url.clone(),
        })
        .collect()
}

/// Builds a deterministic literature graph from citations and concept-set Jaccard similarity.
#[must_use]
pub fn build_literature_graph(request: &LiteratureGraphRequest) -> GraphData {
    let papers = unique_papers(&request.papers);
    let similarity_threshold = request.similarity_threshold.clamp(0.0, 1.0);
    let aliases = paper_aliases(&papers);
    let concepts = papers
        .iter()
        .map(|paper| (graph_node_id(&paper.id), paper_concepts(paper)))
        .collect::<BTreeMap<_, _>>();
    let mut edges = citation_edges(&papers, &aliases);
    for (index, left) in papers.iter().enumerate() {
        for right in papers.iter().skip(index + 1) {
            let left_id = graph_node_id(&left.id);
            let right_id = graph_node_id(&right.id);
            let score = jaccard_similarity(
                concepts.get(&left_id).expect("paper concept set exists"),
                concepts.get(&right_id).expect("paper concept set exists"),
            );
            if score >= similarity_threshold && score > 0.0 {
                edges.push(GraphEdge {
                    source: left_id,
                    target: right_id,
                    relation: "concept_similarity".to_owned(),
                    weight: score,
                });
            }
        }
    }
    edges.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.target.cmp(&right.target))
            .then_with(|| left.relation.cmp(&right.relation))
    });
    edges.dedup_by(|left, right| {
        left.source == right.source
            && left.target == right.target
            && left.relation == right.relation
    });
    let nodes = papers
        .iter()
        .map(|paper| GraphNode {
            id: graph_node_id(&paper.id),
            label: paper.title.clone(),
            kind: "paper".to_owned(),
            weight: f64::from(paper.citation_count.max(1)),
            metadata: json!({
                "paper_id": paper.id,
                "year": paper.year,
                "venue": paper.venue,
                "doi": paper.doi,
                "source": paper.source,
            }),
        })
        .collect::<Vec<_>>();
    GraphData {
        communities: build_communities(&nodes, &edges, &concepts),
        nodes,
        edges,
    }
}

fn evidence_matches(
    claim_terms: &BTreeSet<String>,
    evidence: &[EvidenceSource],
) -> Vec<(String, f64)> {
    if claim_terms.is_empty() {
        return Vec::new();
    }
    let mut matches = evidence
        .iter()
        .map(|source| {
            let terms = normalize_terms(&format!("{} {}", source.title, source.text))
                .into_iter()
                .collect::<BTreeSet<_>>();
            let overlap = claim_terms.intersection(&terms).count();
            (source.id.clone(), overlap as f64 / claim_terms.len() as f64)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    matches
}

fn cited_labels(claim: &str) -> Vec<String> {
    let mut labels = BTreeSet::new();
    let mut cursor = claim;
    while let Some(start) = cursor.find('[') {
        let after_start = &cursor[start + 1..];
        let Some(end) = after_start.find(']') else {
            break;
        };
        labels.extend(
            after_start[..end]
                .split(',')
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .map(ToOwned::to_owned),
        );
        cursor = &after_start[end + 1..];
    }
    labels.into_iter().collect()
}

fn contains_logical_overreach(claim: &str) -> bool {
    ["causes", "proves", "guarantees", "therefore"]
        .iter()
        .any(|term| normalize_terms(claim).iter().any(|word| word == term))
}

fn contains_bias_language(claim: &str) -> bool {
    [
        "all",
        "always",
        "never",
        "universally",
        "clearly",
        "obviously",
    ]
    .iter()
    .any(|term| normalize_terms(claim).iter().any(|word| word == term))
}

fn overall_score(report: &ValidationReport) -> f64 {
    if report.claims.is_empty() {
        return 1.0;
    }
    let mean_support = report
        .claims
        .iter()
        .map(|assessment| assessment.support_score)
        .sum::<f64>()
        / report.claims.len() as f64;
    let claim_count = report.claims.len() as f64;
    let penalty = report.hallucinations.len() as f64 * 0.20 / claim_count
        + report.citation_errors.len() as f64 * 0.10 / claim_count
        + report.logical_issues.len() as f64 * 0.08 / claim_count
        + report.bias_flags.len() as f64 * 0.05 / claim_count;
    (mean_support - penalty).clamp(0.0, 1.0)
}

fn unique_papers(papers: &[PaperMetadata]) -> Vec<PaperMetadata> {
    let mut unique = BTreeMap::new();
    for paper in papers {
        unique
            .entry(paper.id.clone())
            .or_insert_with(|| paper.clone());
    }
    unique.into_values().collect()
}

fn paper_aliases(papers: &[PaperMetadata]) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    for paper in papers {
        let node_id = graph_node_id(&paper.id);
        aliases.insert(paper.id.clone(), node_id.clone());
        for source_id in paper.source_ids.values() {
            aliases.insert(source_id.clone(), node_id.clone());
        }
    }
    aliases
}

fn citation_edges(papers: &[PaperMetadata], aliases: &BTreeMap<String, String>) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    for paper in papers {
        let node_id = graph_node_id(&paper.id);
        for reference in &paper.referenced_paper_ids {
            if let Some(target) = aliases.get(reference) {
                edges.push(GraphEdge {
                    source: node_id.clone(),
                    target: target.clone(),
                    relation: "references".to_owned(),
                    weight: 1.0,
                });
            }
        }
        for influenced in &paper.influenced_paper_ids {
            if let Some(source) = aliases.get(influenced) {
                edges.push(GraphEdge {
                    source: source.clone(),
                    target: node_id.clone(),
                    relation: "cites".to_owned(),
                    weight: 1.0,
                });
            }
        }
    }
    edges
}

fn build_communities(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    concepts: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<Community> {
    let mut adjacency = nodes
        .iter()
        .map(|node| (node.id.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        if adjacency.contains_key(&edge.source) && adjacency.contains_key(&edge.target) {
            adjacency
                .get_mut(&edge.source)
                .expect("source node exists")
                .insert(edge.target.clone());
            adjacency
                .get_mut(&edge.target)
                .expect("target node exists")
                .insert(edge.source.clone());
        }
    }
    let mut visited = BTreeSet::new();
    let mut communities = Vec::new();
    for node in adjacency.keys() {
        if visited.contains(node) {
            continue;
        }
        let mut stack = vec![node.clone()];
        let mut component = BTreeSet::new();
        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            component.insert(current.clone());
            if let Some(neighbors) = adjacency.get(&current) {
                stack.extend(neighbors.iter().rev().cloned());
            }
        }
        let node_ids = component.into_iter().collect::<Vec<_>>();
        let label = community_label(&node_ids, concepts);
        communities.push(Community {
            id: format!("community-{}", communities.len() + 1),
            label,
            node_ids,
        });
    }
    communities
}

fn community_label(node_ids: &[String], concepts: &BTreeMap<String, BTreeSet<String>>) -> String {
    let mut frequencies = BTreeMap::<String, usize>::new();
    for node_id in node_ids {
        for concept in concepts.get(node_id).into_iter().flatten() {
            *frequencies.entry(concept.clone()).or_insert(0) += 1;
        }
    }
    frequencies
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map_or_else(
            || "Literature community".to_owned(),
            |(concept, _)| format!("{concept} community"),
        )
}

fn jaccard_similarity(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    let union = left.union(right).count();
    if union == 0 {
        return 0.0;
    }
    left.intersection(right).count() as f64 / union as f64
}

fn graph_node_id(paper_id: &str) -> String {
    format!("paper:{paper_id}")
}

#[cfg(test)]
mod tests {
    use edumind_core::{
        ClaimSupport, ClaimValidationRequest, EvidenceSource, LiteratureGraphRequest, PaperMetadata,
    };

    use super::{build_literature_graph, validate_claims};

    #[test]
    fn claim_validation_separates_supported_and_unsupported_claims() {
        let report = validate_claims(&ClaimValidationRequest {
            claims: vec![
                "Retrieval improves student learning [paper-1]".to_owned(),
                "All learners always improve [missing]".to_owned(),
            ],
            evidence: vec![EvidenceSource {
                id: "paper-1".to_owned(),
                title: "Retrieval and student learning".to_owned(),
                text: "The study evaluates retrieval practice for student learning outcomes."
                    .to_owned(),
                citation_label: None,
                source_url: None,
            }],
            support_threshold: 0.35,
        });

        assert_eq!(report.claims[0].support, ClaimSupport::Supported);
        assert_eq!(report.claims[1].support, ClaimSupport::Unsupported);
        assert_eq!(report.citation_errors.len(), 1);
        assert_eq!(report.bias_flags.len(), 1);
    }

    #[test]
    fn literature_graph_links_shared_concepts_and_builds_communities() {
        let papers = vec![
            PaperMetadata {
                id: "a".to_owned(),
                title: "Retrieval learning".to_owned(),
                keywords: vec!["retrieval".to_owned(), "education".to_owned()],
                referenced_paper_ids: vec!["b".to_owned()],
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "b".to_owned(),
                title: "Retrieval assessment".to_owned(),
                keywords: vec!["retrieval".to_owned(), "assessment".to_owned()],
                ..PaperMetadata::default()
            },
        ];

        let graph = build_literature_graph(&LiteratureGraphRequest {
            papers,
            similarity_threshold: 0.2,
        });

        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.edges.iter().any(|edge| edge.relation == "references"));
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.relation == "concept_similarity")
        );
        assert_eq!(graph.communities.len(), 1);
    }
}
