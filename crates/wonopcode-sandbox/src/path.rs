//! Path mapping between host and sandbox filesystems.

use std::path::{Path, PathBuf};

/// Maps paths between host and sandbox filesystem.
///
/// This handles the translation of paths when executing commands in a sandboxed
/// environment where the project directory is mounted at a different location.
///
/// # Example
///
/// ```
/// use wonopcode_sandbox::PathMapper;
/// use std::path::PathBuf;
///
/// let mapper = PathMapper::new(
///     PathBuf::from("/Users/dev/myproject"),
///     PathBuf::from("/workspace"),
/// );
///
/// // Host path -> Sandbox path
/// assert_eq!(
///     mapper.to_sandbox("/Users/dev/myproject/src/main.rs"),
///     Some(PathBuf::from("/workspace/src/main.rs"))
/// );
///
/// // Sandbox path -> Host path
/// assert_eq!(
///     mapper.to_host("/workspace/src/main.rs"),
///     Some(PathBuf::from("/Users/dev/myproject/src/main.rs"))
/// );
/// ```
#[derive(Debug, Clone)]
pub struct PathMapper {
    /// Host project root (e.g., /Users/dev/myproject)
    host_root: PathBuf,
    /// Sandbox workspace path (e.g., /workspace)
    sandbox_root: PathBuf,
}

impl PathMapper {
    /// Create a new path mapper.
    ///
    /// # Arguments
    ///
    /// * `host_root` - The project root directory on the host filesystem
    /// * `sandbox_root` - The workspace mount point in the sandbox
    pub fn new(host_root: PathBuf, sandbox_root: PathBuf) -> Self {
        Self {
            host_root,
            sandbox_root,
        }
    }

    /// Get the host root directory.
    pub fn host_root(&self) -> &Path {
        &self.host_root
    }

    /// Get the sandbox root directory.
    pub fn sandbox_root(&self) -> &Path {
        &self.sandbox_root
    }

    /// Convert a host path to a sandbox path.
    ///
    /// Returns `None` if the path is not under the host root.
    ///
    /// # Example
    ///
    /// `/Users/dev/myproject/src/main.rs` -> `/workspace/src/main.rs`
    pub fn to_sandbox(&self, host_path: impl AsRef<Path>) -> Option<PathBuf> {
        let host_path = host_path.as_ref();

        // Try to strip the host root prefix
        host_path
            .strip_prefix(&self.host_root)
            .ok()
            .map(|relative| self.sandbox_root.join(relative))
    }

    /// Convert a sandbox path to a host path.
    ///
    /// Returns `None` if the path is not under the sandbox root.
    ///
    /// # Example
    ///
    /// `/workspace/src/main.rs` -> `/Users/dev/myproject/src/main.rs`
    pub fn to_host(&self, sandbox_path: impl AsRef<Path>) -> Option<PathBuf> {
        let sandbox_path = sandbox_path.as_ref();

        // Try to strip the sandbox root prefix
        sandbox_path
            .strip_prefix(&self.sandbox_root)
            .ok()
            .map(|relative| self.host_root.join(relative))
    }

    /// Check if a host path is within the mapped workspace.
    pub fn is_host_path_mapped(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().starts_with(&self.host_root)
    }

    /// Check if a sandbox path is within the mapped workspace.
    pub fn is_sandbox_path_mapped(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().starts_with(&self.sandbox_root)
    }

    /// Convert a path, auto-detecting direction.
    ///
    /// If the path starts with the host root, converts to sandbox.
    /// If the path starts with the sandbox root, converts to host.
    /// Otherwise returns None.
    pub fn convert(&self, path: impl AsRef<Path>) -> Option<PathBuf> {
        let path = path.as_ref();

        if path.starts_with(&self.host_root) {
            self.to_sandbox(path)
        } else if path.starts_with(&self.sandbox_root) {
            self.to_host(path)
        } else {
            None
        }
    }

