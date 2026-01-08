//! Upgrade and update functionality for wonopcode.
//!
//! This module handles:
//! - Checking for updates from GitHub releases
//! - Downloading and installing new versions
//! - Release channel filtering (stable, beta, nightly)
//! - Auto-update on startup

use anyhow::{Context, Result};
use std::path::Path;
use wonopcode_core::config::{AutoUpdateMode, Config};
use wonopcode_core::version::{ReleaseChannel, Version};

/// GitHub release information.
#[derive(Debug, Clone)]
pub struct Release {
    /// Parsed version.
    pub version: Version,
    /// Git tag name.
    pub tag: String,
    /// Release assets (binaries).
    pub assets: Vec<Asset>,
    /// Release notes body.
    #[allow(dead_code)]
    pub body: String,
    /// Whether this is a pre-release.
    #[allow(dead_code)]
    pub prerelease: bool,
    /// Publication timestamp.
    #[allow(dead_code)]
    pub published_at: String,
}

/// Release asset (downloadable file).
#[derive(Debug, Clone)]
pub struct Asset {
    /// File name.
    pub name: String,
    /// Download URL.
    pub download_url: String,
    /// File size in bytes.
    pub size: u64,
}

/// Result of checking for updates.
#[derive(Debug)]
pub struct CheckResult {
    /// Current installed version.
    pub current: Version,
    /// Latest available release (if any).
    pub latest: Option<Release>,
    /// Whether an update is available.
    pub update_available: bool,
}

/// GitHub repository for releases.
const GITHUB_REPO: &str = "wonop-io/wonopcode";

/// Fetch releases from GitHub API.
pub async fn fetch_releases(channel: ReleaseChannel) -> Result<Vec<Release>> {
    let client = reqwest::Client::builder()
        .user_agent("wonopcode")
        .build()?;

    let url = format!("https://api.github.com/repos/{}/releases", GITHUB_REPO);
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub API error: {}", response.status());
    }

    let releases: Vec<serde_json::Value> = response.json().await?;

    let mut result = Vec::new();
    for release in releases {
        let tag = release["tag_name"].as_str().unwrap_or_default();
        let version = match Version::parse(tag) {
            Some(v) => v,
            None => continue,
        };

        // Filter by channel
        if !version.matches_channel(channel) {
            continue;
        }

        let assets = release["assets"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| {
                        Some(Asset {
                            name: a["name"].as_str()?.to_string(),
                            download_url: a["browser_download_url"].as_str()?.to_string(),
                            size: a["size"].as_u64().unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        result.push(Release {
            version,
            tag: tag.to_string(),
            assets,
            body: release["body"].as_str().unwrap_or_default().to_string(),
            prerelease: release["prerelease"].as_bool().unwrap_or(false),
            published_at: release["published_at"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        });
    }

    // Sort by version descending (newest first)
    result.sort_by(|a, b| b.version.cmp(&a.version));

    Ok(result)
}

/// Fetch the latest release from GitHub.
pub async fn fetch_latest_release(channel: ReleaseChannel) -> Result<Option<Release>> {
    let releases = fetch_releases(channel).await?;
    Ok(releases.into_iter().next())
}

/// Check for available updates.
pub async fn check_for_update(channel: ReleaseChannel) -> Result<CheckResult> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .ok_or_else(|| anyhow::anyhow!("Invalid current version"))?;

    let latest = fetch_latest_release(channel).await?;

    let update_available = latest
        .as_ref()
        .map(|r| r.version > current)
        .unwrap_or(false);

    Ok(CheckResult {
        current,
        latest,
        update_available,
    })
}

/// Find the appropriate asset for the current platform.
pub fn find_platform_asset(release: &Release) -> Result<&Asset> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        os => os,
    };
    let arch = std::env::consts::ARCH;

    release
        .assets
        .iter()
        .find(|a| {
            let name = a.name.to_lowercase();
            name.contains(os)
                && name.contains(arch)
                && !name.ends_with(".sha256")
                && !name.ends_with(".md5")
        })
        .ok_or_else(|| anyhow::anyhow!("No binary available for {}-{}", os, arch))
}

/// Format file size for display.
pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Download and install a release.
pub async fn install_release(release: &Release) -> Result<()> {
    let asset = find_platform_asset(release)?;

    println!("Downloading {} ({})...", release.tag, format_size(asset.size));

    // Download
    let client = reqwest::Client::builder()
        .user_agent("wonopcode")
        .build()?;

    let response = client.get(&asset.download_url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Download failed: {}", response.status());
    }

    let bytes = response.bytes().await?;

    // Install binary
    install_binary(&bytes, &asset.name).await?;

    Ok(())
}

/// Install binary from downloaded bytes.
async fn install_binary(bytes: &[u8], filename: &str) -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine binary directory"))?;

    let temp_path = parent.join(".wonopcode.tmp");

    // Extract if archive
    if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let decoder = GzDecoder::new(bytes);
        let mut archive = Archive::new(decoder);

        let mut found = false;
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.path()?.file_name().is_some_and(|n| n == "wonopcode") {
                entry.unpack(&temp_path)?;
                found = true;
                break;
            }
        }
        if !found {
            anyhow::bail!("Binary not found in archive");
        }
    } else if filename.ends_with(".zip") {
        use std::io::Cursor;
        use zip::ZipArchive;

        let cursor = Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor)?;

        let mut found = false;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_name = file.name().to_string();
            if file_name.contains("wonopcode") && !file_name.ends_with('/') {
                let mut outfile = std::fs::File::create(&temp_path)?;
                std::io::copy(&mut file, &mut outfile)?;
                found = true;
                break;
            }
        }
        if !found {
            anyhow::bail!("Binary not found in archive");
        }
    } else {
        // Raw binary
        tokio::fs::write(&temp_path, bytes).await?;
    }

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&temp_path, perms)?;
    }

    // Atomic replace
    let backup_path = parent.join(".wonopcode.bak");
    if backup_path.exists() {
        std::fs::remove_file(&backup_path)?;
    }
    std::fs::rename(&current_exe, &backup_path)?;
    std::fs::rename(&temp_path, &current_exe)?;

    // Clean up backup
    let _ = std::fs::remove_file(&backup_path);

    Ok(())
}

