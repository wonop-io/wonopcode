//! CLI integration tests.
//!
//! These tests exercise the CLI commands end-to-end.

use std::process::Command;

/// Get the path to the wonopcode binary.
fn binary_path() -> String {
    // In test mode, the binary might be in target/debug or target/release
    let mut path = std::env::current_exe()
        .expect("Failed to get current exe")
        .parent()
        .expect("Failed to get parent directory")
        .to_path_buf();

    // Go up from deps directory
    if path.ends_with("deps") {
        path.pop();
    }

    path.join("wonopcode").to_string_lossy().to_string()
}

#[test]
fn test_version_command() {
    let output = Command::new(binary_path())
        .arg("version")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("wonopcode"));
}

#[test]
fn test_help_command() {
    let output = Command::new(binary_path())
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AI-powered coding assistant"));
    assert!(stdout.contains("--provider"));
    assert!(stdout.contains("--model"));
}

#[test]
fn test_models_command() {
    let output = Command::new(binary_path())
        .arg("models")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Available models"));
    assert!(stdout.contains("claude"));
}

#[test]
fn test_auth_status_command() {
    let output = Command::new(binary_path())
        .args(["auth", "status"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Authentication status"));
}

#[test]
fn test_session_list_command() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(binary_path())
        .args(["session", "list"])
        .current_dir(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    // Either succeeds or says no sessions found
    assert!(output.status.success());
}

#[test]
fn test_mcp_list_command() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(binary_path())
        .args(["mcp", "list"])
        .current_dir(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_config_command() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(binary_path())
        .arg("config")
        .current_dir(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Configuration"));
}

#[test]
fn test_invalid_provider() {
    let output = Command::new(binary_path())
        .args(["--provider", "invalid-provider-name", "run", "test"])
        .output()
        .expect("Failed to execute command");

    // Should either fail gracefully or show an error
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The error message should mention something about the provider
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("Error")
            || combined.contains("error")
            || combined.contains("provider")
            || !output.status.success()
    );
}
