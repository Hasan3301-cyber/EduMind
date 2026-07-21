use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::infra::{EduMindError, Result};

/// A validated embedding together with the model that produced it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Embedding {
    pub model: String,
    pub values: Vec<f32>,
}

impl Embedding {
    /// Creates an embedding after validating model identity and numeric values.
    pub fn new(model: impl Into<String>, values: Vec<f32>) -> Result<Self> {
        let model = model.into();
        if model.trim().is_empty() {
            return Err(EduMindError::InvalidEmbedding(
                "model must not be empty".to_owned(),
            ));
        }
        if values.is_empty() {
            return Err(EduMindError::InvalidEmbedding(
                "embedding must contain at least one dimension".to_owned(),
            ));
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(EduMindError::InvalidEmbedding(
                "embedding values must be finite".to_owned(),
            ));
        }
        Ok(Self { model, values })
    }

    /// Returns the number of scalar dimensions in this embedding.
    #[must_use]
    pub fn dimensions(&self) -> usize {
        self.values.len()
    }

    /// Calculates cosine similarity, returning zero when either vector is zero.
    pub fn cosine_similarity(&self, other: &Self) -> Result<f32> {
        if self.dimensions() != other.dimensions() {
            return Err(EduMindError::InvalidEmbedding(format!(
                "dimension mismatch: {} != {}",
                self.dimensions(),
                other.dimensions()
            )));
        }
        let (dot, left_norm, right_norm) = self.values.iter().zip(&other.values).fold(
            (0.0_f32, 0.0_f32, 0.0_f32),
            |(dot, left_norm, right_norm), (left, right)| {
                (
                    dot + left * right,
                    left_norm + left * left,
                    right_norm + right * right,
                )
            },
        );
        if left_norm == 0.0 || right_norm == 0.0 {
            return Ok(0.0);
        }
        Ok(dot / (left_norm.sqrt() * right_norm.sqrt()))
    }
}

/// Embedding provider contract used by persistent memory and retrieval.
#[async_trait]
pub trait TextEmbedder: Send + Sync {
    /// Embeds text into the provider's configured vector space.
    async fn embed(&self, text: &str) -> Result<Embedding>;

    /// Returns the configured provider model identifier.
    fn model(&self) -> &str;

    /// Returns the expected embedding dimensions.
    fn dimensions(&self) -> usize;
}

/// Deterministic, local embedding provider used for offline operation and tests.
#[derive(Clone, Debug)]
pub struct HashEmbedder {
    model: String,
    dimensions: usize,
}

impl HashEmbedder {
    /// Creates a deterministic hash embedder with the requested dimensions.
    pub fn new(dimensions: usize) -> Result<Self> {
        if dimensions == 0 {
            return Err(EduMindError::InvalidEmbedding(
                "hash embedder dimensions must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            model: format!("hash-v1-{dimensions}"),
            dimensions,
        })
    }

    fn embed_deterministically(&self, text: &str) -> Result<Embedding> {
        let mut values = vec![0.0_f32; self.dimensions];
        for token in text.split_whitespace().map(str::to_lowercase) {
            let digest = Sha256::digest(token.as_bytes());
            let bucket = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) as usize
                % self.dimensions;
            let sign = if digest[4] & 1 == 0 { 1.0 } else { -1.0 };
            values[bucket] += sign;
        }
        normalize(&mut values);
        Embedding::new(self.model.clone(), values)
    }
}

#[async_trait]
impl TextEmbedder for HashEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        self.embed_deterministically(text)
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

/// Ollama-compatible remote embedding provider.
#[derive(Clone, Debug)]
pub struct OllamaEmbedder {
    client: Client,
    endpoint: String,
    model: String,
    dimensions: usize,
}

impl OllamaEmbedder {
    /// Creates an embedder that sends requests to the Ollama `/api/embed` endpoint.
    pub fn new(
        client: Client,
        endpoint: impl Into<String>,
        model: impl Into<String>,
        dimensions: usize,
    ) -> Result<Self> {
        let endpoint = endpoint.into();
        let model = model.into();
        validate_remote_config(&endpoint, &model, dimensions, "Ollama")?;
        Ok(Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            model,
            dimensions,
        })
    }
}

#[async_trait]
impl TextEmbedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        let response = self
            .client
            .post(format!("{}/api/embed", self.endpoint))
            .json(&OllamaEmbedRequest {
                model: &self.model,
                input: text,
            })
            .send()
            .await?
            .error_for_status()?
            .json::<OllamaEmbedResponse>()
            .await?;
        let values = response
            .embeddings
            .and_then(|mut embeddings| embeddings.pop())
            .or(response.embedding)
            .ok_or_else(|| {
                EduMindError::InvalidEmbedding("Ollama returned no embedding".to_owned())
            })?;
        embedding_from_remote(self.model.clone(), values, self.dimensions)
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[derive(Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    #[serde(default)]
    embeddings: Option<Vec<Vec<f32>>>,
    #[serde(default)]
    embedding: Option<Vec<f32>>,
}

/// OpenAI-compatible remote embedding provider.
#[derive(Clone, Debug)]
pub struct OpenAiCompatibleEmbedder {
    client: Client,
    endpoint: String,
    api_key: Option<String>,
    model: String,
    dimensions: usize,
}

impl OpenAiCompatibleEmbedder {
    /// Creates an embedder for an OpenAI-compatible `/embeddings` endpoint.
    pub fn new(
        client: Client,
        endpoint: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        dimensions: usize,
    ) -> Result<Self> {
        let endpoint = endpoint.into();
        let model = model.into();
        validate_remote_config(&endpoint, &model, dimensions, "OpenAI-compatible")?;
        Ok(Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
            model,
            dimensions,
        })
    }
}