/// Get the path for storing last update check timestamp.
fn get_update_cache_path() -> Option<std::path::PathBuf> {
    Config::data_dir().map(|d| d.join("last_update_check"))
}

/// Check if we should perform an update check based on interval.
pub fn should_check_update(config: &Config) -> bool {
    let interval_hours = config
        .update
        .as_ref()
        .and_then(|u| u.check_interval)
        .unwrap_or(24);

    let cache_path = match get_update_cache_path() {
        Some(p) => p,
        None => return true,
    };

    if let Ok(content) = std::fs::read_to_string(&cache_path) {
        if let Ok(timestamp) = content.trim().parse::<i64>() {
            if let Some(last_check) = chrono::DateTime::from_timestamp(timestamp, 0) {
                let now = chrono::Utc::now();
                let elapsed = now.signed_duration_since(last_check);
                if elapsed.num_hours() < interval_hours as i64 {
                    return false;
                }
            }
        }
    }

    // Update the cache timestamp
    if let Some(cache_path) = get_update_cache_path() {
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&cache_path, chrono::Utc::now().timestamp().to_string());
    }

    true
}

/// Get the effective auto-update mode.
pub fn get_auto_update_mode(config: &Config) -> AutoUpdateMode {
    // Check new update config first
    if let Some(ref update) = config.update {
        if let Some(mode) = update.auto {
            return mode;
        }
    }

    // Fall back to legacy autoupdate field
    if let Some(ref autoupdate) = config.autoupdate {
        return match autoupdate {
            wonopcode_core::config::AutoUpdate::Bool(true) => AutoUpdateMode::Auto,
            wonopcode_core::config::AutoUpdate::Bool(false) => AutoUpdateMode::Disabled,
            wonopcode_core::config::AutoUpdate::Notify => AutoUpdateMode::Notify,
        };
    }

    // Default to notify
    AutoUpdateMode::Notify
}

/// Get the effective release channel.
pub fn get_release_channel(config: &Config) -> ReleaseChannel {
    config
        .update
        .as_ref()
        .and_then(|u| u.channel)
        .unwrap_or_default()
}

