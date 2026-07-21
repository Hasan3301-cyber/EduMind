use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use chrono::{DateTime, Utc};

use crate::{
    infra::{EduMindError, Result},
    memory::{MemoryId, MemoryRecord, MemoryStore, NewMemory, TextEmbedder, VectorIndex},
};

const LEXICAL_WEIGHT: f32 = 0.45;
const VECTOR_WEIGHT: f32 = 0.55;

/// A merged lexical and semantic memory-search result.
#[derive(Clone, Debug, PartialEq)]
pub struct HybridSearchHit {
    pub record: MemoryRecord,
    pub lexical_score: f32,
    pub vector_similarity: Option<f32>,
    pub candidate_rank: Option<usize>,
    pub exact_rerank_score: Option<f32>,
    pub index_generation: u64,
    pub score: f32,
}

/// Coordinates durable memory, a text embedder, and an exact vector index.
#[derive(Clone)]
pub struct HybridMemory {
    store: MemoryStore,
    embedder: Arc<dyn TextEmbedder>,
    vectors: Arc<RwLock<VectorIndex>>,
}

impl HybridMemory {
    /// Creates a memory service and restores its exact index from persisted embeddings.
    pub fn new(store: MemoryStore, embedder: Arc<dyn TextEmbedder>) -> Result<Self> {
        let memory = Self {
            store,
            embedder,
            vectors: Arc::new(RwLock::new(VectorIndex::default())),
        };
        memory.rebuild_index()?;
        Ok(memory)
    }

    /// Returns the durable store used by this service.
    #[must_use]
    pub fn store_handle(&self) -> &MemoryStore {
        &self.store
    }

    /// Reconstructs the exact vector index from all persisted embedding rows.
    pub fn rebuild_index(&self) -> Result<()> {
        let mut rebuilt = VectorIndex::default();
        for stored in self.store.list_embeddings()? {
            rebuilt.insert(stored.memory_id, stored.embedding)?;
        }
        *self.write_vectors()? = rebuilt;
        Ok(())
    }

    /// Embeds, persists, and indexes one memory atomically at the application level.
    pub async fn store_memory(&self, input: NewMemory, now: DateTime<Utc>) -> Result<MemoryRecord> {
        let embedding = self.embedder.embed(&input.content).await?;
        if embedding.dimensions() != self.embedder.dimensions() {
            return Err(EduMindError::InvalidEmbedding(format!(
                "embedder returned {} dimensions; expected {}",
                embedding.dimensions(),
                self.embedder.dimensions()
            )));
        }
        let record = self.store.store(input, now)?;
        self.store.upsert_embedding(record.id, &embedding, now)?;
        self.write_vectors()?.insert(record.id, embedding)?;
        Ok(record)
    }

    /// Re-embeds, persists, and re-indexes an existing memory record.
    pub async fn update_memory(
        &self,
        memory_id: MemoryId,
        input: NewMemory,
        now: DateTime<Utc>,
    ) -> Result<Option<MemoryRecord>> {
        let embedding = self.embedder.embed(&input.content).await?;
        if embedding.dimensions() != self.embedder.dimensions() {
            return Err(EduMindError::InvalidEmbedding(format!(
                "embedder returned {} dimensions; expected {}",
                embedding.dimensions(),
                self.embedder.dimensions()
            )));
        }
        let Some(record) = self.store.update(memory_id, input, now)? else {
            return Ok(None);
        };
        self.store.upsert_embedding(memory_id, &embedding, now)?;
        self.write_vectors()?.insert(memory_id, embedding)?;
        Ok(Some(record))
    }

    /// Deletes a memory record from persistent and in-memory indexes.
    pub fn delete_memory(&self, memory_id: MemoryId) -> Result<bool> {
        let deleted = self.store.delete(memory_id)?;
        if deleted {
            self.write_vectors()?.remove(memory_id);
        }
        Ok(deleted)
    }

    /// Executes lexical and semantic search, merging scores deterministically.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<HybridSearchHit>> {
        if limit == 0 || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let candidate_limit = limit.saturating_mul(4).max(limit);
        let lexical_hits = self.store.search_lexical(query, candidate_limit)?;
        let query_embedding = self.embedder.embed(query).await?;
        let vector_index = self.read_vectors()?;
        let index_generation = vector_index.generation();
        let vector_hits =
            vector_index.search_ann(&query_embedding, candidate_limit, candidate_limit)?;
        drop(vector_index);

