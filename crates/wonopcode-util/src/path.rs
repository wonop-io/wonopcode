//! Path utilities.
//!
//! This module provides utilities for working with file paths.

use std::path::{Path, PathBuf};

/// Get the wonopcode configuration directory.
///
/// This follows XDG conventions on Linux/macOS:
/// - `$XDG_CONFIG_HOME/wonopcode` if set
/// - `~/.config/wonopcode` otherwise
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("wonopcode"))
}

/// Get the wonopcode data directory.
///
/// This follows XDG conventions:
/// - `$XDG_DATA_HOME/wonopcode` if set
/// - `~/.local/share/wonopcode` otherwise
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("wonopcode"))
}

/// Get the wonopcode state directory.
///
/// This is where runtime state like prompt history is stored.
pub fn state_dir() -> Option<PathBuf> {
    config_dir().map(|p| p.join("state"))
}

/// Get the wonopcode auth directory.
///
/// This is where provider credentials are stored.
pub fn auth_dir() -> Option<PathBuf> {
    config_dir().map(|p| p.join("auth"))
}

/// Get the wonopcode themes directory.
///
/// This is where custom themes are stored.
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|p| p.join("themes"))
}

/// Get the wonopcode logs directory.
pub fn logs_dir() -> Option<PathBuf> {
    config_dir().map(|p| p.join("logs"))
}

/// Check if a path is within a base directory.
///
/// This is used for security checks to prevent path traversal.
pub fn is_within(path: &Path, base: &Path) -> bool {
    // Canonicalize both paths if possible
    let canonical_path = path.canonicalize().ok();
    let canonical_base = base.canonicalize().ok();

    match (canonical_path, canonical_base) {
        (Some(p), Some(b)) => p.starts_with(&b),
        _ => {
            // If we can't canonicalize, do a simple prefix check
            // This is less reliable but better than nothing
            path.starts_with(base)
        }
    }
}

/// Normalize a path by removing `.` and `..` components.
///
/// Unlike `canonicalize`, this doesn't require the path to exist.
pub fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {
                // Skip `.`
            }
            _ => {
                result.push(component);
            }
        }
    }

    result
}

/// Make a path relative to a base directory.
///
/// Returns `None` if the path is not within the base directory.
pub fn relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    path.strip_prefix(base).ok().map(|p| p.to_path_buf())
}

/// Join a path safely, preventing path traversal.
///
/// Returns `None` if the resulting path would be outside the base.
pub fn safe_join(base: &Path, path: &Path) -> Option<PathBuf> {
    let result = base.join(path);
    let normalized = normalize(&result);

    if is_within(&normalized, base) {
        Some(normalized)
    } else {
        None
    }
}

/// Get the project-local wonopcode directory.
pub fn project_config_dir(project_root: &Path) -> PathBuf {
    project_root.join(".wonopcode")
}

/// Find the project root by walking up the directory tree.
///
/// Looks for markers like `.wonopcode/`, `.git/`, `Cargo.toml`, `package.json`, etc.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let markers = [
        ".wonopcode",
        ".git",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
    ];

    let mut current = start.to_path_buf();

    loop {
        for marker in &markers {
            if current.join(marker).exists() {
                return Some(current);
            }
        }

        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_dir() {
        let dir = config_dir();
        assert!(dir.is_some());
        assert!(dir.unwrap().ends_with("wonopcode"));
    }

    #[test]
    fn test_data_dir() {
        let dir = data_dir();
        // Most systems have a data_local_dir
        if let Some(d) = dir {
            assert!(d.to_string_lossy().contains("wonopcode"));
        }
    }

    #[test]
    fn test_state_dir() {
        let dir = state_dir();
        if let Some(d) = dir {
            assert!(d.to_string_lossy().contains("state"));
        }
    }

    #[test]
    fn test_auth_dir() {
        let dir = auth_dir();
        if let Some(d) = dir {
            assert!(d.to_string_lossy().contains("auth"));
        }
    }

    #[test]
    fn test_themes_dir() {
        let dir = themes_dir();
        if let Some(d) = dir {
            assert!(d.to_string_lossy().contains("themes"));
        }
    }

    #[test]
    fn test_logs_dir() {
        let dir = logs_dir();
        if let Some(d) = dir {
            assert!(d.to_string_lossy().contains("logs"));
        }
    }

    #[test]
    fn test_is_within() {
        let base = PathBuf::from("/home/user/project");
        assert!(is_within(Path::new("/home/user/project/src"), &base));
        assert!(!is_within(Path::new("/home/user/other"), &base));
    }

    #[test]
    fn test_is_within_with_real_paths() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();

        // Real paths that exist should canonicalize
        assert!(is_within(&subdir, dir.path()));
    }

    #[test]
    fn test_normalize() {
        let path = Path::new("/home/user/./project/../project/src");
        let normalized = normalize(path);
        assert_eq!(normalized, PathBuf::from("/home/user/project/src"));
    }

    #[test]
    fn test_normalize_only_dots() {
        let path = Path::new("./././file.txt");
        let normalized = normalize(path);
        assert_eq!(normalized, PathBuf::from("file.txt"));
    }

    #[test]
    fn test_normalize_parent_dirs() {
        let path = Path::new("a/b/c/../../d");
        let normalized = normalize(path);
        assert_eq!(normalized, PathBuf::from("a/d"));
    }

    #[test]
    fn test_relative_to() {
        let base = Path::new("/home/user/project");
        let path = Path::new("/home/user/project/src/main.rs");
        let relative = relative_to(path, base);
        assert_eq!(relative, Some(PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_relative_to_not_within() {
        let base = Path::new("/home/user/project");
        let path = Path::new("/home/other/file.txt");
        let relative = relative_to(path, base);
        assert_eq!(relative, None);
    }

    #[test]
    fn test_safe_join() {
        let base = PathBuf::from("/home/user/project");

        // Safe join
        let result = safe_join(&base, Path::new("src/main.rs"));
        assert!(result.is_some());

        // Unsafe join (path traversal attempt)
        let result = safe_join(&base, Path::new("../../../etc/passwd"));
        assert!(result.is_none());
    }

    #[test]
    fn test_project_config_dir() {
        let project_root = Path::new("/home/user/myproject");
        let config = project_config_dir(project_root);
        assert_eq!(config, PathBuf::from("/home/user/myproject/.wonopcode"));
    }

    #[test]
    fn test_find_project_root() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("myproject");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir(project.join(".git")).unwrap();

        let src = project.join("src");
        std::fs::create_dir(&src).unwrap();

        let root = find_project_root(&src);
        assert_eq!(root, Some(project));
    }

    #[test]
    fn test_find_project_root_with_wonopcode_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("myproject");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir(project.join(".wonopcode")).unwrap();

        let root = find_project_root(&project);
        assert_eq!(root, Some(project));
    }

    #[test]
    fn test_find_project_root_with_cargo_toml() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("myproject");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("Cargo.toml"), "[package]").unwrap();

        let root = find_project_root(&project);
        assert_eq!(root, Some(project));
    }

    #[test]
    fn test_find_project_root_none() {
        // Start from a path that has no markers going up
        // This is tricky to test reliably, so just verify function doesn't panic
        let result = find_project_root(Path::new("/nonexistent/path"));
        // Result might be Some if there's a marker up the tree, or None
        let _ = result;
    }
}
