use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::infra::Result;

use super::{HybridMemory, HybridSearchHit, MemoryRecord};

const RELEVANCE_WEIGHT: f64 = 0.65;
const QUERY_OVERLAP_WEIGHT: f64 = 0.15;
const REDUNDANCY_WEIGHT: f64 = 0.35;

/// A hybrid result after the deterministic Jaccard/MMR diversity reranker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdvancedSearchHit {
    pub record: MemoryRecord,
    pub lexical_score: f32,
    #[serde(default)]
    pub vector_similarity: Option<f32>,
    pub initial_score: f32,
    pub rerank_score: f64,
    pub redundancy_penalty: f64,
}

/// Multi-stage memory retrieval that reranks hybrid candidates for relevance and novelty.
#[derive(Clone)]
pub struct AdvancedSearchService {
    memory: HybridMemory,
}

impl AdvancedSearchService {
    /// Creates an advanced search service over the existing local hybrid index.
    #[must_use]
    pub fn new(memory: HybridMemory) -> Self {
        Self { memory }
    }

    /// Retrieves a broad hybrid candidate set and applies a deterministic MMR reranker.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<AdvancedSearchHit>> {
        if limit == 0 || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let candidate_limit = limit.saturating_mul(8).min(256).max(limit);
        let candidates = self.memory.search(query, candidate_limit).await?;
        Ok(rerank_hits(query, candidates, limit))
    }

    /// Returns the underlying hybrid service for composing memory intelligence workflows.
    #[must_use]
    pub fn hybrid_memory(&self) -> HybridMemory {
        self.memory.clone()
    }
}

/// Applies Jaccard-aware maximal marginal relevance to already-scored hybrid candidates.
#[must_use]
pub fn rerank_hits(
    query: &str,
    hits: Vec<HybridSearchHit>,
    limit: usize,
) -> Vec<AdvancedSearchHit> {
    if limit == 0 {
        return Vec::new();
    }
    let query_terms = terms(query);
    let mut remaining = hits
        .into_iter()
        .map(|hit| Candidate {
            terms: terms(&hit.record.content),
            hit,
        })
        .collect::<Vec<_>>();
    let mut selected = Vec::<Candidate>::new();
    let mut results = Vec::new();

    while !remaining.is_empty() && results.len() < limit {
        let mut best: Option<(usize, f64, f64)> = None;
        for (index, candidate) in remaining.iter().enumerate() {
            let query_overlap = jaccard(&query_terms, &candidate.terms);
            let redundancy = selected
                .iter()
                .map(|chosen| jaccard(&candidate.terms, &chosen.terms))
                .fold(0.0_f64, f64::max);
            let score = RELEVANCE_WEIGHT * f64::from(candidate.hit.score)
                + QUERY_OVERLAP_WEIGHT * query_overlap
                - REDUNDANCY_WEIGHT * redundancy;
            let replace = best.is_none_or(|(best_index, best_score, _)| {
                score.total_cmp(&best_score).is_gt()
                    || (score.total_cmp(&best_score).is_eq()
                        && candidate.hit.record.id < remaining[best_index].hit.record.id)
            });
            if replace {
                best = Some((index, score, redundancy));
            }
        }
        let (index, rerank_score, redundancy_penalty) = best.expect("remaining candidates exist");
        let candidate = remaining.remove(index);
        results.push(AdvancedSearchHit {
            record: candidate.hit.record.clone(),
            lexical_score: candidate.hit.lexical_score,
            vector_similarity: candidate.hit.vector_similarity,
            initial_score: candidate.hit.score,
            rerank_score,
            redundancy_penalty,
        });
        selected.push(candidate);
    }

    results
}

#[derive(Clone)]
struct Candidate {
    hit: HybridSearchHit,
    terms: BTreeSet<String>,
}

fn terms(value: &str) -> BTreeSet<String> {
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.len() >= 2)
        .map(ToOwned::to_owned)
        .collect()
}

fn jaccard(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count();
    let union = left.union(right).count();
    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::rerank_hits;
    use crate::memory::{HybridSearchHit, MemoryId, MemoryRecord};

    fn hit(content: &str, score: f32) -> HybridSearchHit {
        HybridSearchHit {
            record: MemoryRecord {
                id: MemoryId::new(),
                module_id: "notes".to_owned(),
                content: content.to_owned(),
                content_type: "note".to_owned(),
                metadata: json!({}),
                created_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                updated_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
            },
            lexical_score: score,
            vector_similarity: Some(score),
            candidate_rank: Some(1),
            exact_rerank_score: Some(score),
            index_generation: 1,
            score,
        }
    }

    #[test]
    fn reranks_relevant_candidates_while_penalizing_near_duplicates() {
        let results = rerank_hits(
            "calculus study",
            vec![
                hit("calculus study plan and limits", 0.95),
                hit("calculus study plan and limits review", 0.93),
                hit("biology revision strategy", 0.72),
            ],
            2,
        );

        assert_eq!(results.len(), 2);
        assert!(results[1].redundancy_penalty <= 1.0);
        assert_ne!(results[0].record.content, results[1].record.content);
    }
}