        let mut records = BTreeMap::new();
        let mut candidates = BTreeMap::<MemoryId, CandidateScores>::new();
        for hit in lexical_hits {
            let memory_id = hit.record.id;
            records.insert(memory_id, hit.record);
            candidates.entry(memory_id).or_default().lexical_score = hit.score;
        }
        for hit in vector_hits {
            let candidate = candidates.entry(hit.memory_id).or_default();
            candidate.vector_similarity = Some(hit.exact_rerank_score);
            candidate.candidate_rank = Some(hit.candidate_rank);
            candidate.exact_rerank_score = Some(hit.exact_rerank_score);
            candidate.index_generation = hit.index_generation;
        }

        let mut results = Vec::new();
        for (memory_id, scores) in candidates {
            let record = match records.remove(&memory_id) {
                Some(record) => record,
                None => match self.store.get(memory_id)? {
                    Some(record) => record,
                    None => continue,
                },
            };
            let vector_score = scores
                .vector_similarity
                .map_or(0.0, |similarity| ((similarity + 1.0) / 2.0).clamp(0.0, 1.0));
            results.push(HybridSearchHit {
                record,
                lexical_score: scores.lexical_score,
                vector_similarity: scores.vector_similarity,
                candidate_rank: scores.candidate_rank,
                exact_rerank_score: scores.exact_rerank_score,
                index_generation: scores.index_generation.max(index_generation),
                score: LEXICAL_WEIGHT * scores.lexical_score + VECTOR_WEIGHT * vector_score,
            });
        }
        results.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.record.id.cmp(&right.record.id))
        });
        results.truncate(limit);
        Ok(results)
    }

    /// Returns the number of currently indexed vector entries.
    pub fn indexed_count(&self) -> Result<usize> {
        Ok(self.read_vectors()?.len())
    }

    fn read_vectors(&self) -> Result<RwLockReadGuard<'_, VectorIndex>> {
        self.vectors
            .read()
            .map_err(|error| EduMindError::MemoryLock(error.to_string()))
    }

    fn write_vectors(&self) -> Result<RwLockWriteGuard<'_, VectorIndex>> {
        self.vectors
            .write()
            .map_err(|error| EduMindError::MemoryLock(error.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CandidateScores {
    lexical_score: f32,
    vector_similarity: Option<f32>,
    candidate_rank: Option<usize>,
    exact_rerank_score: Option<f32>,
    index_generation: u64,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};

    use crate::memory::{HashEmbedder, HybridMemory, MemoryStore, NewMemory};

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn combines_lexical_and_vector_results() {
        let store = MemoryStore::in_memory().unwrap();
        let memory = HybridMemory::new(store, Arc::new(HashEmbedder::new(64).unwrap())).unwrap();
        let calculus = memory
            .store_memory(
                NewMemory::new("class-notes", "Calculus limits revision", "note"),
                timestamp(),
            )
            .await
            .unwrap();
        memory
            .store_memory(
                NewMemory::new("class-notes", "Cell biology flashcards", "note"),
                timestamp(),
            )
            .await
            .unwrap();

        let results = memory.search("calculus", 2).await.unwrap();

        assert_eq!(results[0].record.id, calculus.id);
        assert!(results[0].lexical_score > 0.0);
        assert!(results[0].vector_similarity.is_some());
        assert_eq!(memory.indexed_count().unwrap(), 2);
    }

    #[tokio::test]
    async fn rebuilds_the_index_from_persisted_embeddings() {
        let store = MemoryStore::in_memory().unwrap();
        let embedder = Arc::new(HashEmbedder::new(32).unwrap());
        let memory = HybridMemory::new(store.clone(), embedder.clone()).unwrap();
        let record = memory
            .store_memory(
                NewMemory::new("research", "Graph neural network literature", "paper"),
                timestamp(),
            )
            .await
            .unwrap();

        let restored = HybridMemory::new(store, embedder).unwrap();
        let results = restored.search("graph neural", 1).await.unwrap();

        assert_eq!(restored.indexed_count().unwrap(), 1);
        assert_eq!(results[0].record.id, record.id);
    }

    #[tokio::test]
    async fn updates_replace_the_searchable_embedding_and_content() {
        let store = MemoryStore::in_memory().unwrap();
        let memory = HybridMemory::new(store, Arc::new(HashEmbedder::new(64).unwrap())).unwrap();
        let record = memory
            .store_memory(
                NewMemory::new("class-notes", "Calculus limits revision", "note"),
                timestamp(),
            )
            .await
            .unwrap();

        let updated = memory
            .update_memory(
                record.id,
                NewMemory::new("class-notes", "Organic chemistry reactions", "note"),
                timestamp(),
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.content, "Organic chemistry reactions");
        assert!(
            memory
                .store_handle()
                .search_lexical("calculus", 5)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            memory.search("chemistry", 1).await.unwrap()[0].record.id,
            record.id
        );
    }
}
