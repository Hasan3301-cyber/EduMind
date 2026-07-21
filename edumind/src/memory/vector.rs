use std::collections::BTreeMap;

use crate::{
    infra::{EduMindError, Result},
    memory::{Embedding, MemoryId},
};

/// A deterministic in-memory exact vector index keyed by persistent memory IDs.
#[derive(Clone, Debug, Default)]
pub struct VectorStore {
    entries: BTreeMap<MemoryId, Embedding>,
}

impl VectorStore {
    /// Inserts or replaces an embedding while enforcing one shared dimensionality.
    pub fn insert(&mut self, memory_id: MemoryId, embedding: Embedding) -> Result<()> {
        if let Some(existing) = self.entries.values().next()
            && existing.dimensions() != embedding.dimensions()
        {
            return Err(EduMindError::InvalidEmbedding(format!(
                "vector store dimensions must remain {}; received {}",
                existing.dimensions(),
                embedding.dimensions()
            )));
        }
        self.entries.insert(memory_id, embedding);
        Ok(())
    }

    /// Removes an embedding from the index.
    pub fn remove(&mut self, memory_id: MemoryId) {
        self.entries.remove(&memory_id);
    }

    /// Returns the number of indexed memories.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the index contains no embeddings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Searches using exact cosine similarity with deterministic tie-breaking.
    pub fn search(&self, query: &Embedding, limit: usize) -> Result<Vec<VectorSearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut hits = self
            .entries
            .iter()
            .filter(|(_, embedding)| embedding.dimensions() == query.dimensions())
            .map(|(memory_id, embedding)| {
                embedding
                    .cosine_similarity(query)
                    .map(|similarity| VectorSearchHit {
                        memory_id: *memory_id,
                        similarity,
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        hits.sort_by(|left, right| {
            right
                .similarity
                .total_cmp(&left.similarity)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

/// One exact vector search result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorSearchHit {
    pub memory_id: MemoryId,
    pub similarity: f32,
}

#[cfg(test)]
mod tests {
    use crate::memory::{Embedding, MemoryId, VectorStore};

    #[test]
    fn ranks_exact_vector_matches_first() {
        let mut store = VectorStore::default();
        let first = MemoryId::new();
        let second = MemoryId::new();
        store
            .insert(first, Embedding::new("test", vec![1.0, 0.0]).unwrap())
            .unwrap();
        store
            .insert(second, Embedding::new("test", vec![0.0, 1.0]).unwrap())
            .unwrap();

        let hits = store
            .search(&Embedding::new("test", vec![1.0, 0.0]).unwrap(), 2)
            .unwrap();

        assert_eq!(hits[0].memory_id, first);
        assert_eq!(hits.len(), 2);
    }
}
