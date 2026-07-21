use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::{config::types::HermesConfig, infra::Result};

use super::{
    AdvancedSearchHit, AdvancedSearchService, GraphNeighborhood, GraphSearchHit, HermesCycle,
    HermesLearningLoop, KnowledgeGraphService, LmWikiService, MemoryId, MemoryIngestionPipeline,
    MemoryIngestionResult, MemoryRecord, MemoryStore, ModuleMemoryService, NewModuleMemory,
    WikiPage, WikiSearchHit,
};

/// A compact observable snapshot of local memory intelligence coverage.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryIntelligenceSnapshot {
    pub memory_count: usize,
    pub indexed_memory_count: usize,
    pub graph_node_count: usize,
    pub graph_edge_count: usize,
    pub graph_community_count: usize,
    pub wiki_page_count: usize,
}

/// Higher-level local memory services: scoped storage, reranking, graph, wiki, and Hermes.
#[derive(Clone)]
pub struct MemoryIntelligence {
    store: MemoryStore,
    modules: ModuleMemoryService,
    advanced_search: AdvancedSearchService,
    graph: KnowledgeGraphService,
    wiki: LmWikiService,
    hermes: HermesLearningLoop,
    pipeline: MemoryIngestionPipeline,
}

impl MemoryIntelligence {
    /// Creates a coherent memory-intelligence service around one hybrid local memory index.
    pub fn new(memory: super::HybridMemory, hermes_config: HermesConfig) -> Result<Self> {
        let store = memory.store_handle().clone();
        let modules = ModuleMemoryService::new(memory.clone());
        let advanced_search = AdvancedSearchService::new(memory);
        let graph = KnowledgeGraphService::new(store.clone());
        let wiki = LmWikiService::new(store.clone());
        let hermes = HermesLearningLoop::new(store.clone(), hermes_config)?;
        let pipeline = MemoryIngestionPipeline::new(modules.clone(), graph.clone(), wiki.clone());
        Ok(Self {
            store,
            modules,
            advanced_search,
            graph,
            wiki,
            hermes,
            pipeline,
        })
    }

    /// Runs the standard embedded-memory ingestion pipeline for one module record.
    pub async fn ingest(
        &self,
        module_id: impl AsRef<str>,
        input: NewModuleMemory,
        now: DateTime<Utc>,
    ) -> Result<MemoryIngestionResult> {
        self.pipeline.ingest(module_id, input, now).await
    }

    /// Runs advanced hybrid retrieval with deterministic Jaccard/MMR reranking.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<AdvancedSearchHit>> {
        self.advanced_search.search(query, limit).await
    }

    /// Loads one raw memory record by stable ID.
    pub fn get(&self, memory_id: MemoryId) -> Result<Option<MemoryRecord>> {
        self.store.get(memory_id)
    }

    /// Lists raw memory records in reverse update order for local administration.
    pub fn list(&self) -> Result<Vec<MemoryRecord>> {
        self.store.list()
    }

    /// Returns a module-scoped memory service with visibility enforcement.
    #[must_use]
    pub fn modules(&self) -> ModuleMemoryService {
        self.modules.clone()
    }

    /// Searches generated local wiki pages.
    pub fn search_wiki(&self, query: &str, limit: usize) -> Result<Vec<WikiSearchHit>> {
        self.wiki.search(query, limit)
    }

    /// Lists generated local wiki pages.
    pub fn wiki_pages(&self) -> Result<Vec<WikiPage>> {
        self.wiki.pages()
    }

    /// Searches knowledge-graph nodes by extracted concepts and labels.
    pub fn search_graph(&self, query: &str, limit: usize) -> Result<Vec<GraphSearchHit>> {
        self.graph.search(query, limit)
    }

    /// Gets a selected knowledge-graph node's direct neighborhood.
    pub fn graph_neighbors(&self, node_id: &str) -> Result<Option<GraphNeighborhood>> {
        self.graph.neighbors(node_id)
    }

    /// Builds the full current knowledge graph for visualization clients.
    pub fn graph(&self) -> Result<edumind_core::GraphData> {
        self.graph.graph()
    }

    /// Lists persisted Hermes learning cycles.
    pub fn hermes_cycles(&self, limit: usize) -> Result<Vec<HermesCycle>> {
        self.hermes.cycles(limit)
    }

    /// Starts Hermes when enabled; callers own the returned task lifecycle.
    #[must_use]
    pub fn spawn_hermes(&self) -> Option<JoinHandle<()>> {
        self.hermes.spawn()
    }

    /// Returns current memory, graph, wiki, and vector-index coverage for observability.
    pub fn snapshot(&self) -> Result<MemoryIntelligenceSnapshot> {
        let graph = self.graph.graph()?;
        Ok(MemoryIntelligenceSnapshot {
            memory_count: self.store.list()?.len(),
            indexed_memory_count: self.advanced_search.hybrid_memory().indexed_count()?,
            graph_node_count: graph.nodes.len(),
            graph_edge_count: graph.edges.len(),
            graph_community_count: graph.communities.len(),
            wiki_page_count: self.wiki.pages()?.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};

    use super::MemoryIntelligence;
    use crate::{
        config::types::HermesConfig,
        memory::{HashEmbedder, HybridMemory, MemoryStore, NewModuleMemory},
    };

    #[tokio::test]
    async fn composes_ingestion_search_and_observability() {
        let memory = HybridMemory::new(
            MemoryStore::in_memory().unwrap(),
            Arc::new(HashEmbedder::new(64).unwrap()),
        )
        .unwrap();
        let intelligence = MemoryIntelligence::new(memory, HermesConfig::default()).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();

        intelligence
            .ingest(
                "notes",
                NewModuleMemory::new("Calculus limits revision plan", "note"),
                now,
            )
            .await
            .unwrap();
        let search = intelligence.search("calculus", 3).await.unwrap();
        let snapshot = intelligence.snapshot().unwrap();

        assert_eq!(search.len(), 1);
        assert_eq!(snapshot.memory_count, 1);
        assert!(snapshot.graph_node_count >= 2);
        assert!(snapshot.wiki_page_count >= 1);
    }
}
