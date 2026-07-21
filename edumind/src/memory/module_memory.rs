use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    infra::{EduMindError, Result},
    memory::MemoryId,
};

use super::{HybridMemory, MemoryRecord, NewMemory};

/// Visibility scope for a module-memory record.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleMemoryScope {
    Private,
    #[default]
    Module,
    CrossModule,
    Global,
}

/// Input for storing one memory record in a module namespace.
#[derive(Clone, Debug, PartialEq)]
pub struct NewModuleMemory {
    pub content: String,
    pub content_type: String,
    pub scope: ModuleMemoryScope,
    pub metadata: Value,
}

impl NewModuleMemory {
    /// Creates a module-memory input with an empty metadata object and module-only visibility.
    #[must_use]
    pub fn new(content: impl Into<String>, content_type: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            content_type: content_type.into(),
            scope: ModuleMemoryScope::Module,
            metadata: Value::Object(Map::new()),
        }
    }
}

/// A module-visible semantic retrieval result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModuleMemoryHit {
    pub record: MemoryRecord,
    pub scope: ModuleMemoryScope,
    pub lexical_score: f32,
    #[serde(default)]
    pub vector_similarity: Option<f32>,
    pub score: f32,
}

/// A compact record excerpt included in a module-memory summary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModuleMemorySummaryEntry {
    pub id: MemoryId,
    pub content_type: String,
    pub scope: ModuleMemoryScope,
    pub excerpt: String,
    pub updated_at: DateTime<Utc>,
}

/// A deterministic module-memory inventory for status panels and agents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModuleMemorySummary {
    pub module_id: String,
    pub record_count: usize,
    #[serde(default)]
    pub scope_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub content_type_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub recent_memories: Vec<ModuleMemorySummaryEntry>,
}

/// Namespaced memory service that retains hybrid embeddings while enforcing scope visibility.
#[derive(Clone)]
pub struct ModuleMemoryService {
    memory: HybridMemory,
}

impl ModuleMemoryService {
    /// Creates a module-memory service over the shared durable hybrid memory index.
    #[must_use]
    pub fn new(memory: HybridMemory) -> Self {
        Self { memory }
    }

    /// Stores an embedded memory inside a module namespace with a durable visibility scope.
    pub async fn store(
        &self,
        module_id: impl AsRef<str>,
        input: NewModuleMemory,
        now: DateTime<Utc>,
    ) -> Result<MemoryRecord> {
        let module_id = normalized_module_id(module_id.as_ref())?;
        let mut metadata = metadata_object(input.metadata)?;
        metadata.insert(
            "memory_scope".to_owned(),
            serde_json::to_value(input.scope)?,
        );
        metadata.insert("module_memory".to_owned(), json!(true));
        self.memory
            .store_memory(
                NewMemory {
                    module_id,
                    content: input.content,
                    content_type: input.content_type,
                    metadata: Value::Object(metadata),
                },
                now,
            )
            .await
    }

    /// Searches memories visible to one module, including explicitly shared global records.
    pub async fn search(
        &self,
        module_id: impl AsRef<str>,
        query: &str,
        access_scope: ModuleMemoryScope,
        limit: usize,
    ) -> Result<Vec<ModuleMemoryHit>> {
        self.search_by_content_type(module_id, query, access_scope, None, limit)
            .await
    }

