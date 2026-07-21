use std::collections::BTreeMap;

use edumind_core::{PaperMetadata, merge_paper_metadata, paper_identity_key};
use serde::{Deserialize, Serialize};

use super::analysis::normalize_terms;

/// Export format accepted by the research project export endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BibliographyFormat {
    Bibtex,
    Ris,
}

/// A bibliography export with its requested format and stable rendered content.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BibliographyExport {
    pub format: BibliographyFormat,
    pub content: String,
}

/// Renders a stable, deduplicated BibTeX bibliography.
#[must_use]
pub fn to_bibtex(papers: &[PaperMetadata]) -> String {
    bibliography_entries(papers)
        .into_iter()
        .map(|(paper, key)| {
            let entry_type = if paper.venue.is_some() {
                "article"
            } else {
                "misc"
            };
            let mut fields = vec![format!("  title = {{{}}}", escape_bibtex(&paper.title))];
            if !paper.authors.is_empty() {
                fields.push(format!(
                    "  author = {{{}}}",
                    escape_bibtex(&paper.authors.join(" and "))
                ));
            }
            if let Some(year) = paper.year {
                fields.push(format!("  year = {{{year}}}"));
            }
            if let Some(venue) = &paper.venue {
                fields.push(format!("  journal = {{{}}}", escape_bibtex(venue)));
            }
            if let Some(doi) = &paper.doi {
                fields.push(format!("  doi = {{{}}}", escape_bibtex(doi)));
            }
            if let Some(url) = paper.open_access_url.as_ref().or(paper.source_url.as_ref()) {
                fields.push(format!("  url = {{{}}}", escape_bibtex(url)));
            }
            format!("@{entry_type}{{{key},\n{}\n}}", fields.join(",\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Renders a stable, deduplicated RIS bibliography.
#[must_use]
pub fn to_ris(papers: &[PaperMetadata]) -> String {
    bibliography_entries(papers)
        .into_iter()
        .map(|(paper, _)| {
            let mut lines = vec![if paper.venue.is_some() {
                "TY  - JOUR".to_owned()
            } else {
                "TY  - GEN".to_owned()
            }];
            lines.push(format!("TI  - {}", paper.title));
            lines.extend(paper.authors.iter().map(|author| format!("AU  - {author}")));
            if let Some(year) = paper.year {
                lines.push(format!("PY  - {year}"));
            }
            if let Some(venue) = &paper.venue {
                lines.push(format!("JO  - {venue}"));
            }
            if let Some(doi) = &paper.doi {
                lines.push(format!("DO  - {doi}"));
            }
            if let Some(url) = paper.open_access_url.as_ref().or(paper.source_url.as_ref()) {
                lines.push(format!("UR  - {url}"));
            }
            lines.push("ER  -".to_owned());
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Builds an export payload for the requested bibliography format.
#[must_use]
pub fn export_bibliography(
    papers: &[PaperMetadata],
    format: BibliographyFormat,
) -> BibliographyExport {
    let content = match format {
        BibliographyFormat::Bibtex => to_bibtex(papers),
        BibliographyFormat::Ris => to_ris(papers),
    };
    BibliographyExport { format, content }
}

fn bibliography_entries(papers: &[PaperMetadata]) -> Vec<(PaperMetadata, String)> {
    let mut deduplicated = BTreeMap::<String, PaperMetadata>::new();
    for paper in papers {
        let key = paper_identity_key(paper);
        if let Some(existing) = deduplicated.remove(&key) {
            deduplicated.insert(key, merge_paper_metadata(existing, paper.clone()));
        } else {
            deduplicated.insert(key, paper.clone());
        }
    }
    let mut papers = deduplicated.into_values().collect::<Vec<_>>();
    papers.sort_by(|left, right| {
        left.title
            .to_lowercase()
            .cmp(&right.title.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut collision_counts = BTreeMap::<String, usize>::new();
    papers
        .into_iter()
        .map(|paper| {
            let base = citation_key_base(&paper);
            let count = collision_counts.entry(base.clone()).or_insert(0);
            let key = if *count == 0 {
                base
            } else {
                format!("{base}{}", suffix_for(*count))
            };
            *count += 1;
            (paper, key)
        })
        .collect()
}

fn citation_key_base(paper: &PaperMetadata) -> String {
    let surname = paper
        .authors
        .first()
        .and_then(|author| author.split_whitespace().last())
        .map(identifier_fragment)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "anon".to_owned());
    let year = paper
        .year
        .map(|year| year.to_string())
        .unwrap_or_else(|| "nd".to_owned());
    let word = normalize_terms(&paper.title)
        .into_iter()
        .next()
        .map(|word| identifier_fragment(&word))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "work".to_owned());
    format!("{surname}{year}{word}")
}

fn suffix_for(index: usize) -> String {
    let mut index = index;
    let mut suffix = String::new();
    while index > 0 {
        index -= 1;
        suffix.insert(0, char::from(b'a' + u8::try_from(index % 26).unwrap_or(0)));
        index /= 26;
    }
    suffix
}

fn identifier_fragment(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn escape_bibtex(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

#[cfg(test)]
mod tests {
    use edumind_core::PaperMetadata;

    use super::{BibliographyFormat, export_bibliography, to_bibtex, to_ris};

    #[test]
    fn exports_stable_deduplicated_bibtex_and_ris_entries() {
        let papers = vec![
            PaperMetadata {
                id: "one".to_owned(),
                title: "Graph learning".to_owned(),
                authors: vec!["Alex Smith".to_owned()],
                year: Some(2026),
                doi: Some("10.1/graph".to_owned()),
                venue: Some("Journal".to_owned()),
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "two".to_owned(),
                title: "Graph learning".to_owned(),
                authors: vec!["Alex Smith".to_owned()],
                year: Some(2026),
                doi: Some("10.1/graph".to_owned()),
                abstract_text: "Richer duplicate.".to_owned(),
                ..PaperMetadata::default()
            },
            PaperMetadata {
                id: "three".to_owned(),
                title: "Graph methods".to_owned(),
                authors: vec!["Alex Smith".to_owned()],
                year: Some(2026),
                ..PaperMetadata::default()
            },
        ];

        let bibtex = to_bibtex(&papers);
        let ris = to_ris(&papers);
        let export = export_bibliography(&papers, BibliographyFormat::Bibtex);

        assert_eq!(
            bibtex.matches("@article").count() + bibtex.matches("@misc").count(),
            2
        );
        assert!(bibtex.contains("smith2026graph"));
        assert!(bibtex.contains("smith2026grapha"));
        assert_eq!(ris.matches("TY  -").count(), 2);
        assert_eq!(export.content, bibtex);
    }
}
