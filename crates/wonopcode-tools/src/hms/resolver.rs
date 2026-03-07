//! Memory resolution and propagation engine.
//!
//! The resolver collects memory entries from the directory hierarchy
//! and merges them according to visibility rules.

use dashmap::DashMap;
use indexmap::IndexMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tokio::fs;
use tracing::debug;

use super::{HmsError, MemoryFile, ResolvedMemory, StorageClass, Visibility};

/// Resolves memory entries for a target directory by traversing the hierarchy.
pub struct MemoryResolver {
    project_root: PathBuf,
}

impl MemoryResolver {
    /// Create a new resolver for the given project root.
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Collect all memories visible at the target directory.
    ///
    /// The resolution algorithm:
    /// 1. **Upstream collection** (root → target): Collect entries with `public` or `downstream` visibility
    /// 2. **Target directory**: Include all entries except `upstream`-only
    /// 3. **Downstream collection** (target → leaves): Collect entries with `public` or `upstream` visibility
    ///
    /// Later entries override earlier ones from the same direction, but upstream entries
    /// don't override keys already set from downstream collection.
    pub async fn resolve(&self, target_dir: &Path) -> Result<Vec<ResolvedMemory>, HmsError> {
        let mut memories: IndexMap<String, ResolvedMemory> = IndexMap::new();

        // Phase 1: Upstream collection (root → target)
        let ancestors = self.get_ancestors(target_dir)?;
        for ancestor in &ancestors[..ancestors.len().saturating_sub(1)] {
            // Exclude target itself
            let entries = self.load_memories_at(ancestor).await?;
            for entry in entries {
                // Only include public or downstream-visible entries
                if matches!(entry.visibility, Visibility::Public | Visibility::Downstream) {
                    // Later entries override earlier ones (closer wins)
                    memories.insert(entry.key.clone(), entry);
                }
            }
        }

        // Phase 2: Target directory (include all except upstream-only for visibility TO parents)
        // At the target dir, we see everything except entries that are ONLY visible upstream
        // Actually, at the target we should see all entries defined there
        let target_entries = self.load_memories_at(target_dir).await?;
        for entry in target_entries {
            // Include all entries at target - they're all visible at their defining directory
            memories.insert(entry.key.clone(), entry);
        }

        // Phase 3: Downstream collection (target → leaves) - BFS
        let downstream_entries = self.collect_downstream(target_dir).await?;
        for entry in downstream_entries {
            // Only include public or upstream-visible entries
            if matches!(entry.visibility, Visibility::Public | Visibility::Upstream) {
                // Don't override existing keys from upstream
                memories.entry(entry.key.clone()).or_insert(entry);
            }
        }

        Ok(memories.into_values().collect())
    }

    /// Get ancestors from root to target (inclusive).
    fn get_ancestors(&self, target: &Path) -> Result<Vec<PathBuf>, HmsError> {
        let target = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
        let root = self
            .project_root
            .canonicalize()
            .unwrap_or_else(|_| self.project_root.clone());

        let relative = target.strip_prefix(&root).map_err(|_| {
            HmsError::PathOutsideProject(format!(
                "{} is not under {}",
                target.display(),
                root.display()
            ))
        })?;

        let mut ancestors = vec![root.clone()];
        let mut current = root;

        for component in relative.components() {
            current = current.join(component);
            ancestors.push(current.clone());
        }

        Ok(ancestors)
    }

    /// Load memories from all storage classes at a directory.
    async fn load_memories_at(&self, dir: &Path) -> Result<Vec<ResolvedMemory>, HmsError> {
        let mut entries = Vec::new();

        // Load in reverse priority order so higher priority overwrites
        for storage_class in StorageClass::all_in_priority_order().iter().rev() {
            let path = storage_class.memory_path(dir);
            if path.exists() {
                debug!(?path, "Loading memory.yaml");
                let content = fs::read_to_string(&path).await?;
                let file = MemoryFile::parse(&content)?;

                for (key, data) in file.iter() {
                    entries.push(ResolvedMemory::from_entry(
                        key.clone(),
                        data,
                        path.clone(),
                        *storage_class,
                    ));
                }
            }
        }

        Ok(entries)
    }

