use std::collections::{BTreeMap, BTreeSet};

use edumind_core::{Citation, EvidenceSpan, GroundedAnswer, PaperMetadata};
use serde::{Deserialize, Serialize};

use super::analysis::normalize_terms;

/// One row of a traceable paper comparison matrix.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComparisonRow {
    pub paper_id: String,
    pub title: String,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub venue: Option<String>,
    pub citation_count: u32,
    #[serde(default)]
    pub themes: Vec<String>,
}

/// One theme-grouped section of a deterministic research outline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SynthesisSection {
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

/// Comparison matrix and traceable outline generated from a project corpus.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResearchSynthesis {
    #[serde(default)]
    pub comparison_matrix: Vec<ComparisonRow>,
    #[serde(default)]
    pub outline: Vec<SynthesisSection>,
    #[serde(default)]
    pub grounded_sections: Vec<GroundedAnswer>,
}

/// Builds a deterministic comparison matrix plus theme-grouped, source-linked outline.
#[must_use]
pub fn build_synthesis(papers: &[PaperMetadata]) -> ResearchSynthesis {
    let mut papers = papers.to_vec();
    papers.sort_by(|left, right| {
        normalized_title(&left.title)
            .cmp(&normalized_title(&right.title))
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut groups = BTreeMap::<String, Vec<&PaperMetadata>>::new();
    let comparison_matrix = papers
        .iter()
        .map(|paper| {
            let themes = explicit_themes(paper).into_iter().collect::<Vec<_>>();
            if themes.is_empty() {
                groups
                    .entry("__cross_cutting__".to_owned())
                    .or_default()
                    .push(paper);
            } else {
                for theme in &themes {
                    groups.entry(theme.clone()).or_default().push(paper);
                }
            }
            ComparisonRow {
                paper_id: paper.id.clone(),
                title: paper.title.clone(),
                year: paper.year,
                venue: paper.venue.clone(),
                citation_count: paper.citation_count,
                themes,
            }
        })
        .collect();
    let outline: Vec<SynthesisSection> = groups
        .into_iter()
        .map(|(theme, papers)| {
            let source_ids = papers
                .iter()
                .map(|paper| paper.id.clone())
                .collect::<Vec<_>>();
            let titles = papers
                .iter()
                .take(3)
                .map(|paper| format!("`{}`", paper.title))
                .collect::<Vec<_>>();
            let (title, summary) = if theme == "__cross_cutting__" {
                (
                    "Cross-cutting papers".to_owned(),
                    format!(
                        "{} papers contribute evidence without explicit theme metadata: {}.",
                        source_ids.len(),
                        titles.join(", ")
                    ),
                )
            } else {
                (
                    format!("Theme: {theme}"),
                    format!(
                        "{} papers explicitly address `{theme}`: {}.",
                        source_ids.len(),
                        titles.join(", ")
                    ),
                )
            };
            SynthesisSection {
                title,
                summary,
                source_ids,
            }
        })
        .collect();
    let grounded_sections = outline
        .iter()
        .map(|section| grounded_section(section, &papers))
        .collect();
    ResearchSynthesis {
        comparison_matrix,
        outline,
        grounded_sections,
    }
}

fn grounded_section(section: &SynthesisSection, papers: &[PaperMetadata]) -> GroundedAnswer {
    let sources = section
        .source_ids
        .iter()
        .filter_map(|source_id| papers.iter().find(|paper| &paper.id == source_id))
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return GroundedAnswer::insufficient_evidence(section.summary.clone());
    }
    let citations = sources
        .iter()
        .map(|paper| Citation {
            source_id: paper.id.clone(),
            title: Some(paper.title.clone()),
            locator: Some("abstract".to_owned()),
        })
        .collect::<Vec<_>>();
    let evidence = sources
        .iter()
        .map(|paper| {
            let text = if paper.abstract_text.trim().is_empty() {
                paper.title.clone()
            } else {
                paper.abstract_text.clone()
            };
            EvidenceSpan::new(paper.id.clone(), 0, text.len(), text)
        })
        .collect::<Vec<_>>();
    let confidence = (0.5 + sources.len() as f32 * 0.1).min(0.9);
    GroundedAnswer::grounded(section.summary.clone(), citations, evidence, confidence)
        .unwrap_or_else(|_| GroundedAnswer::insufficient_evidence(section.summary.clone()))
}

fn explicit_themes(paper: &PaperMetadata) -> BTreeSet<String> {
    paper
        .keywords
        .iter()
        .chain(&paper.fields_of_study)
        .filter_map(|value| {
            let theme = normalize_terms(value).join(" ");
            (!theme.is_empty()).then_some(theme)
        })
        .collect()
}

fn normalized_title(title: &str) -> String {
    title.to_lowercase()
}

#[cfg(test)]
mod tests {
    use edumind_core::PaperMetadata;

    use super::build_synthesis;

    #[test]
    fn synthesis_groups_explicit_themes_and_keeps_untagged_papers_traceable() {
        let papers = vec![
            PaperMetadata {
                id: "retrieval".to_owned(),
                title: "Retrieval practice".to_owned(),
                keywords: vec!["retrieval".to_owned()],
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "untagged".to_owned(),
                title: "General methods".to_owned(),
                ..PaperMetadata::default()
            },
        ];

        let synthesis = build_synthesis(&papers);

        assert_eq!(synthesis.comparison_matrix.len(), 2);
        assert_eq!(synthesis.outline.len(), 2);
        assert!(
            synthesis
                .outline
                .iter()
                .any(|section| section.source_ids == vec!["retrieval"])
        );
        assert!(
            synthesis
                .outline
                .iter()
                .any(|section| section.title == "Cross-cutting papers")
        );
    }
}
