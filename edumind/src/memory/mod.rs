//! Local-first persistent memory, deterministic embeddings, and hybrid search.

pub mod advanced_search;
pub mod collab;
pub mod embedder;
pub mod hermes;
pub mod hybrid;
pub mod knowledge_graph;
pub mod lm_wiki;
pub mod memory_intelligence;
pub mod module_memory;
pub mod pipeline;
pub mod privacy;
pub mod store;
pub mod vector;
pub mod vector_index;

pub use advanced_search::{AdvancedSearchHit, AdvancedSearchService, rerank_hits};
pub use collab::{
    CollaborationEvent, CollaborationEventId, CollaborationMember, CollaborationSession,
    CollaborationSessionId, NewCollaborationEvent, NewCollaborationSession,
};
pub use embedder::{
    Embedding, FallbackEmbedder, HashEmbedder, OllamaEmbedder, OpenAiCompatibleEmbedder,
    TextEmbedder,
};
pub use hermes::{HermesCycle, HermesLearningLoop, HermesSkillInsight};
pub use hybrid::{HybridMemory, HybridSearchHit};
pub use knowledge_graph::{
    GraphNeighborhood, GraphSearchHit, KnowledgeGraphService, build_knowledge_graph,
    extract_concepts,
};
pub use lm_wiki::{LmWikiService, WikiPage, WikiSearchHit, build_wiki_pages};
pub use memory_intelligence::{MemoryIntelligence, MemoryIntelligenceSnapshot};
pub use module_memory::{
    ModuleMemoryHit, ModuleMemoryScope, ModuleMemoryService, ModuleMemorySummary,
    ModuleMemorySummaryEntry, NewModuleMemory,
};
pub use pipeline::{MemoryIngestionPipeline, MemoryIngestionResult};
pub use privacy::{
    DataClassification, EncryptedMemoryEnvelope, MemoryPrivacyService, SecureDeletionRecord,
};
pub use store::{
    LexicalSearchHit, MemoryId, MemoryRecord, MemoryStore, NewMemory, StoredEmbedding,
};
pub use vector::{VectorSearchHit, VectorStore};
pub use vector_index::{VectorIndex, VectorIndexHit};