    /// Collect downstream memories using BFS.
    async fn collect_downstream(&self, start: &Path) -> Result<Vec<ResolvedMemory>, HmsError> {
        let mut queue = VecDeque::new();
        let mut results = Vec::new();

        // Add immediate children to queue
        if let Ok(mut dir_entries) = fs::read_dir(start).await {
            while let Ok(Some(entry)) = dir_entries.next_entry().await {
                if let Ok(ft) = entry.file_type().await {
                    if ft.is_dir() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        // Skip hidden directories
                        if !name_str.starts_with('.') {
                            queue.push_back(entry.path());
                        }
                    }
                }
            }
        }

        // BFS traversal
        while let Some(dir) = queue.pop_front() {
            // Load memories from this directory
            let entries = self.load_memories_at(&dir).await?;
            results.extend(entries);

            // Add children to queue
            if let Ok(mut dir_entries) = fs::read_dir(&dir).await {
                while let Ok(Some(entry)) = dir_entries.next_entry().await {
                    if let Ok(ft) = entry.file_type().await {
                        if ft.is_dir() {
                            let name = entry.file_name();
                            let name_str = name.to_string_lossy();
                            if !name_str.starts_with('.') {
                                queue.push_back(entry.path());
                            }
                        }
                    }
                }
            }
        }

        Ok(results)
    }
}

/// Cached entry for resolved memories.
struct CachedMemories {
    entries: Vec<ResolvedMemory>,
    cached_at: Instant,
    #[allow(dead_code)]
    file_mtimes: std::collections::HashMap<PathBuf, SystemTime>,
}

/// A caching wrapper around MemoryResolver.
pub struct CachedResolver {
    resolver: MemoryResolver,
    cache: DashMap<PathBuf, CachedMemories>,
    ttl: Duration,
}

impl CachedResolver {
    /// Default cache TTL.
    pub const DEFAULT_TTL: Duration = Duration::from_secs(5);