    /// Searches visible module memory while requiring an exact content-type match when supplied.
    pub async fn search_by_content_type(
        &self,
        module_id: impl AsRef<str>,
        query: &str,
        access_scope: ModuleMemoryScope,
        content_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ModuleMemoryHit>> {
        if limit == 0 || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let module_id = normalized_module_id(module_id.as_ref())?;
        let content_type = content_type
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        let candidate_limit = if content_type.is_some() {
            128
        } else {
            limit.saturating_mul(16).min(128).max(limit)
        };
        let candidates = self.memory.search(query, candidate_limit).await?;
        let mut hits = candidates
            .into_iter()
            .filter_map(|hit| {
                let scope = record_scope(&hit.record);
                let visible = visible_to(&hit.record, &module_id, access_scope, scope);
                let type_matches = content_type.as_ref().is_none_or(|expected| {
                    hit.record
                        .content_type
                        .trim()
                        .eq_ignore_ascii_case(expected)
                });
                (visible && type_matches).then_some(ModuleMemoryHit {
                    record: hit.record,
                    scope,
                    lexical_score: hit.lexical_score,
                    vector_similarity: hit.vector_similarity,
                    score: hit.score,
                })
            })
            .collect::<Vec<_>>();
        hits.truncate(limit);
        Ok(hits)
    }

    /// Loads one memory if it is visible to the supplied module and requested access scope.
    pub fn get(
        &self,
        module_id: impl AsRef<str>,
        memory_id: MemoryId,
        access_scope: ModuleMemoryScope,
    ) -> Result<Option<MemoryRecord>> {
        let module_id = normalized_module_id(module_id.as_ref())?;
        let Some(record) = self.memory.store_handle().get(memory_id)? else {
            return Ok(None);
        };
        let scope = record_scope(&record);
        Ok(visible_to(&record, &module_id, access_scope, scope).then_some(record))
    }

    /// Summarizes the module's own stored records without leaking other module-private memory.
    pub fn summary(&self, module_id: impl AsRef<str>) -> Result<ModuleMemorySummary> {
        let module_id = normalized_module_id(module_id.as_ref())?;
        let mut records = self
            .memory
            .store_handle()
            .list()?
            .into_iter()
            .filter(|record| record.module_id == module_id)
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut scope_counts = BTreeMap::new();
        let mut content_type_counts = BTreeMap::new();
        for record in &records {
            *scope_counts
                .entry(scope_label(record_scope(record)).to_owned())
                .or_insert(0) += 1;
            *content_type_counts
                .entry(record.content_type.clone())
                .or_insert(0) += 1;
        }
        let recent_memories = records
            .iter()
            .take(8)
            .map(|record| ModuleMemorySummaryEntry {
                id: record.id,
                content_type: record.content_type.clone(),
                scope: record_scope(record),
                excerpt: truncate_excerpt(&record.content, 220),
                updated_at: record.updated_at,
            })
            .collect();
        Ok(ModuleMemorySummary {
            module_id,
            record_count: records.len(),
            scope_counts,
            content_type_counts,
            recent_memories,
        })
    }

    /// Returns a clone of the hybrid memory used for embedded module storage.
    #[must_use]
    pub fn hybrid_memory(&self) -> HybridMemory {
        self.memory.clone()
    }
}

fn normalized_module_id(value: &str) -> Result<String> {
    let module_id = value.trim();
    if module_id.is_empty() {
        return Err(EduMindError::InvalidStoredData(
            "module memory requires a non-empty module_id".to_owned(),
        ));
    }
    Ok(module_id.to_owned())
}

fn metadata_object(metadata: Value) -> Result<Map<String, Value>> {
    match metadata {
        Value::Null => Ok(Map::new()),
        Value::Object(object) => Ok(object),
        _ => Err(EduMindError::InvalidStoredData(
            "module memory metadata must be a JSON object".to_owned(),
        )),
    }
}

fn record_scope(record: &MemoryRecord) -> ModuleMemoryScope {
    record
        .metadata
        .get("memory_scope")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or(ModuleMemoryScope::Module)
}

fn visible_to(
    record: &MemoryRecord,
    requester_module_id: &str,
    access_scope: ModuleMemoryScope,
    record_scope: ModuleMemoryScope,
) -> bool {
    if record.module_id == requester_module_id || record_scope == ModuleMemoryScope::Global {
        return true;
    }
    record_scope == ModuleMemoryScope::CrossModule
        && matches!(
            access_scope,
            ModuleMemoryScope::CrossModule | ModuleMemoryScope::Global
        )
}

fn scope_label(scope: ModuleMemoryScope) -> &'static str {
    match scope {
        ModuleMemoryScope::Private => "private",
        ModuleMemoryScope::Module => "module",
        ModuleMemoryScope::CrossModule => "cross_module",
        ModuleMemoryScope::Global => "global",
    }
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
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};

    use super::{ModuleMemoryScope, ModuleMemoryService, NewModuleMemory};
    use crate::memory::{HashEmbedder, HybridMemory, MemoryStore};

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap()
    }

    fn service() -> ModuleMemoryService {
        ModuleMemoryService::new(
            HybridMemory::new(
                MemoryStore::in_memory().unwrap(),
                Arc::new(HashEmbedder::new(64).unwrap()),
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn keeps_private_records_inside_their_module_and_shares_global_records() {
        let service = service();
        let mut private = NewModuleMemory::new("Private calculus plan", "note");
        private.scope = ModuleMemoryScope::Private;
        service.store("math", private, timestamp()).await.unwrap();
        let mut global = NewModuleMemory::new("Global calculus reference", "reference");
        global.scope = ModuleMemoryScope::Global;
        service.store("math", global, timestamp()).await.unwrap();

        let math = service
            .search("math", "calculus", ModuleMemoryScope::Module, 10)
            .await
            .unwrap();
        let science = service
            .search("science", "calculus", ModuleMemoryScope::Module, 10)
            .await
            .unwrap();

        assert_eq!(math.len(), 2);
        assert_eq!(science.len(), 1);
        assert_eq!(science[0].scope, ModuleMemoryScope::Global);
    }

    #[tokio::test]
    async fn makes_cross_module_records_visible_only_when_requested() {
        let service = service();
        let mut shared = NewModuleMemory::new("Shared study method", "note");
        shared.scope = ModuleMemoryScope::CrossModule;
        service
            .store("research", shared, timestamp())
            .await
            .unwrap();

        let default_scope = service
            .search("notes", "study", ModuleMemoryScope::Module, 10)
            .await
            .unwrap();
        let cross_module_scope = service
            .search("notes", "study", ModuleMemoryScope::CrossModule, 10)
            .await
            .unwrap();

        assert!(default_scope.is_empty());
        assert_eq!(cross_module_scope.len(), 1);
    }

    #[tokio::test]
    async fn summarizes_module_memory_by_scope_and_content_type() {
        let service = service();
        service
            .store(
                "notes",
                NewModuleMemory::new("Biology cells", "note"),
                timestamp(),
            )
            .await
            .unwrap();

        let summary = service.summary("notes").unwrap();

        assert_eq!(summary.record_count, 1);
        assert_eq!(summary.scope_counts["module"], 1);
        assert_eq!(summary.content_type_counts["note"], 1);
    }
}
