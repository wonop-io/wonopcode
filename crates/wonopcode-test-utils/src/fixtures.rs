//! Test fixtures for creating reproducible test environments.
//!
//! Provides utilities for setting up temporary project directories,
//! configuration files, and test data.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A temporary test project with configurable file structure.
///
/// Creates a temporary directory that is automatically cleaned up
/// when the `TestProject` is dropped.
///
/// # Example
///
/// ```rust
/// use wonopcode_test_utils::fixtures::TestProject;
///
/// let project = TestProject::new()
///     .with_file("src/main.rs", "fn main() { println!(\"Hello\"); }")
///     .with_file("Cargo.toml", "[package]\nname = \"test\"")
///     .with_dir("src/modules")
///     .build();
///
/// assert!(project.path().join("src/main.rs").exists());
/// ```
pub struct TestProject {
    /// The temporary directory backing this project.
    temp_dir: TempDir,
    /// Files to create (path relative to root -> contents).
    files: HashMap<PathBuf, String>,
    /// Directories to create (paths relative to root).
    dirs: Vec<PathBuf>,
}

impl TestProject {
    /// Create a new test project builder.
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
            files: HashMap::new(),
            dirs: Vec::new(),
        }
    }

    /// Add a file to the project.
    ///
    /// The path should be relative to the project root.
    /// Parent directories are created automatically.
    pub fn with_file(mut self, path: impl AsRef<Path>, contents: impl Into<String>) -> Self {
        self.files
            .insert(path.as_ref().to_path_buf(), contents.into());
        self
    }

    /// Add an empty directory to the project.
    pub fn with_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.dirs.push(path.as_ref().to_path_buf());
        self
    }

    /// Add a standard Rust project structure.
    pub fn with_rust_project(self, name: &str) -> Self {
        let cargo_toml = format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#
        );

        let main_rs = r#"fn main() {
    println!("Hello, world!");
}
"#;

        self.with_file("Cargo.toml", cargo_toml)
            .with_file("src/main.rs", main_rs)
    }

    /// Add a wonopcode configuration file.
    pub fn with_config(self, config: &str) -> Self {
        self.with_file("wonopcode.json", config)
    }

    /// Add a .gitignore file.
    pub fn with_gitignore(self, contents: &str) -> Self {
        self.with_file(".gitignore", contents)
    }

    /// Build the project, creating all files and directories.
    pub fn build(self) -> BuiltTestProject {
        let root = self.temp_dir.path();

        // Create directories first
        for dir in &self.dirs {
            let full_path = root.join(dir);
            fs::create_dir_all(&full_path).unwrap_or_else(|e| {
                panic!("Failed to create directory {}: {}", full_path.display(), e)
            });
        }

        // Create files (parent directories are created automatically)
        for (path, contents) in &self.files {
            let full_path = root.join(path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).unwrap_or_else(|e| {
                    panic!(
                        "Failed to create parent directory for {}: {}",
                        full_path.display(),
                        e
                    )
                });
            }
            fs::write(&full_path, contents)
                .unwrap_or_else(|e| panic!("Failed to write file {}: {}", full_path.display(), e));
        }

        BuiltTestProject {
            temp_dir: self.temp_dir,
        }
    }
}

impl Default for TestProject {
    fn default() -> Self {
        Self::new()
    }
}

/// A built test project with files created on disk.
///
/// The temporary directory is automatically cleaned up when this is dropped.
pub struct BuiltTestProject {
    temp_dir: TempDir,
}

impl BuiltTestProject {
    /// Get the path to the project root.
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Read a file from the project.
    pub fn read_file(&self, path: impl AsRef<Path>) -> String {
        let full_path = self.path().join(path.as_ref());
        fs::read_to_string(&full_path)
            .unwrap_or_else(|e| panic!("Failed to read file {}: {}", full_path.display(), e))
    }

    /// Check if a file exists in the project.
    pub fn file_exists(&self, path: impl AsRef<Path>) -> bool {
        self.path().join(path.as_ref()).exists()
    }

    /// Write a file to the project (for modifying during tests).
    pub fn write_file(&self, path: impl AsRef<Path>, contents: impl AsRef<str>) {
        let full_path = self.path().join(path.as_ref());
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&full_path, contents.as_ref())
            .unwrap_or_else(|e| panic!("Failed to write file {}: {}", full_path.display(), e));
    }

    /// Delete a file from the project.
    pub fn delete_file(&self, path: impl AsRef<Path>) {
        let full_path = self.path().join(path.as_ref());
        fs::remove_file(&full_path)
            .unwrap_or_else(|e| panic!("Failed to delete file {}: {}", full_path.display(), e));
    }

    /// List files in a directory (relative paths).
    pub fn list_files(&self, dir: impl AsRef<Path>) -> Vec<PathBuf> {
        let full_path = self.path().join(dir.as_ref());
        if !full_path.exists() {
            return Vec::new();
        }

        fs::read_dir(&full_path)
            .unwrap_or_else(|e| panic!("Failed to read directory {}: {}", full_path.display(), e))
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    if e.file_type().ok()?.is_file() {
                        Some(e.path().strip_prefix(self.path()).ok()?.to_path_buf())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}

/// Common test file contents.
pub mod content {
    /// A simple Rust main function.
    pub const RUST_MAIN: &str = r#"fn main() {
    println!("Hello, world!");
}
"#;

    /// A Rust function with a bug (for testing fixes).
    pub const RUST_BUGGY: &str = r#"fn divide(a: i32, b: i32) -> i32 {
    a / b  // Bug: no zero check
}

fn main() {
    println!("{}", divide(10, 0));
}
"#;

    /// A simple Cargo.toml.
    pub fn cargo_toml(name: &str) -> String {
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#
        )
    }

    /// A wonopcode configuration.
    pub fn wonopcode_config(theme: &str, model: &str) -> String {
        format!(
            r#"{{
    "theme": "{theme}",
    "model": "{model}"
}}"#
        )
    }

    /// A Python hello world.
    pub const PYTHON_HELLO: &str = r#"def main():
    print("Hello, world!")

if __name__ == "__main__":
    main()
"#;

    /// A JavaScript hello world.
    pub const JS_HELLO: &str = r#"function main() {
    console.log("Hello, world!");
}

main();
"#;

    /// A TypeScript hello world.
    pub const TS_HELLO: &str = r#"function main(): void {
    console.log("Hello, world!");
}

main();
"#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_project() {
        let project = TestProject::new().build();
        assert!(project.path().exists());
    }

    #[test]
    fn test_project_with_files() {
        let project = TestProject::new()
            .with_file("test.txt", "Hello")
            .with_file("src/main.rs", "fn main() {}")
            .build();

        assert!(project.file_exists("test.txt"));
        assert!(project.file_exists("src/main.rs"));
        assert_eq!(project.read_file("test.txt"), "Hello");
    }

    #[test]
    fn test_rust_project() {
        let project = TestProject::new().with_rust_project("my-project").build();

        assert!(project.file_exists("Cargo.toml"));
        assert!(project.file_exists("src/main.rs"));

        let cargo = project.read_file("Cargo.toml");
        assert!(cargo.contains("my-project"));
    }

    #[test]
    fn test_write_and_delete() {
        let project = TestProject::new().build();

        project.write_file("new.txt", "content");
        assert!(project.file_exists("new.txt"));

        project.delete_file("new.txt");
        assert!(!project.file_exists("new.txt"));
    }
}
