use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::infra::Result;

use super::{MemoryId, MemoryRecord, MemoryStore, knowledge_graph::extract_concepts};

/// A locally generated, source-linked concept page derived from stored memory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WikiPage {
    pub id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub concepts: Vec<String>,
    #[serde(default)]
    pub source_memory_ids: Vec<MemoryId>,
    pub updated_at: DateTime<Utc>,
}

/// A ranked local wiki search result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiSearchHit {
    pub page: WikiPage,
    pub score: f64,
}

/// Generates and searches local wiki pages from the durable memory corpus.
#[derive(Clone)]
pub struct LmWikiService {
    store: MemoryStore,
}

impl LmWikiService {
    /// Creates an always-fresh local wiki service over the supplied memory store.
    #[must_use]
    pub fn new(store: MemoryStore) -> Self {
        Self { store }
    }

    /// Returns every automatically generated concept page in deterministic ID order.
    pub fn pages(&self) -> Result<Vec<WikiPage>> {
        Ok(build_wiki_pages(&self.store.list()?))
    }

    /// Searches generated wiki pages using concept overlap and title/summary matches.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<WikiSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query_terms = extract_concepts(query).into_iter().collect::<BTreeSet<_>>();
        let normalized_query = query.trim().to_lowercase();
        let mut hits = self
            .pages()?
            .into_iter()
            .filter_map(|page| {
                if query_terms.is_empty() {
                    return Some(WikiSearchHit { page, score: 0.0 });
                }
                let page_terms = page.concepts.iter().cloned().collect::<BTreeSet<_>>();
                let overlap = query_terms.intersection(&page_terms).count();
                let title_or_summary_match = page.title.to_lowercase().contains(&normalized_query)
                    || page.summary.to_lowercase().contains(&normalized_query);
                let score = overlap as f64 / query_terms.len() as f64
                    + if title_or_summary_match { 0.25 } else { 0.0 };
                (score > 0.0).then_some(WikiSearchHit { page, score })
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.page.id.cmp(&right.page.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

/// Builds auto-generated local wiki pages grouped by extracted primary concept.
#[must_use]
pub fn build_wiki_pages(records: &[MemoryRecord]) -> Vec<WikiPage> {
    let mut groups = BTreeMap::<String, Vec<MemoryRecord>>::new();
    for record in records {
        for concept in extract_concepts(&record.content) {
            groups.entry(concept).or_default().push(record.clone());
        }
    }

    groups
        .into_iter()
        .map(|(concept, mut sources)| {
            sources.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.id.cmp(&right.id))
            });
            let related_concepts = sources
                .iter()
                .flat_map(|record| extract_concepts(&record.content))
                .filter(|candidate| candidate != &concept)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(8)
                .collect::<Vec<_>>();
            let source_memory_ids = sources.iter().map(|record| record.id).collect::<Vec<_>>();
            let modules = sources
                .iter()
                .map(|record| record.module_id.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let excerpts = sources
                .iter()
                .take(3)
                .map(|record| truncate_excerpt(&record.content, 150))
                .collect::<Vec<_>>();
            let updated_at = sources
                .iter()
                .map(|record| record.updated_at)
                .max()
                .expect("wiki pages always have at least one source");
            let mut concepts = Vec::with_capacity(related_concepts.len() + 1);
            concepts.push(concept.clone());
            concepts.extend(related_concepts);
            WikiPage {
                id: format!("wiki:{concept}"),
                title: format!("Concept: {concept}"),
                summary: format!(
                    "`{concept}` is grounded in {} memory record(s) across {}. Evidence: {}",
                    sources.len(),
                    modules.join(", "),
                    excerpts.join(" ")
                ),
                concepts,
                source_memory_ids,
                updated_at,
            }
        })
        .collect()
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

    use super::{LmWikiService, build_wiki_pages};
    use crate::memory::{MemoryStore, NewMemory};

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap()
    }

    #[test]
    fn builds_source_linked_concept_pages() {
        let store = MemoryStore::in_memory().unwrap();
        let first = store
            .store(
                NewMemory::new("notes", "Retrieval practice improves recall", "note"),
                timestamp(),
            )
            .unwrap();
        let second = store
            .store(
                NewMemory::new("notes", "Recall benefits from retrieval", "note"),
                timestamp(),
            )
            .unwrap();

        let pages = build_wiki_pages(&[first, second]);
        let retrieval = pages
            .iter()
            .find(|page| page.id == "wiki:retrieval")
            .unwrap();

        assert_eq!(retrieval.source_memory_ids.len(), 2);
        assert!(retrieval.summary.contains("Evidence:"));
    }

    #[test]
    fn searches_derived_pages_without_network_access() {
        let store = MemoryStore::in_memory().unwrap();
        store
            .store(
                NewMemory::new("notes", "Calculus limits require practice", "note"),
                timestamp(),
            )
            .unwrap();
        let wiki = LmWikiService::new(store);

        let hits = wiki.search("calculus", 3).unwrap();

        assert_eq!(hits[0].page.id, "wiki:calculus");
    }
}
