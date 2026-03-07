//! Adapter to expose HMS functionality to the TypeScript execution engine.
//!
//! This module bridges the `wonopcode_tools::hms::HmsService` with the
//! `wonopcode_codemode::HmsService` trait.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use wonopcode_codemode::{
    HmsLocationInfo, HmsMemoryEntry, HmsMemoryValue, HmsRenderResult, HmsService as CodemodeHmsService,
    ServiceError, ServiceResult,
};

use super::{HmsService, MemoryEntryData, MemoryValue, StorageClass, Visibility};

/// Adapter that implements the codemode HmsService trait using the tools HmsService.
pub struct HmsServiceAdapter {
    service: Arc<RwLock<HmsService>>,
    project_root: PathBuf,
}

impl HmsServiceAdapter {
    /// Create a new adapter wrapping the given HMS service.
    pub fn new(service: Arc<RwLock<HmsService>>, project_root: PathBuf) -> Self {
        Self {
            service,
            project_root,
        }
    }

    /// Create a new adapter with a fresh HMS service for the given project root.
    pub fn for_project(project_root: PathBuf) -> Self {
        let service = Arc::new(RwLock::new(HmsService::new(project_root.clone())));
        Self {
            service,
            project_root,
        }
    }

    /// Resolve a path relative to the project root.
    fn resolve_path(&self, path: &str) -> PathBuf {
        if path == "." || path.is_empty() {
            self.project_root.clone()
        } else {
            let p = PathBuf::from(path);
            if p.is_absolute() {
                p
            } else {
                self.project_root.join(p)
            }
        }
    }

    /// Convert internal MemoryValue to codemode HmsMemoryValue.
    fn convert_value(value: &MemoryValue) -> HmsMemoryValue {
        match value {
            MemoryValue::String(s) => HmsMemoryValue::String(s.clone()),
            MemoryValue::List(items) => HmsMemoryValue::List(items.clone()),
            MemoryValue::Map(map) => {
                // Convert map to YAML string representation
                let yaml = serde_yaml::to_string(map).unwrap_or_default();
                HmsMemoryValue::String(yaml)
            }
        }
    }

    /// Convert codemode HmsMemoryValue to internal MemoryValue.
    fn convert_value_back(value: &HmsMemoryValue) -> MemoryValue {
        match value {
            HmsMemoryValue::String(s) => MemoryValue::String(s.clone()),
            HmsMemoryValue::List(items) => MemoryValue::List(items.clone()),
        }
    }

    /// Convert visibility string to Visibility enum.
    fn parse_visibility(vis: Option<&str>) -> Visibility {
        match vis {
            Some("public") | Some("global") => Visibility::Public,
            Some("upstream") | Some("parents") => Visibility::Upstream,
            Some("downstream") | Some("children") => Visibility::Downstream,
            Some("private") | Some("self") => Visibility::Private,
            _ => Visibility::default(),
        }
    }

    /// Convert Visibility enum to string.
    fn visibility_to_string(vis: &Visibility) -> String {
        match vis {
            Visibility::Public => "public".to_string(),
            Visibility::Upstream => "upstream".to_string(),
            Visibility::Downstream => "downstream".to_string(),
            Visibility::Private => "private".to_string(),
        }
    }
}

#[async_trait]
impl CodemodeHmsService for HmsServiceAdapter {
    async fn set(&self, path: &str, entry: HmsMemoryEntry) -> ServiceResult<()> {
        let target_dir = self.resolve_path(path);
        let value = Self::convert_value_back(&entry.value);
        let visibility = Self::parse_visibility(entry.visibility.as_deref());

        let mut data = MemoryEntryData::new(value).with_visibility(visibility);

        if let Some(desc) = entry.description {
            data = data.with_description(desc);
        }

        if !entry.tags.is_empty() {
            data = data.with_tags(entry.tags);
        }

        let service = self.service.read().await;
        service
            .set_memory(&target_dir, &entry.key, data, StorageClass::Tracked)
            .await
            .map_err(|e| ServiceError::new("HMS_SET_ERROR", e.to_string()))
    }

    async fn get(&self, path: &str, key: &str) -> ServiceResult<Option<HmsMemoryEntry>> {
        let target_dir = self.resolve_path(path);
        let service = self.service.read().await;

        let memories = service
            .resolver()
            .resolve(&target_dir)
            .await
            .map_err(|e| ServiceError::new("HMS_RESOLVE_ERROR", e.to_string()))?;

        let entry = memories.into_iter().find(|m| m.key == key).map(|m| {
            HmsMemoryEntry {
                key: m.key,
                value: Self::convert_value(&m.value),
                visibility: Some(Self::visibility_to_string(&m.visibility)),
                description: None, // Not stored in resolved memory
                tags: Vec::new(),  // Not stored in resolved memory
            }
        });

        Ok(entry)
    }

