use edumind_core::{PaperMetadata, RankedPaper};

use crate::{infra::Result, memory::TextEmbedder};

use super::analysis::normalize_terms;

/// Ranks discovery results by relevance, citations, corpus-relative recency, and abstract richness.
#[must_use]
pub fn rank_discovery_papers(papers: &[PaperMetadata], query: &str) -> Vec<RankedPaper> {
    let query_terms = normalize_terms(query);
    let max_citations = papers
        .iter()
        .map(|paper| paper.citation_count)
        .max()
        .unwrap_or(0);
    let latest_year = papers.iter().filter_map(|paper| paper.year).max();
    let mut ranked = papers
        .iter()
        .cloned()
        .map(|paper| {
            let lexical = lexical_relevance(&paper, &query_terms);
            let citations = if max_citations == 0 {
                0.0
            } else {
                f64::from(paper.citation_count) / f64::from(max_citations)
            };
            let recency = recency_score(paper.year, latest_year);
            let abstract_richness = (paper.abstract_text.chars().count() as f64 / 1_500.0).min(1.0);
            RankedPaper {
                paper,
                score: (lexical * 0.45
                    + citations * 0.25
                    + recency * 0.20
                    + abstract_richness * 0.10)
                    .clamp(0.0, 1.0),
            }
        })
        .collect::<Vec<_>>();
    sort_ranked_papers(&mut ranked);
    ranked
}

/// Lexically ranks project papers with title (3×), keyword (1.5×), and abstract (1×) weighting.
/// Citation count is used only as a stable tie-break after the lexical score.
#[must_use]
pub fn rank_papers_for_query(papers: &[PaperMetadata], query: &str) -> Vec<RankedPaper> {
    let query_terms = normalize_terms(query);
    let mut ranked = papers
        .iter()
        .cloned()
        .map(|paper| RankedPaper {
            score: lexical_relevance(&paper, &query_terms),
            paper,
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.paper.citation_count.cmp(&left.paper.citation_count))
            .then_with(|| normalized_title(&left.paper).cmp(&normalized_title(&right.paper)))
            .then_with(|| left.paper.id.cmp(&right.paper.id))
    });
    ranked
}

/// Narrows through lexical ranking, then reranks candidates by cosine similarity of title plus abstract.
pub async fn semantic_rank_papers(
    papers: &[PaperMetadata],
    query: &str,
    embedder: &dyn TextEmbedder,
    lexical_limit: usize,
) -> Result<Vec<RankedPaper>> {
    if lexical_limit == 0 || papers.is_empty() {
        return Ok(Vec::new());
    }
    let mut candidates = rank_papers_for_query(papers, query);
    candidates.truncate(lexical_limit.min(candidates.len()));
    let query_embedding = embedder.embed(query).await?;
    for candidate in &mut candidates {
        let text = format!(
            "{}\n{}",
            candidate.paper.title, candidate.paper.abstract_text
        );
        let paper_embedding = embedder.embed(&text).await?;
        let semantic_similarity = f64::from(query_embedding.cosine_similarity(&paper_embedding)?);
        let normalized_similarity = ((semantic_similarity + 1.0) / 2.0).clamp(0.0, 1.0);
        candidate.score = (candidate.score * 0.35 + normalized_similarity * 0.65).clamp(0.0, 1.0);
    }
    sort_ranked_papers(&mut candidates);
    Ok(candidates)
}

fn lexical_relevance(paper: &PaperMetadata, query_terms: &[String]) -> f64 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let weighted_matches = weighted_lexical_matches(paper, query_terms);
    (weighted_matches / (query_terms.len() as f64 * 3.0)).min(1.0)
}

fn weighted_lexical_matches(paper: &PaperMetadata, query_terms: &[String]) -> f64 {
    let title = normalize_terms(&paper.title);
    let keywords = paper
        .keywords
        .iter()
        .chain(&paper.fields_of_study)
        .flat_map(|value| normalize_terms(value))
        .collect::<Vec<_>>();
    let abstract_terms = normalize_terms(&paper.abstract_text);
    query_terms
        .iter()
        .map(|term| {
            if title.contains(term) {
                3.0
            } else if keywords.contains(term) {
                1.5
            } else if abstract_terms.contains(term) {
                1.0
            } else {
                0.0
            }
        })
        .sum()
}

fn recency_score(year: Option<i32>, latest_year: Option<i32>) -> f64 {
    match (year, latest_year) {
        (Some(year), Some(latest)) => {
            (1.0 - f64::from(latest.saturating_sub(year)) / 10.0).clamp(0.0, 1.0)
        }
        _ => 0.0,
    }
}

fn normalized_title(paper: &PaperMetadata) -> String {
    paper.title.to_lowercase()
}

fn sort_ranked_papers(ranked: &mut [RankedPaper]) {
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.paper.citation_count.cmp(&left.paper.citation_count))
            .then_with(|| normalized_title(&left.paper).cmp(&normalized_title(&right.paper)))
            .then_with(|| left.paper.id.cmp(&right.paper.id))
    });
}

#[cfg(test)]
mod tests {
    use edumind_core::PaperMetadata;

    use super::{rank_discovery_papers, rank_papers_for_query, semantic_rank_papers};
    use crate::memory::HashEmbedder;

    #[test]
    fn ranks_relevant_recent_and_well_cited_papers_first() {
        let papers = vec![
            PaperMetadata {
                id: "older".to_owned(),
                title: "Retrieval methods".to_owned(),
                abstract_text: "retrieval ".repeat(80),
                year: Some(2020),
                citation_count: 20,
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "newer".to_owned(),
                title: "Retrieval methods for students".to_owned(),
                abstract_text: "retrieval ".repeat(80),
                year: Some(2026),
                citation_count: 100,
                ..PaperMetadata::default()
            },
        ];

        let ranked = rank_discovery_papers(&papers, "retrieval");

        assert_eq!(ranked[0].paper.id, "newer");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn lexical_ranking_uses_citations_only_after_query_relevance() {
        let papers = vec![
            PaperMetadata {
                id: "relevant".to_owned(),
                title: "Retrieval practice".to_owned(),
                citation_count: 1,
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "popular".to_owned(),
                title: "Unrelated methods".to_owned(),
                citation_count: 10_000,
                ..PaperMetadata::default()
            },
        ];

        let ranked = rank_papers_for_query(&papers, "retrieval");

        assert_eq!(ranked[0].paper.id, "relevant");
    }

    #[tokio::test]
    async fn semantic_ranking_reranks_the_lexical_shortlist_offline() {
        let papers = vec![
            PaperMetadata {
                id: "retrieval".to_owned(),
                title: "Retrieval practice for learning".to_owned(),
                abstract_text: "Retrieval supports learning outcomes.".to_owned(),
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "biology".to_owned(),
                title: "Cell biology overview".to_owned(),
                abstract_text: "Cells and proteins.".to_owned(),
                ..PaperMetadata::default()
            },
        ];
        let embedder = HashEmbedder::new(64).unwrap();

        let ranked = semantic_rank_papers(&papers, "retrieval learning", &embedder, 2)
            .await
            .unwrap();

        assert_eq!(ranked[0].paper.id, "retrieval");
    }
}
