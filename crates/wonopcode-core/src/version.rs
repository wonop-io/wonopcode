//! Semantic versioning with pre-release support.
//!
//! Supports versions like:
//! - `1.2.3` (stable)
//! - `1.2.3-beta.1` (beta)
//! - `1.2.3-alpha.2` (alpha)
//! - `1.2.3-rc.1` (release candidate)
//! - `nightly-20260108` (nightly)

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

/// Semantic version with optional pre-release tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre_release: Option<PreRelease>,
}

/// Pre-release version type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreRelease {
    /// Alpha release (e.g., alpha.1)
    Alpha(u32),
    /// Beta release (e.g., beta.1)
    Beta(u32),
    /// Release candidate (e.g., rc.1)
    Rc(u32),
    /// Nightly build (e.g., nightly-20260108)
    Nightly(String),
}

/// Release channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseChannel {
    /// Stable releases only.
    #[default]
    Stable,
    /// Beta and stable releases.
    Beta,
    /// All releases including nightly.
    Nightly,
}

impl Version {
    /// Create a new version.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            pre_release: None,
        }
    }

    /// Create a new version with pre-release tag.
    pub fn with_pre_release(major: u32, minor: u32, patch: u32, pre_release: PreRelease) -> Self {
        Self {
            major,
            minor,
            patch,
            pre_release: Some(pre_release),
        }
    }

    /// Parse version from string.
    ///
    /// Supports formats:
    /// - `1.2.3`
    /// - `v1.2.3`
    /// - `1.2.3-beta.1`
    /// - `nightly-20260108`
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        // Handle nightly builds specially
        if s.starts_with("nightly-") {
            return Some(Self {
                major: 0,
                minor: 0,
                patch: 0,
                pre_release: Some(PreRelease::Nightly(s.to_string())),
            });
        }

        let s = s.strip_prefix('v').unwrap_or(s);

        // Split version and pre-release
        let (version_part, pre_release_str) = if let Some((v, p)) = s.split_once('-') {
            (v, Some(p))
        } else {
            (s, None)
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        let major = parts.first()?.parse().ok()?;
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

        let pre_release = pre_release_str.and_then(Self::parse_pre_release);

        Some(Self {
            major,
            minor,
            patch,
            pre_release,
        })
    }

    /// Parse pre-release string.
    fn parse_pre_release(s: &str) -> Option<PreRelease> {
        if let Some(n) = s.strip_prefix("beta.") {
            Some(PreRelease::Beta(n.parse().ok()?))
        } else if let Some(n) = s.strip_prefix("alpha.") {
            Some(PreRelease::Alpha(n.parse().ok()?))
        } else if let Some(n) = s.strip_prefix("rc.") {
            Some(PreRelease::Rc(n.parse().ok()?))
        } else if s.starts_with("nightly") {
            Some(PreRelease::Nightly(s.to_string()))
        } else if s == "beta" {
            Some(PreRelease::Beta(0))
        } else if s == "alpha" {
            Some(PreRelease::Alpha(0))
        } else if s == "rc" {
            Some(PreRelease::Rc(0))
        } else {
            None
        }
    }

    /// Check if this is a stable release (no pre-release tag).
    pub fn is_stable(&self) -> bool {
        self.pre_release.is_none()
    }

    /// Check if this is a beta release.
    pub fn is_beta(&self) -> bool {
        matches!(
            self.pre_release,
            Some(PreRelease::Beta(_)) | Some(PreRelease::Alpha(_)) | Some(PreRelease::Rc(_))
        )
    }

    /// Check if this is a nightly release.
    pub fn is_nightly(&self) -> bool {
        matches!(self.pre_release, Some(PreRelease::Nightly(_)))
    }

    /// Get the release channel for this version.
    pub fn channel(&self) -> ReleaseChannel {
        match &self.pre_release {
            None => ReleaseChannel::Stable,
            Some(PreRelease::Nightly(_)) => ReleaseChannel::Nightly,
            Some(_) => ReleaseChannel::Beta,
        }
    }

    /// Check if this version matches the given channel filter.
    ///
    /// - Stable channel: only stable versions
    /// - Beta channel: stable and beta versions
    /// - Nightly channel: all versions
    pub fn matches_channel(&self, channel: ReleaseChannel) -> bool {
        match channel {
            ReleaseChannel::Stable => self.is_stable(),
            ReleaseChannel::Beta => !self.is_nightly(),
            ReleaseChannel::Nightly => true,
        }
    }

    /// Format as a Git tag.
    pub fn to_tag(&self) -> String {
        if let Some(PreRelease::Nightly(ref s)) = self.pre_release {
            s.clone()
        } else {
            format!("v{}", self)
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.pre_release {
            write!(f, "-{}", pre)?;
        }
        Ok(())
    }
}