    /// Create a new cached resolver.
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            resolver: MemoryResolver::new(project_root),
            cache: DashMap::new(),
            ttl: Self::DEFAULT_TTL,
        }
    }

    /// Create with custom TTL.
    pub fn with_ttl(project_root: PathBuf, ttl: Duration) -> Self {
        Self {
            resolver: MemoryResolver::new(project_root),
            cache: DashMap::new(),
            ttl,
        }
    }

    /// Get the project root.
    pub fn project_root(&self) -> &Path {
        &self.resolver.project_root
    }

    /// Resolve memories, using cache if valid.
    pub async fn resolve(&self, target: &Path) -> Result<Vec<ResolvedMemory>, HmsError> {
        let target = target
            .canonicalize()
            .unwrap_or_else(|_| target.to_path_buf());

        // Check cache
        if let Some(cached) = self.cache.get(&target) {
            if cached.cached_at.elapsed() < self.ttl {
                debug!(?target, "Using cached memories");
                return Ok(cached.entries.clone());
            }
        }

        // Cache miss - resolve and cache
        debug!(?target, "Resolving memories (cache miss)");
        let entries = self.resolver.resolve(&target).await?;

        self.cache.insert(
            target.clone(),
            CachedMemories {
                entries: entries.clone(),
                cached_at: Instant::now(),
                file_mtimes: std::collections::HashMap::new(), // TODO: track mtimes
            },
        );

        Ok(entries)
    }

    /// Invalidate cache for a directory and its ancestors.
    pub fn invalidate(&self, dir: &Path) {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        self.cache.remove(&dir);

        // Also invalidate ancestors
        let mut current = dir;
        while let Some(parent) = current.parent() {
            self.cache.remove(parent);
            current = parent.to_path_buf();
        }
    }

    /// Clear all cached entries.
    pub fn clear(&self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_memory_file(dir: &Path, content: &str) -> PathBuf {
        let memory_dir = dir.join(".wonopcode");
        fs::create_dir_all(&memory_dir).await.unwrap();
        let path = memory_dir.join("memory.yaml");
        fs::write(&path, content).await.unwrap();
        path
    }

    #[tokio::test]
    async fn test_resolver_simple() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        create_memory_file(
            root,
            r#"
root_key:
  value: "root value"
  visibility: public
"#,
        )
        .await;

        let resolver = MemoryResolver::new(root.to_path_buf());
        let memories = resolver.resolve(root).await.unwrap();

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].key, "root_key");
    }

    #[tokio::test]
    async fn test_resolver_upstream_propagation() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("src");
        fs::create_dir_all(&subdir).await.unwrap();

        // Root has a public key
        create_memory_file(
            root,
            r#"
root_key:
  value: "from root"
  visibility: public
"#,
        )
        .await;

        let resolver = MemoryResolver::new(root.to_path_buf());
        let memories = resolver.resolve(&subdir).await.unwrap();

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].key, "root_key");
        assert_eq!(
            memories[0].value.to_string_lossy(),
            "from root"
        );
    }

    #[tokio::test]
    async fn test_resolver_private_not_propagated() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("src");
        fs::create_dir_all(&subdir).await.unwrap();

        create_memory_file(
            root,
            r#"
private_key:
  value: "secret"
  visibility: private
"#,
        )
        .await;

        let resolver = MemoryResolver::new(root.to_path_buf());
        let memories = resolver.resolve(&subdir).await.unwrap();

        assert!(memories.is_empty());
    }

    #[tokio::test]
    async fn test_resolver_downstream_visibility() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("src");
        fs::create_dir_all(&subdir).await.unwrap();

        create_memory_file(
            root,
            r#"
downstream_key:
  value: "for children"
  visibility: downstream
"#,
        )
        .await;

        let resolver = MemoryResolver::new(root.to_path_buf());

        // Should be visible in child
        let memories = resolver.resolve(&subdir).await.unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].key, "downstream_key");

        // Should NOT be visible at root (downstream means only children see it)
        // Actually, at the defining directory, all entries are visible
        let root_memories = resolver.resolve(root).await.unwrap();
        assert_eq!(root_memories.len(), 1); // visible at defining dir
    }

    #[tokio::test]
    async fn test_resolver_upstream_visibility() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("src");
        fs::create_dir_all(&subdir).await.unwrap();

        create_memory_file(
            &subdir,
            r#"
upstream_key:
  value: "for parents"
  visibility: upstream
"#,
        )
        .await;

        let resolver = MemoryResolver::new(root.to_path_buf());

        // Should be visible at root (parent)
        let root_memories = resolver.resolve(root).await.unwrap();
        assert_eq!(root_memories.len(), 1);
        assert_eq!(root_memories[0].key, "upstream_key");

        // Should be visible at defining directory
        let subdir_memories = resolver.resolve(&subdir).await.unwrap();
        assert_eq!(subdir_memories.len(), 1);
    }

    #[tokio::test]
    async fn test_resolver_closer_overrides() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let subdir = root.join("src");
        fs::create_dir_all(&subdir).await.unwrap();

        create_memory_file(
            root,
            r#"
shared_key:
  value: "from root"
  visibility: public
"#,
        )
        .await;

        create_memory_file(
            &subdir,
            r#"
shared_key:
  value: "from subdir"
  visibility: public
"#,
        )
        .await;

        let resolver = MemoryResolver::new(root.to_path_buf());
        let memories = resolver.resolve(&subdir).await.unwrap();

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].value.to_string_lossy(), "from subdir");
    }

    #[tokio::test]
    async fn test_cached_resolver() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        create_memory_file(
            root,
            r#"
key1:
  value: "value1"
"#,
        )
        .await;

        let resolver = CachedResolver::new(root.to_path_buf());

        // First call populates cache
        let memories1 = resolver.resolve(root).await.unwrap();
        assert_eq!(memories1.len(), 1);

        // Second call uses cache (would be same result)
        let memories2 = resolver.resolve(root).await.unwrap();
        assert_eq!(memories2.len(), 1);
    }
}
