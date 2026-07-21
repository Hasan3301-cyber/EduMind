use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::infra::Result;

use super::{
    KnowledgeGraphService, LmWikiService, MemoryRecord, ModuleMemoryService, NewModuleMemory,
    extract_concepts,
};

/// Inspectable result of one local memory-ingestion pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryIngestionResult {
    pub record: MemoryRecord,
    #[serde(default)]
    pub extracted_concepts: Vec<String>,
    pub graph_node_count: usize,
    pub wiki_page_count: usize,
}

/// Coordinates embedded memory storage with derived knowledge graph and wiki views.
#[derive(Clone)]
pub struct MemoryIngestionPipeline {
    modules: ModuleMemoryService,
    graph: KnowledgeGraphService,
    wiki: LmWikiService,
}

impl MemoryIngestionPipeline {
    /// Creates an ingestion pipeline over shared module, graph, and wiki services.
    #[must_use]
    pub fn new(
        modules: ModuleMemoryService,
        graph: KnowledgeGraphService,
        wiki: LmWikiService,
    ) -> Self {
        Self {
            modules,
            graph,
            wiki,
        }
    }

    /// Stores one embedded module memory and returns its immediately refreshed derived coverage.
    pub async fn ingest(
        &self,
        module_id: impl AsRef<str>,
        input: NewModuleMemory,
        now: DateTime<Utc>,
    ) -> Result<MemoryIngestionResult> {
        let record = self.modules.store(module_id, input, now).await?;
        let extracted_concepts = extract_concepts(&record.content);
        let graph_node_count = self.graph.graph()?.nodes.len();
        let wiki_page_count = self.wiki.pages()?.len();
        Ok(MemoryIngestionResult {
            record,
            extracted_concepts,
            graph_node_count,
            wiki_page_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};

    use super::MemoryIngestionPipeline;
    use crate::memory::{
        HashEmbedder, HybridMemory, KnowledgeGraphService, LmWikiService, MemoryStore,
        ModuleMemoryService, NewModuleMemory,
    };

    #[tokio::test]
    async fn ingests_embeds_and_refreshes_derived_memory_views() {
        let store = MemoryStore::in_memory().unwrap();
        let hybrid =
            HybridMemory::new(store.clone(), Arc::new(HashEmbedder::new(64).unwrap())).unwrap();
        let pipeline = MemoryIngestionPipeline::new(
            ModuleMemoryService::new(hybrid),
            KnowledgeGraphService::new(store.clone()),
            LmWikiService::new(store),
        );
        let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();

        let result = pipeline
            .ingest(
                "class-notes",
                NewModuleMemory::new("Retrieval practice improves recall", "note"),
                now,
            )
            .await
            .unwrap();

        assert_eq!(result.record.module_id, "class-notes");
        assert!(result.extracted_concepts.contains(&"retrieval".to_owned()));
        assert!(result.graph_node_count >= 2);
        assert!(result.wiki_page_count >= 1);
    }
}
