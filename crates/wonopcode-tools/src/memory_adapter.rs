//! Adapter bridging wonopcode-tools memory service to wonopcode-codemode MemoryService trait.
//!
//! This allows the TypeScript executor to use the shared memory service
//! from the tools crate while speaking the codemode service trait interface.

use async_trait::async_trait;
use std::sync::Arc;
use wonop_memory::MemoryService;
use wonopcode_codemode::{
    MemoryEntry as CodemodeMemoryEntry, MemoryScope as CodemodeMemoryScope,
    MemorySearchResult as CodemodeMemorySearchResult, MemoryService as CodemodeMemoryService,
    ServiceError, ServiceResult, StoreMemoryInput,
};

/// Adapter that wraps a wonop_memory::MemoryService to implement the codemode MemoryService trait.
pub struct MemoryServiceAdapter {
    service: Arc<MemoryService>,
}

impl MemoryServiceAdapter {
    /// Create a new adapter wrapping the given memory service.
    pub fn new(service: Arc<MemoryService>) -> Self {
        Self { service }
    }

    /// Convert codemode scope to string.
    fn scope_to_string(scope: Option<CodemodeMemoryScope>) -> Option<String> {
        scope.map(|s| match s {
            CodemodeMemoryScope::Global => "global".to_string(),
            CodemodeMemoryScope::Workstream => "workstream".to_string(),
            CodemodeMemoryScope::Session => "session".to_string(),
        })
    }

    /// Convert scopes list to strings.
    fn scopes_to_strings(scopes: Option<Vec<CodemodeMemoryScope>>) -> Option<Vec<String>> {
        scopes.map(|ss| {
            ss.into_iter()
                .map(|s| match s {
                    CodemodeMemoryScope::Global => "global".to_string(),
                    CodemodeMemoryScope::Workstream => "workstream".to_string(),
                    CodemodeMemoryScope::Session => "session".to_string(),
                })
                .collect()
        })
    }

    /// Convert wonop_memory MemoryEntry to codemode MemoryEntry.
    fn convert_entry(entry: wonop_memory::MemoryEntry) -> CodemodeMemoryEntry {
        CodemodeMemoryEntry {
            key: entry.key,
            content: entry.content,
            scope: match entry.scope.as_str() {
                "global" => CodemodeMemoryScope::Global,
                "workstream" => CodemodeMemoryScope::Workstream,
                "session" => CodemodeMemoryScope::Session,
                _ => CodemodeMemoryScope::Session, // Default to session for unknown scopes
            },
            tags: entry.tags,
            created_at: entry.created_at.to_rfc3339(),
            updated_at: entry.accessed_at.to_rfc3339(),
        }
    }
}

#[async_trait]
impl CodemodeMemoryService for MemoryServiceAdapter {
    async fn store(&self, input: StoreMemoryInput) -> ServiceResult<CodemodeMemoryEntry> {
        let params = wonop_memory::MemoryStoreParams {
            key: input.key,
            content: input.content,
            scope: Self::scope_to_string(input.scope),
            tags: input.tags,
            index: Some(true),
            metadata: None,
        };

        self.service
            .store(params)
            .await
            .map(Self::convert_entry)
            .map_err(|e| ServiceError::new("MEMORY_ERROR", e.to_string()))
    }

    async fn recall(
        &self,
        key: &str,
        scope: Option<CodemodeMemoryScope>,
    ) -> ServiceResult<Option<CodemodeMemoryEntry>> {
        let params = wonop_memory::MemoryRecallParams {
            key: Some(key.to_string()),
            query: None,
            scope: Self::scope_to_string(scope),
            limit: Some(1),
        };

        self.service
            .recall(params)
            .await
            .map(|entries| entries.into_iter().next().map(Self::convert_entry))
            .map_err(|e| ServiceError::new("MEMORY_ERROR", e.to_string()))
    }

    async fn search(
        &self,
        query: &str,
        scopes: Option<Vec<CodemodeMemoryScope>>,
        limit: Option<usize>,
        min_similarity: Option<f32>,
    ) -> ServiceResult<Vec<CodemodeMemorySearchResult>> {
        let params = wonop_memory::MemorySearchParams {
            query: query.to_string(),
            scopes: Self::scopes_to_strings(scopes),
            threshold: min_similarity.map(|s| s as f64),
            limit,
        };

        self.service
            .search(params)
            .await
            .map(|result| {
                result
                    .entries
                    .into_iter()
                    .map(|entry| {
                        let similarity = entry.relevance_score.unwrap_or(0.0) as f32;
                        CodemodeMemorySearchResult {
                            entry: Self::convert_entry(entry),
                            similarity,
                        }
                    })
                    .collect()
            })
            .map_err(|e| ServiceError::new("MEMORY_ERROR", e.to_string()))
    }

    async fn clear(
        &self,
        scope: Option<CodemodeMemoryScope>,
        pattern: Option<String>,
        older_than: Option<String>,
    ) -> ServiceResult<usize> {
        // Clear requires a scope - default to session if none provided
        let scope_str = Self::scope_to_string(scope).unwrap_or_else(|| "session".to_string());

        let params = wonop_memory::MemoryClearParams {
            scope: scope_str,
            pattern,
            older_than,
        };

        self.service
            .clear(params)
            .await
            .map_err(|e| ServiceError::new("MEMORY_ERROR", e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_adapter_creation() {
        let service = Arc::new(MemoryService::new().unwrap());
        let _adapter = MemoryServiceAdapter::new(service);
    }
}
