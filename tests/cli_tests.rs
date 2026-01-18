//! CLI integration tests.
//!
//! These tests exercise the CLI commands end-to-end.

use std::fs;
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

// === Version and Help Commands ===

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
fn test_subcommand_help() {
    // Test help for each subcommand
    let subcommands = ["models", "auth", "session", "mcp", "config"];

    for cmd in subcommands {
        let output = Command::new(binary_path())
            .args([cmd, "--help"])
            .output()
            .expect(&format!("Failed to execute {cmd} --help"));

        assert!(
            output.status.success(),
            "{cmd} --help should succeed, got: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

// === Models Command ===

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
fn test_models_with_provider_filter() {
    let output = Command::new(binary_path())
        .args(["models", "--provider", "anthropic"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show Anthropic models
    assert!(
        stdout.contains("claude") || stdout.contains("Anthropic") || stdout.contains("anthropic")
    );
}

// === Auth Commands ===

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
fn test_auth_help() {
    let output = Command::new(binary_path())
        .args(["auth", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("status") || stdout.contains("Authentication"));
}

// === Session Commands ===

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
fn test_session_list_empty() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(binary_path())
        .args(["session", "list"])
        .current_dir(temp_dir.path())
        .env("HOME", temp_dir.path()) // Use temp dir as home to avoid reading real sessions
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_session_help() {
    let output = Command::new(binary_path())
        .args(["session", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("Session"));
}

// === MCP Commands ===

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
fn test_mcp_help() {
    let output = Command::new(binary_path())
        .args(["mcp", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list") || stdout.contains("MCP") || stdout.contains("server"));
}

// === Config Commands ===

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
fn test_config_with_project_config() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    // Create a project config file
    let config_content = r#"{
        "model": "anthropic/claude-sonnet-4-5-20250929",
        "theme": "dark"
    }"#;
    fs::write(temp_dir.path().join("wonopcode.json"), config_content)
        .expect("Failed to write config");

    let output = Command::new(binary_path())
        .arg("config")
        .current_dir(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show the project config
    assert!(stdout.contains("Configuration"));
}

// === Error Handling ===

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

#[test]
fn test_invalid_subcommand() {
    let output = Command::new(binary_path())
        .arg("nonexistent-subcommand")
        .output()
        .expect("Failed to execute command");

    // Should fail with an error
    assert!(!output.status.success());
}

#[test]
fn test_invalid_flag() {
    let output = Command::new(binary_path())
        .arg("--nonexistent-flag")
        .output()
        .expect("Failed to execute command");

    // Should fail with an error
    assert!(!output.status.success());
}

// === Provider and Model Selection ===

#[test]
fn test_model_flag_accepted() {
    // Just test that the model flag is accepted, not that it works
    // (would need API keys for that)
    let output = Command::new(binary_path())
        .args(["--model", "anthropic/claude-sonnet-4-5-20250929", "--help"])
        .output()
        .expect("Failed to execute command");

    // --help should still work even with --model specified
    assert!(output.status.success());
}

#[test]
fn test_provider_flag_accepted() {
    let output = Command::new(binary_path())
        .args(["--provider", "anthropic", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

// === Output Format Tests ===

#[test]
fn test_version_output_format() {
    let output = Command::new(binary_path())
        .arg("version")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should contain version number in expected format
    assert!(
        stdout.contains("0.") || stdout.contains("1."),
        "Version output should contain version number"
    );
}

#[test]
fn test_models_output_contains_providers() {
    let output = Command::new(binary_path())
        .arg("models")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should list at least some known providers
    let has_known_provider = stdout.contains("anthropic")
        || stdout.contains("Anthropic")
        || stdout.contains("openai")
        || stdout.contains("OpenAI")
        || stdout.contains("google")
        || stdout.contains("Google");

    assert!(
        has_known_provider,
        "Models output should contain known providers"
    );
}

// === Environment Variable Tests ===

#[test]
fn test_respects_no_color_env() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(binary_path())
        .arg("--help")
        .env("NO_COLOR", "1")
        .current_dir(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output should not contain ANSI escape codes when NO_COLOR is set
    assert!(
        !stdout.contains("\x1b["),
        "Output should not contain ANSI codes when NO_COLOR=1"
    );
}

// === Working Directory Tests ===

#[test]
fn test_works_from_any_directory() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let output = Command::new(binary_path())
        .arg("--help")
        .current_dir(temp_dir.path())
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_config_detects_project_root() {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    // Create a git repo structure
    fs::create_dir(temp_dir.path().join(".git")).expect("Failed to create .git");
    fs::create_dir(temp_dir.path().join("subdir")).expect("Failed to create subdir");

    // Run from subdir
    let output = Command::new(binary_path())
        .arg("config")
        .current_dir(temp_dir.path().join("subdir"))
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}