/// Check for updates on startup and optionally auto-install.
///
/// Returns an optional notification message to show the user.
pub async fn check_updates_on_startup(cwd: &Path) -> Option<String> {
    // Load config
    let (config, _) = Config::load(Some(cwd)).await.ok()?;

    let mode = get_auto_update_mode(&config);

    // Check if updates are disabled
    if mode == AutoUpdateMode::Disabled {
        return None;
    }

    // Check if we should check (based on interval)
    if !should_check_update(&config) {
        return None;
    }

    let channel = get_release_channel(&config);

    // Check for updates
    let result = match check_for_update(channel).await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("Update check failed: {}", e);
            return None;
        }
    };

    if !result.update_available {
        return None;
    }

    let latest = result.latest.as_ref()?;

    match mode {
        AutoUpdateMode::Auto => {
            // Auto-install
            match install_release(latest).await {
                Ok(_) => Some(format!(
                    "Updated wonopcode to {} - restart to use new version",
                    latest.version
                )),
                Err(e) => {
                    tracing::warn!("Auto-update failed: {}", e);
                    Some(format!(
                        "Update available: {} -> {} (auto-update failed: {})",
                        result.current, latest.version, e
                    ))
                }
            }
        }
        AutoUpdateMode::Notify => Some(format!(
            "Update available: {} -> {} (run 'wonopcode upgrade' to install)",
            result.current, latest.version
        )),
        AutoUpdateMode::Disabled => None,
    }
}



/// Handle the check command.
pub async fn handle_check(channel: Option<ReleaseChannel>, json: bool) -> Result<()> {
    use std::io::Write;

    let channel = channel.unwrap_or_default();

    print!("Checking for updates... ");
    std::io::stdout().flush()?;

    let result = check_for_update(channel).await?;

    println!("done");
    println!();

    if json {
        let json_output = serde_json::json!({
            "current_version": result.current.to_string(),
            "latest_version": result.latest.as_ref().map(|r| r.version.to_string()),
            "channel": channel.to_string(),
            "update_available": result.update_available,
            "download_url": result.latest.as_ref().and_then(|r| find_platform_asset(r).ok()).map(|a| &a.download_url),
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("Current version: {} ({})", result.current, channel);

        if let Some(ref release) = result.latest {
            println!("Latest version:  {}", release.version);
            println!();

            if result.update_available {
                println!("A new version is available!");
                println!();
                println!("Run 'wonopcode upgrade' to install.");
            } else {
                println!("You are running the latest version.");
            }
        } else {
            println!("No releases found for channel '{}'", channel);
        }
    }

    Ok(())
}

/// Handle the upgrade command.
pub async fn handle_upgrade(
    yes: bool,
    channel: Option<ReleaseChannel>,
    version: Option<String>,
    force: bool,
) -> Result<()> {
    use std::io::{self, Write};

    println!();
    println!("Wonopcode Upgrade");
    println!("=================");
    println!();

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .ok_or_else(|| anyhow::anyhow!("Invalid current version"))?;

    println!("Current version: {}", current);

    // Determine which release to install
    let release = if let Some(ref ver) = version {
        // Specific version requested
        print!("Finding version {}... ", ver);
        io::stdout().flush()?;

        let target_version =
            Version::parse(ver).ok_or_else(|| anyhow::anyhow!("Invalid version: {}", ver))?;

        let releases = fetch_releases(ReleaseChannel::Nightly).await?; // Fetch all
        releases
            .into_iter()
            .find(|r| r.version == target_version)
            .ok_or_else(|| anyhow::anyhow!("Version {} not found", ver))?
    } else {
        // Latest version for channel
        let channel = channel.unwrap_or_default();

        print!("Checking for updates ({})... ", channel);
        io::stdout().flush()?;

        fetch_latest_release(channel)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No releases found for channel '{}'", channel))?
    };

    println!("done");
    println!("Latest version:  {}", release.version);
    println!();

    // Check if upgrade is needed
    if !force && current >= release.version {
        println!("You are already running the latest version.");
        return Ok(());
    }

    // Confirm upgrade
    if !yes {
        print!("Upgrade to {}? [y/N] ", release.version);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Upgrade cancelled.");
            return Ok(());
        }
    }

    // Verify platform asset exists before downloading
    let _asset = find_platform_asset(&release).context("No binary available for your platform")?;

    println!();
    println!("Downloading {}...", release.version);

    // Download and install
    install_release(&release).await?;

    println!();
    println!("Successfully upgraded to {}!", release.version);
    println!();
    println!("Restart wonopcode to use the new version.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1500), "1.5 KB");
        assert_eq!(format_size(1_500_000), "1.5 MB");
    }
}
