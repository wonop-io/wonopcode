//! Configuration integration tests.
//!
//! Tests for configuration loading, merging, and saving.

use std::fs;
use tempfile::TempDir;

/// Test that config loads from project directory.
#[tokio::test]
async fn test_load_project_config() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a project config
    let config_content = r#"{
        "theme": "dark",
        "model": "anthropic/claude-sonnet-4-5-20250929"
    }"#;

    fs::write(temp.path().join("wonopcode.json"), config_content)
        .expect("Failed to write config");

    let (config, sources) =
        wonopcode_core::config::Config::load(Some(temp.path()))
            .await
            .expect("Failed to load config");

    assert_eq!(config.theme, Some("dark".to_string()));
    assert_eq!(
        config.model,
        Some("anthropic/claude-sonnet-4-5-20250929".to_string())
    );
    assert!(!sources.is_empty());
}

/// Test that JSONC comments are handled.
#[tokio::test]
async fn test_load_jsonc_config() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a JSONC config with comments
    let config_content = r#"{
        // This is a comment
        "theme": "light",
        /* Multi-line
           comment */
        "model": "openai/gpt-4o"
    }"#;

    fs::write(temp.path().join("wonopcode.jsonc"), config_content)
        .expect("Failed to write config");

    let (config, _) = wonopcode_core::config::Config::load(Some(temp.path()))
        .await
        .expect("Failed to load config");

    assert_eq!(config.theme, Some("light".to_string()));
    assert_eq!(config.model, Some("openai/gpt-4o".to_string()));
}

/// Test default config when no file exists.
#[tokio::test]
async fn test_default_config() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    let (config, sources) =
        wonopcode_core::config::Config::load(Some(temp.path()))
            .await
            .expect("Failed to load config");

    // Should be default values
    assert!(config.theme.is_none() || config.theme == Some(String::new()));
    // No project config source should be found
    let project_config_found = sources
        .iter()
        .any(|s| s.file_name().is_some_and(|n| n.to_string_lossy().contains("wonopcode")));
    // Global config might exist, but project config shouldn't
    assert!(!project_config_found || sources.is_empty());
}

/// Test config save and reload.
#[tokio::test]
async fn test_save_and_reload_config() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    let mut config = wonopcode_core::config::Config::default();
    config.theme = Some("custom".to_string());
    config.model = Some("anthropic/claude-haiku-4-5-20251001".to_string());

    // Save to project directory
    config
        .save(Some(temp.path()))
        .await
        .expect("Failed to save config");

    // Reload and verify
    let (loaded, _) = wonopcode_core::config::Config::load(Some(temp.path()))
        .await
        .expect("Failed to reload config");

    assert_eq!(loaded.theme, Some("custom".to_string()));
    assert_eq!(
        loaded.model,
        Some("anthropic/claude-haiku-4-5-20251001".to_string())
    );
}

/// Test config merging (project overrides global).
#[tokio::test]
async fn test_config_merge() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create project config
    let config_content = r#"{
        "theme": "project-theme",
        "model": "project/model"
    }"#;

    fs::write(temp.path().join("wonopcode.json"), config_content)
        .expect("Failed to write config");

    let (config, _) = wonopcode_core::config::Config::load(Some(temp.path()))
        .await
        .expect("Failed to load config");

    // Project values should be present
    assert_eq!(config.theme, Some("project-theme".to_string()));
    assert_eq!(config.model, Some("project/model".to_string()));
}
