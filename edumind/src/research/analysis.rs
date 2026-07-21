use std::collections::{BTreeMap, BTreeSet};

use edumind_core::PaperMetadata;

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "into", "is", "it", "of",
    "on", "or", "that", "the", "this", "to", "with",
];

/// Counts normalized concepts once per paper to avoid prolific abstracts dominating themes.
pub(crate) fn corpus_keyword_frequencies(papers: &[PaperMetadata]) -> BTreeMap<String, usize> {
    let mut frequencies = BTreeMap::new();
    for paper in papers {
        for concept in paper_concepts(paper) {
            *frequencies.entry(concept).or_insert(0) += 1;
        }
    }
    frequencies
}

/// Returns the share of dated papers published inside the corpus-relative recent window.
pub(crate) fn recency_share(papers: &[PaperMetadata], window_years: i32) -> f64 {
    let years = papers
        .iter()
        .filter_map(|paper| paper.year)
        .collect::<Vec<_>>();
    let Some(latest_year) = years.iter().copied().max() else {
        return 0.0;
    };
    let window_years = window_years.max(1);
    let cutoff = latest_year.saturating_sub(window_years - 1);
    let recent = years.iter().filter(|year| **year >= cutoff).count();
    recent as f64 / years.len() as f64
}

/// Finds query terms that are absent from, or represented by at most one quarter of, the corpus.
pub(crate) fn detect_gaps(query_terms: &[String], papers: &[PaperMetadata]) -> Vec<String> {
    if query_terms.is_empty() {
        return Vec::new();
    }
    let total = papers.len();
    let mut gaps = BTreeSet::new();
    for term in query_terms {
        let normalized = normalize_terms(term);
        for candidate in normalized {
            let coverage = papers
                .iter()
                .filter(|paper| paper_terms(paper).contains(&candidate))
                .count();
            if total == 0 || coverage.saturating_mul(4) <= total {
                gaps.insert(candidate);
            }
        }
    }
    gaps.into_iter().collect()
}

/// Estimates corpus novelty from recent coverage and the share of one-off concepts.
pub(crate) fn corpus_novelty(papers: &[PaperMetadata]) -> f64 {
    if papers.is_empty() {
        return 0.0;
    }
    let frequencies = corpus_keyword_frequencies(papers);
    let unique_share = if frequencies.is_empty() {
        0.0
    } else {
        frequencies.values().filter(|count| **count == 1).count() as f64 / frequencies.len() as f64
    };
    (recency_share(papers, 3) * 0.65 + unique_share * 0.35).clamp(0.0, 1.0)
}

/// Truncates at a Unicode character boundary without appending ungrounded text.
pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Extracts lower-cased non-stopword terms in deterministic encounter order.
pub(crate) fn normalize_terms(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty() && !STOP_WORDS.contains(term))
        .map(ToOwned::to_owned)
        .collect()
}

/// Returns normalized concepts supplied by a paper or derives terms from its text.
pub(crate) fn paper_concepts(paper: &PaperMetadata) -> BTreeSet<String> {
    let explicit = paper
        .keywords
        .iter()
        .chain(&paper.fields_of_study)
        .filter_map(|value| normalize_concept(value))
        .collect::<BTreeSet<_>>();
    if !explicit.is_empty() {
        return explicit;
    }
    normalize_terms(&format!("{} {}", paper.title, paper.abstract_text))
        .into_iter()
        .collect()
}

/// Returns all lexical terms used for query coverage and evidence matching.
pub(crate) fn paper_terms(paper: &PaperMetadata) -> BTreeSet<String> {
    let mut terms = paper_concepts(paper);
    terms.extend(
        paper
            .keywords
            .iter()
            .flat_map(|keyword| normalize_terms(keyword)),
    );
    terms.extend(
        paper
            .fields_of_study
            .iter()
            .flat_map(|field| normalize_terms(field)),
    );
    terms.extend(normalize_terms(&paper.title));
    terms.extend(normalize_terms(&paper.abstract_text));
    terms
}

fn normalize_concept(value: &str) -> Option<String> {
    let normalized = normalize_terms(value).join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use edumind_core::PaperMetadata;

    use super::{
        corpus_keyword_frequencies, corpus_novelty, detect_gaps, recency_share, truncate_chars,
    };

    fn paper(id: &str, year: i32, keywords: &[&str]) -> PaperMetadata {
        PaperMetadata {
            id: id.to_owned(),
            title: format!("{id} learning systems"),
            abstract_text: "A focused study of retrieval and learning outcomes.".to_owned(),
            year: Some(year),
            keywords: keywords.iter().map(|value| (*value).to_owned()).collect(),
            ..PaperMetadata::default()
        }
    }

    #[test]
    fn frequency_and_recency_helpers_are_corpus_relative() {
        let papers = vec![
            paper("a", 2026, &["retrieval", "evaluation"]),
            paper("b", 2025, &["retrieval"]),
            paper("c", 2021, &["reasoning"]),
        ];

        let frequencies = corpus_keyword_frequencies(&papers);

        assert_eq!(frequencies["retrieval"], 2);
        assert_eq!(recency_share(&papers, 2), 2.0 / 3.0);
        assert!(corpus_novelty(&papers) > 0.0);
    }

    #[test]
    fn gap_detection_and_unicode_truncation_are_deterministic() {
        let papers = vec![
            paper("a", 2026, &["retrieval"]),
            paper("b", 2025, &["retrieval"]),
        ];
        let terms = vec!["retrieval".to_owned(), "fairness".to_owned()];

        assert_eq!(detect_gaps(&terms, &papers), vec!["fairness".to_owned()]);
        assert_eq!(truncate_chars("éclair", 2), "éc");
        assert_eq!(truncate_chars("research", 0), "");
    }
}