    /// Make a path relative to the sandbox workspace.
    ///
    /// If the path is absolute and under host root, converts it.
    /// If the path is already relative, returns it unchanged.
    /// If the path is absolute but not under host root, returns None.
    pub fn make_sandbox_relative(&self, path: impl AsRef<Path>) -> Option<PathBuf> {
        let path = path.as_ref();

        if path.is_relative() {
            // Already relative, assume it's relative to workspace
            Some(path.to_path_buf())
        } else if path.starts_with(&self.host_root) {
            // Absolute host path, convert to sandbox
            self.to_sandbox(path)
        } else if path.starts_with(&self.sandbox_root) {
            // Already a sandbox path
            Some(path.to_path_buf())
        } else {
            // Path outside workspace
            None
        }
    }
}

impl Default for PathMapper {
    fn default() -> Self {
        Self {
            host_root: PathBuf::from("."),
            sandbox_root: PathBuf::from("/workspace"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_mapper() -> PathMapper {
        PathMapper::new(
            PathBuf::from("/Users/dev/myproject"),
            PathBuf::from("/workspace"),
        )
    }

    #[test]
    fn test_to_sandbox() {
        let mapper = test_mapper();

        assert_eq!(
            mapper.to_sandbox("/Users/dev/myproject/src/main.rs"),
            Some(PathBuf::from("/workspace/src/main.rs"))
        );

        assert_eq!(
            mapper.to_sandbox("/Users/dev/myproject"),
            Some(PathBuf::from("/workspace"))
        );

        assert_eq!(mapper.to_sandbox("/Users/other/project/file.rs"), None);
    }

    #[test]
    fn test_to_host() {
        let mapper = test_mapper();

        assert_eq!(
            mapper.to_host("/workspace/src/main.rs"),
            Some(PathBuf::from("/Users/dev/myproject/src/main.rs"))
        );

        assert_eq!(
            mapper.to_host("/workspace"),
            Some(PathBuf::from("/Users/dev/myproject"))
        );

        assert_eq!(mapper.to_host("/other/path/file.rs"), None);
    }

    #[test]
    fn test_is_mapped() {
        let mapper = test_mapper();

        assert!(mapper.is_host_path_mapped("/Users/dev/myproject/src"));
        assert!(!mapper.is_host_path_mapped("/Users/other/project"));

        assert!(mapper.is_sandbox_path_mapped("/workspace/src"));
        assert!(!mapper.is_sandbox_path_mapped("/other/path"));
    }

    #[test]
    fn test_convert_auto() {
        let mapper = test_mapper();

        // Host -> Sandbox
        assert_eq!(
            mapper.convert("/Users/dev/myproject/file.rs"),
            Some(PathBuf::from("/workspace/file.rs"))
        );

        // Sandbox -> Host
        assert_eq!(
            mapper.convert("/workspace/file.rs"),
            Some(PathBuf::from("/Users/dev/myproject/file.rs"))
        );

        // Neither
        assert_eq!(mapper.convert("/other/path"), None);
    }

    #[test]
    #[cfg(unix)]
    fn test_make_sandbox_relative() {
        let mapper = test_mapper();

        // Relative path stays relative
        assert_eq!(
            mapper.make_sandbox_relative("src/main.rs"),
            Some(PathBuf::from("src/main.rs"))
        );

        // Host absolute path converts
        assert_eq!(
            mapper.make_sandbox_relative("/Users/dev/myproject/src/main.rs"),
            Some(PathBuf::from("/workspace/src/main.rs"))
        );

        // Sandbox path stays as-is
        assert_eq!(
            mapper.make_sandbox_relative("/workspace/src/main.rs"),
            Some(PathBuf::from("/workspace/src/main.rs"))
        );

        // Outside path returns None
        assert_eq!(mapper.make_sandbox_relative("/other/absolute/path"), None);
    }
}