impl fmt::Display for PreRelease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreRelease::Alpha(n) => {
                if *n == 0 {
                    write!(f, "alpha")
                } else {
                    write!(f, "alpha.{}", n)
                }
            }
            PreRelease::Beta(n) => {
                if *n == 0 {
                    write!(f, "beta")
                } else {
                    write!(f, "beta.{}", n)
                }
            }
            PreRelease::Rc(n) => {
                if *n == 0 {
                    write!(f, "rc")
                } else {
                    write!(f, "rc.{}", n)
                }
            }
            PreRelease::Nightly(s) => write!(f, "{}", s),
        }
    }
}

impl fmt::Display for ReleaseChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReleaseChannel::Stable => write!(f, "stable"),
            ReleaseChannel::Beta => write!(f, "beta"),
            ReleaseChannel::Nightly => write!(f, "nightly"),
        }
    }
}

impl Ord for PreRelease {
    fn cmp(&self, other: &Self) -> Ordering {
        use PreRelease::*;

        match (self, other) {
            // Same type: compare by number/string
            (Alpha(a), Alpha(b)) => a.cmp(b),
            (Beta(a), Beta(b)) => a.cmp(b),
            (Rc(a), Rc(b)) => a.cmp(b),
            (Nightly(a), Nightly(b)) => a.cmp(b),

            // Different types: alpha < beta < rc < nightly
            (Alpha(_), _) => Ordering::Less,
            (_, Alpha(_)) => Ordering::Greater,
            (Beta(_), Rc(_)) | (Beta(_), Nightly(_)) => Ordering::Less,
            (Rc(_), Beta(_)) | (Nightly(_), Beta(_)) => Ordering::Greater,
            (Rc(_), Nightly(_)) => Ordering::Less,
            (Nightly(_), Rc(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for PreRelease {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare major.minor.patch first
        match (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Pre-release versions are less than release versions
        match (&self.pre_release, &other.pre_release) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stable() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert!(v.is_stable());
    }

    #[test]
    fn test_parse_with_v_prefix() {
        let v = Version::parse("v1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn test_parse_beta() {
        let v = Version::parse("1.2.3-beta.1").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.pre_release, Some(PreRelease::Beta(1)));
        assert!(v.is_beta());
    }

    #[test]
    fn test_parse_nightly() {
        let v = Version::parse("nightly-20260108").unwrap();
        assert!(v.is_nightly());
        assert_eq!(v.channel(), ReleaseChannel::Nightly);
    }

    #[test]
    fn test_version_ordering() {
        let v1 = Version::parse("1.0.0").unwrap();
        let v2 = Version::parse("1.0.1").unwrap();
        let v3 = Version::parse("1.1.0").unwrap();
        let v4 = Version::parse("2.0.0").unwrap();

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v3 < v4);
    }

    #[test]
    fn test_prerelease_less_than_release() {
        let beta = Version::parse("1.0.0-beta.1").unwrap();
        let stable = Version::parse("1.0.0").unwrap();

        assert!(beta < stable);
    }

    #[test]
    fn test_prerelease_ordering() {
        let alpha = Version::parse("1.0.0-alpha.1").unwrap();
        let beta = Version::parse("1.0.0-beta.1").unwrap();
        let rc = Version::parse("1.0.0-rc.1").unwrap();
        let stable = Version::parse("1.0.0").unwrap();

        assert!(alpha < beta);
        assert!(beta < rc);
        assert!(rc < stable);
    }

    #[test]
    fn test_channel_matching() {
        let stable = Version::parse("1.0.0").unwrap();
        let beta = Version::parse("1.0.0-beta.1").unwrap();
        let nightly = Version::parse("nightly-20260108").unwrap();

        // Stable channel
        assert!(stable.matches_channel(ReleaseChannel::Stable));
        assert!(!beta.matches_channel(ReleaseChannel::Stable));
        assert!(!nightly.matches_channel(ReleaseChannel::Stable));

        // Beta channel
        assert!(stable.matches_channel(ReleaseChannel::Beta));
        assert!(beta.matches_channel(ReleaseChannel::Beta));
        assert!(!nightly.matches_channel(ReleaseChannel::Beta));

        // Nightly channel
        assert!(stable.matches_channel(ReleaseChannel::Nightly));
        assert!(beta.matches_channel(ReleaseChannel::Nightly));
        assert!(nightly.matches_channel(ReleaseChannel::Nightly));
    }

    #[test]
    fn test_display() {
        assert_eq!(Version::parse("1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(
            Version::parse("1.2.3-beta.1").unwrap().to_string(),
            "1.2.3-beta.1"
        );
    }

    #[test]
    fn test_to_tag() {
        assert_eq!(Version::parse("1.2.3").unwrap().to_tag(), "v1.2.3");
        assert_eq!(
            Version::parse("1.2.3-beta.1").unwrap().to_tag(),
            "v1.2.3-beta.1"
        );
        assert_eq!(
            Version::parse("nightly-20260108").unwrap().to_tag(),
            "nightly-20260108"
        );
    }
}
