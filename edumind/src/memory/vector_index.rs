use std::collections::{BTreeMap, BTreeSet};

use crate::{
    infra::{EduMindError, Result},
    memory::{Embedding, MemoryId},
};

/// Provenance emitted for an approximate candidate that was exactly reranked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorIndexHit {
    pub memory_id: MemoryId,
    pub candidate_rank: usize,
    pub approximate_score: f32,
    pub exact_rerank_score: f32,
    pub index_generation: u64,
}

/// Local quantized candidate index with exact cosine reranking and stable provenance.
#[derive(Clone, Debug, Default)]
pub struct VectorIndex {
    entries: BTreeMap<MemoryId, Embedding>,
    buckets: BTreeMap<u8, BTreeSet<MemoryId>>,
    generation: u64,
}

impl VectorIndex {
    /// Inserts or replaces an embedding while maintaining a deterministic ANN bucket.
    pub fn insert(&mut self, memory_id: MemoryId, embedding: Embedding) -> Result<()> {
        if let Some(existing) = self.entries.values().next()
            && existing.dimensions() != embedding.dimensions()
        {
            return Err(EduMindError::InvalidEmbedding(format!(
                "vector index dimensions must remain {}; received {}",
                existing.dimensions(),
                embedding.dimensions()
            )));
        }
        if let Some(previous) = self.entries.get(&memory_id) {
            let previous_bucket = signature(previous);
            if let Some(ids) = self.buckets.get_mut(&previous_bucket) {
                ids.remove(&memory_id);
                if ids.is_empty() {
                    self.buckets.remove(&previous_bucket);
                }
            }
        }
        let bucket = signature(&embedding);
        self.entries.insert(memory_id, embedding);
        self.buckets.entry(bucket).or_default().insert(memory_id);
        self.generation = self.generation.saturating_add(1);
        Ok(())
    }

    /// Removes one embedding and increments the index generation when it existed.
    pub fn remove(&mut self, memory_id: MemoryId) {
        let Some(previous) = self.entries.remove(&memory_id) else {
            return;
        };
        let bucket = signature(&previous);
        if let Some(ids) = self.buckets.get_mut(&bucket) {
            ids.remove(&memory_id);
            if ids.is_empty() {
                self.buckets.remove(&bucket);
            }
        }
        self.generation = self.generation.saturating_add(1);
    }

    /// Returns the current stable generation for search provenance.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the number of indexed embeddings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Searches quantized ANN candidates and reranks only that candidate set exactly.
    pub fn search_ann(
        &self,
        query: &Embedding,
        candidate_limit: usize,
        limit: usize,
    ) -> Result<Vec<VectorIndexHit>> {
        if candidate_limit == 0 || limit == 0 {
            return Ok(Vec::new());
        }
        let candidate_ids = self.candidate_ids(query, candidate_limit);
        let mut candidates = candidate_ids
            .into_iter()
            .filter_map(|memory_id| {
                self.entries.get(&memory_id).and_then(|embedding| {
                    (embedding.dimensions() == query.dimensions())
                        .then(|| (memory_id, quantized_cosine(query, embedding)))
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        candidates.truncate(candidate_limit);

        let mut reranked = candidates
            .into_iter()
            .enumerate()
            .map(|(index, (memory_id, approximate_score))| {
                let exact_rerank_score = self
                    .entries
                    .get(&memory_id)
                    .expect("candidate IDs originate from the index")
                    .cosine_similarity(query)?;
                Ok(VectorIndexHit {
                    memory_id,
                    candidate_rank: index + 1,
                    approximate_score,
                    exact_rerank_score,
                    index_generation: self.generation,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        reranked.sort_by(|left, right| {
            right
                .exact_rerank_score
                .total_cmp(&left.exact_rerank_score)
                .then_with(|| left.candidate_rank.cmp(&right.candidate_rank))
                .then_with(|| left.memory_id.cmp(&right.memory_id))
        });
        reranked.truncate(limit);
        Ok(reranked)
    }

    fn candidate_ids(&self, query: &Embedding, candidate_limit: usize) -> Vec<MemoryId> {
        let query_signature = signature(query);
        let mut ids = Vec::new();
        for distance in 0..=8 {
            for (bucket, bucket_ids) in &self.buckets {
                if (bucket ^ query_signature).count_ones() == distance {
                    ids.extend(bucket_ids.iter().copied());
                    if ids.len() >= candidate_limit {
                        return ids;
                    }
                }
            }
        }
        if ids.len() < candidate_limit {
            for memory_id in self.entries.keys() {
                if !ids.contains(memory_id) {
                    ids.push(*memory_id);
                }
                if ids.len() >= candidate_limit {
                    break;
                }
            }
        }
        ids
    }
}

fn signature(embedding: &Embedding) -> u8 {
    embedding
        .values
        .iter()
        .take(8)
        .enumerate()
        .fold(0_u8, |signature, (index, value)| {
            if *value >= 0.0 {
                signature | (1_u8 << index)
            } else {
                signature
            }
        })
}

fn quantized_cosine(left: &Embedding, right: &Embedding) -> f32 {
    let (dot, left_norm, right_norm) = left.values.iter().zip(&right.values).fold(
        (0_i64, 0_i64, 0_i64),
        |(dot, left_norm, right_norm), (left_value, right_value)| {
            let left_value = quantize(*left_value);
            let right_value = quantize(*right_value);
            (
                dot + i64::from(left_value) * i64::from(right_value),
                left_norm + i64::from(left_value) * i64::from(left_value),
                right_norm + i64::from(right_value) * i64::from(right_value),
            )
        },
    );
    if left_norm == 0 || right_norm == 0 {
        return 0.0;
    }
    dot as f32 / ((left_norm as f32).sqrt() * (right_norm as f32).sqrt())
}

fn quantize(value: f32) -> i8 {
    (value.clamp(-1.0, 1.0) * 127.0).round() as i8
}

#[cfg(test)]
mod tests {
    use crate::memory::{Embedding, MemoryId, VectorIndex};

    #[test]
    fn candidates_are_exactly_reranked_with_stable_provenance() {
        let mut index = VectorIndex::default();
        let first = MemoryId::new();
        let second = MemoryId::new();
        index
            .insert(first, Embedding::new("test", vec![1.0, 0.0, 0.0]).unwrap())
            .unwrap();
        index
            .insert(second, Embedding::new("test", vec![0.0, 1.0, 0.0]).unwrap())
            .unwrap();

        let hits = index
            .search_ann(&Embedding::new("test", vec![1.0, 0.0, 0.0]).unwrap(), 2, 2)
            .unwrap();

        assert_eq!(hits[0].memory_id, first);
        assert_eq!(hits[0].candidate_rank, 1);
        assert!(hits[0].exact_rerank_score > 0.99);
        assert!(hits[0].index_generation >= 2);
    }
}