#[async_trait]
impl TextEmbedder for OpenAiCompatibleEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        let request = self
            .client
            .post(format!("{}/embeddings", self.endpoint))
            .json(&OpenAiEmbedRequest {
                model: &self.model,
                input: text,
            });
        let request = if let Some(api_key) = &self.api_key {
            request.bearer_auth(api_key)
        } else {
            request
        };
        let response = request
            .send()
            .await?
            .error_for_status()?
            .json::<OpenAiEmbedResponse>()
            .await?;
        let values = response
            .data
            .into_iter()
            .next()
            .map(|entry| entry.embedding)
            .ok_or_else(|| {
                EduMindError::InvalidEmbedding(
                    "OpenAI-compatible provider returned no embedding".to_owned(),
                )
            })?;
        embedding_from_remote(self.model.clone(), values, self.dimensions)
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[derive(Serialize)]
struct OpenAiEmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
}

/// Wraps a remote embedder with a deterministic local fallback.
#[derive(Clone)]
pub struct FallbackEmbedder {
    primary: Arc<dyn TextEmbedder>,
    fallback: Arc<dyn TextEmbedder>,
}

impl FallbackEmbedder {
    /// Creates a fallback chain whose providers share one vector dimensionality.
    pub fn new(primary: Arc<dyn TextEmbedder>, fallback: Arc<dyn TextEmbedder>) -> Result<Self> {
        if primary.dimensions() != fallback.dimensions() {
            return Err(EduMindError::InvalidEmbedding(format!(
                "fallback dimension mismatch: {} != {}",
                primary.dimensions(),
                fallback.dimensions()
            )));
        }
        Ok(Self { primary, fallback })
    }
}

#[async_trait]
impl TextEmbedder for FallbackEmbedder {
    async fn embed(&self, text: &str) -> Result<Embedding> {
        match self.primary.embed(text).await {
            Ok(embedding) => Ok(embedding),
            Err(_) => self.fallback.embed(text).await,
        }
    }

    fn model(&self) -> &str {
        self.primary.model()
    }

    fn dimensions(&self) -> usize {
        self.primary.dimensions()
    }
}

fn validate_remote_config(
    endpoint: &str,
    model: &str,
    dimensions: usize,
    provider: &str,
) -> Result<()> {
    if endpoint.trim().is_empty() {
        return Err(EduMindError::InvalidEmbedding(format!(
            "{provider} endpoint must not be empty"
        )));
    }
    if model.trim().is_empty() {
        return Err(EduMindError::InvalidEmbedding(format!(
            "{provider} model must not be empty"
        )));
    }
    if dimensions == 0 {
        return Err(EduMindError::InvalidEmbedding(format!(
            "{provider} dimensions must be greater than zero"
        )));
    }
    Ok(())
}

fn embedding_from_remote(model: String, values: Vec<f32>, dimensions: usize) -> Result<Embedding> {
    if values.len() != dimensions {
        return Err(EduMindError::InvalidEmbedding(format!(
            "remote provider returned {} dimensions; expected {dimensions}",
            values.len()
        )));
    }
    Embedding::new(model, values)
}

fn normalize(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        values.iter_mut().for_each(|value| *value /= norm);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::{Embedding, FallbackEmbedder, HashEmbedder, TextEmbedder};
    use crate::infra::{EduMindError, Result};

    #[tokio::test]
    async fn hash_embeddings_are_deterministic_and_normalized() {
        let embedder = HashEmbedder::new(32).unwrap();
        let first = embedder.embed("calculus revision plan").await.unwrap();
        let second = embedder.embed("calculus revision plan").await.unwrap();

        assert_eq!(first, second);
        assert!((first.cosine_similarity(&first).unwrap() - 1.0).abs() < 0.000_1);
    }

    #[test]
    fn cosine_similarity_rejects_mismatched_dimensions() {
        let first = Embedding::new("test", vec![1.0, 0.0]).unwrap();
        let second = Embedding::new("test", vec![1.0]).unwrap();

        assert!(first.cosine_similarity(&second).is_err());
    }

    struct FailingEmbedder;

    #[async_trait]
    impl TextEmbedder for FailingEmbedder {
        async fn embed(&self, _text: &str) -> Result<Embedding> {
            Err(EduMindError::InvalidEmbedding("offline".to_owned()))
        }

        fn model(&self) -> &str {
            "failing"
        }

        fn dimensions(&self) -> usize {
            16
        }
    }

    #[tokio::test]
    async fn fallback_uses_local_embedder_after_primary_failure() {
        let fallback = Arc::new(HashEmbedder::new(16).unwrap());
        let embedder = FallbackEmbedder::new(Arc::new(FailingEmbedder), fallback).unwrap();

        assert_eq!(
            embedder.embed("offline study").await.unwrap().dimensions(),
            16
        );
    }
}
