//! Publish functionality for creating GitHub releases.
//!
//! This module is primarily for CI/CD and maintainers to create
//! new releases on GitHub.

use anyhow::Result;
use wonopcode_core::version::ReleaseChannel;

/// Options for publishing a release.
#[derive(Debug, Clone)]
pub struct PublishOptions {
    /// Perform a dry run without creating the release.
    pub dry_run: bool,
    /// GitHub token for authentication.
    pub token: Option<String>,
    /// Release channel.
    pub channel: ReleaseChannel,
    /// Release notes content.
    pub notes: Option<String>,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            token: None,
            channel: ReleaseChannel::Stable,
            notes: None,
        }
    }
}

/// GitHub repository for releases.
const GITHUB_REPO: &str = "wonop-io/wonopcode";

/// Format version as a git tag based on channel.
fn format_tag(version: &str, channel: ReleaseChannel) -> String {
    match channel {
        ReleaseChannel::Stable => format!("v{}", version),
        ReleaseChannel::Beta => format!("v{}-beta.1", version),
        ReleaseChannel::Nightly => {
            let date = chrono::Utc::now().format("%Y%m%d");
            format!("nightly-{}", date)
        }
    }
}

/// Extract release notes for a version from CHANGELOG.md.
fn extract_changelog_section(version: &str) -> Option<String> {
    let changelog = std::fs::read_to_string("CHANGELOG.md").ok()?;

    // Try different section formats
    let patterns = [
        format!("## [{}]", version),
        format!("## {}", version),
        format!("## v{}", version),
    ];

    for pattern in &patterns {
        if let Some(start) = changelog.find(pattern) {
            let rest = &changelog[start + pattern.len()..];
            // Find the next section (## [...] or ## ...)
            let end = rest
                .find("\n## ")
                .or_else(|| rest.find("\n# "))
                .unwrap_or(rest.len());
            return Some(rest[..end].trim().to_string());
        }
    }

    None
}

/// Create a GitHub release.
async fn create_github_release(
    token: &str,
    tag: &str,
    notes: &str,
    channel: ReleaseChannel,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::builder().user_agent("wonopcode").build()?;

    let prerelease = channel != ReleaseChannel::Stable;
    let name = match channel {
        ReleaseChannel::Stable => format!("Wonopcode {}", tag),
        ReleaseChannel::Beta => format!("Wonopcode {} (Beta)", tag),
        ReleaseChannel::Nightly => format!("Wonopcode {} (Nightly)", tag),
    };

    let response = client
        .post(format!(
            "https://api.github.com/repos/{}/releases",
            GITHUB_REPO
        ))
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&serde_json::json!({
            "tag_name": tag,
            "name": name,
            "body": notes,
            "prerelease": prerelease,
            "draft": false,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error: serde_json::Value = response.json().await.unwrap_or_default();
        anyhow::bail!(
            "GitHub API error ({}): {}",
            status,
            error["message"].as_str().unwrap_or("Unknown error")
        );
    }

    let result: serde_json::Value = response.json().await?;
    Ok(result)
}

/// Handle the publish command.
pub async fn handle_publish(options: PublishOptions) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let tag = format_tag(version, options.channel);

    println!();
    println!("Wonopcode Publish");
    println!("=================");
    println!();
    println!("Version: {}", version);
    println!("Tag:     {}", tag);
    println!("Channel: {}", options.channel);
    println!();

    // Get release notes
    let notes = options.notes.clone().unwrap_or_else(|| {
        extract_changelog_section(version).unwrap_or_else(|| {
            format!(
                "Release {} of wonopcode.\n\nSee https://github.com/{}/blob/main/CHANGELOG.md for details.",
                version, GITHUB_REPO
            )
        })
    });

    if options.dry_run {
        println!("[DRY RUN] Would create release:");
        println!();
        println!("  Tag:        {}", tag);
        println!("  Channel:    {:?}", options.channel);
        println!(
            "  Pre-release: {}",
            options.channel != ReleaseChannel::Stable
        );
        println!();
        println!("Release notes:");
        println!("---");
        for line in notes.lines().take(10) {
            println!("  {}", line);
        }
        if notes.lines().count() > 10 {
            println!("  ... ({} more lines)", notes.lines().count() - 10);
        }
        println!("---");
        println!();
        println!("Run without --dry-run to create the release.");
        return Ok(());
    }

    // Get GitHub token
    let token = options
        .token
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "GitHub token required. Use --token or set GITHUB_TOKEN environment variable."
            )
        })?;

    println!("Creating GitHub release...");

    let result = create_github_release(&token, &tag, &notes, options.channel).await?;

    let html_url = result["html_url"].as_str().unwrap_or("(unknown)");

    println!();
    println!("Release created successfully!");
    println!();
    println!("URL: {}", html_url);
    println!();
    println!("GitHub Actions will build and attach binaries.");
    println!("Check: https://github.com/{}/actions", GITHUB_REPO);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tag() {
        assert_eq!(format_tag("0.1.0", ReleaseChannel::Stable), "v0.1.0");
        assert_eq!(format_tag("0.1.0", ReleaseChannel::Beta), "v0.1.0-beta.1");
        // Nightly includes date, so we just check the prefix
        assert!(format_tag("0.1.0", ReleaseChannel::Nightly).starts_with("nightly-"));
    }
}