    async fn delete(&self, path: &str, key: &str) -> ServiceResult<bool> {
        let target_dir = self.resolve_path(path);
        let service = self.service.read().await;

        service
            .delete_memory(&target_dir, key, None)
            .await
            .map_err(|e| ServiceError::new("HMS_DELETE_ERROR", e.to_string()))
    }

    async fn list(&self, path: &str) -> ServiceResult<Vec<HmsMemoryEntry>> {
        let target_dir = self.resolve_path(path);
        let service = self.service.read().await;

        let memories = service
            .resolver()
            .resolve(&target_dir)
            .await
            .map_err(|e| ServiceError::new("HMS_RESOLVE_ERROR", e.to_string()))?;

        // Filter to only show memories defined at this exact path
        let entries = memories
            .into_iter()
            .filter(|m| {
                // Check if the memory source is in the target directory
                m.source_path
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p == target_dir)
                    .unwrap_or(false)
            })
            .map(|m| HmsMemoryEntry {
                key: m.key,
                value: Self::convert_value(&m.value),
                visibility: Some(Self::visibility_to_string(&m.visibility)),
                description: None,
                tags: Vec::new(),
            })
            .collect();

        Ok(entries)
    }

    async fn list_locations(&self) -> ServiceResult<Vec<HmsLocationInfo>> {
        // Scan the project for memory file locations
        let mut locations = Vec::new();
        let walker = walkdir::WalkDir::new(&self.project_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // Skip hidden directories except .wonopcode
                !name.starts_with('.') || name == ".wonopcode" || name == ".wonopcode-local"
            });

        for entry in walker.flatten() {
            if entry.file_type().is_dir() {
                let dir_path = entry.path();
                let wonopcode_dir = dir_path.join(".wonopcode");
                let wonopcode_local_dir = dir_path.join(".wonopcode-local");

                let has_template = wonopcode_dir.join("AGENTS.TEMPLATE.md").exists();
                let has_memories = wonopcode_dir.join("memory.yaml").exists()
                    || wonopcode_local_dir.join("memory.yaml").exists();
                let has_generated = dir_path.join("AGENTS.md").exists()
                    || dir_path.join("CLAUDE.md").exists();

                if has_template || has_memories || has_generated {
                    let relative_path = dir_path
                        .strip_prefix(&self.project_root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| ".".to_string());

                    locations.push(HmsLocationInfo {
                        path: if relative_path.is_empty() {
                            ".".to_string()
                        } else {
                            relative_path
                        },
                        has_template,
                        has_memories,
                        has_generated,
                    });
                }
            }
        }

        Ok(locations)
    }

    async fn render(&self, path: &str) -> ServiceResult<HmsRenderResult> {
        let target_dir = self.resolve_path(path);
        let mut service = self.service.write().await;

        let content = service
            .generate(&target_dir)
            .await
            .map_err(|e| ServiceError::new("HMS_RENDER_ERROR", e.to_string()))?;

        // Write the file
        let output_path = service
            .write_agents_md(&target_dir)
            .await
            .map_err(|e| ServiceError::new("HMS_WRITE_ERROR", e.to_string()))?;

        // Count memories and source directories
        let memories = service
            .resolver()
            .resolve(&target_dir)
            .await
            .map_err(|e| ServiceError::new("HMS_RESOLVE_ERROR", e.to_string()))?;

        let source_dirs: std::collections::HashSet<_> = memories
            .iter()
            .filter_map(|m| m.source_path.parent().and_then(|p| p.parent()))
            .collect();

        Ok(HmsRenderResult {
            rendered: content,
            memories_used: memories.len(),
            source_directories: source_dirs.len(),
        })
    }

    async fn effective_memories(&self, path: &str) -> ServiceResult<Vec<HmsMemoryEntry>> {
        let target_dir = self.resolve_path(path);
        let service = self.service.read().await;

        let memories = service
            .resolver()
            .resolve(&target_dir)
            .await
            .map_err(|e| ServiceError::new("HMS_RESOLVE_ERROR", e.to_string()))?;

        let entries = memories
            .into_iter()
            .map(|m| HmsMemoryEntry {
                key: m.key,
                value: Self::convert_value(&m.value),
                visibility: Some(Self::visibility_to_string(&m.visibility)),
                description: None,
                tags: Vec::new(),
            })
            .collect();

        Ok(entries)
    }
}
