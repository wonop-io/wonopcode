//! Shared types for the ACE framework integration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Artifact types supported at ticket level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactType {
    UseCase,
    Requirement,
    Design,
    TestCase,
    Task,
}

impl ArtifactType {
    /// Get the ID prefix for this artifact type.
    pub fn prefix(&self) -> &'static str {
        match self {
            ArtifactType::UseCase => "UC",
            ArtifactType::Requirement => "REQ",
            ArtifactType::Design => "DES",
            ArtifactType::TestCase => "TC",
            ArtifactType::Task => "TASK",
        }
    }

    /// Get the directory name for this artifact type.
    pub fn directory(&self) -> &'static str {
        match self {
            ArtifactType::UseCase => "use-cases",
            ArtifactType::Requirement => "requirements",
            ArtifactType::Design => "designs",
            ArtifactType::TestCase => "tests",
            ArtifactType::Task => "tasks",
        }
    }

    /// Parse artifact type from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "use-case" | "usecase" | "uc" => Some(ArtifactType::UseCase),
            "requirement" | "req" => Some(ArtifactType::Requirement),
            "design" | "des" => Some(ArtifactType::Design),
            "test-case" | "testcase" | "test" | "tc" => Some(ArtifactType::TestCase),
            "task" => Some(ArtifactType::Task),
            _ => None,
        }
    }

    /// Parse artifact type from ID prefix.
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "UC" => Some(ArtifactType::UseCase),
            "REQ" => Some(ArtifactType::Requirement),
            "DES" => Some(ArtifactType::Design),
            "TC" => Some(ArtifactType::TestCase),
            "TASK" => Some(ArtifactType::Task),
            _ => None,
        }
    }

    /// Returns valid parent types for this artifact type.
    pub fn valid_parent_types(&self) -> &'static [ArtifactType] {
        match self {
            ArtifactType::UseCase => &[], // UC has no parents (ticket is implicit)
            ArtifactType::Requirement => &[ArtifactType::UseCase],
            ArtifactType::Design => &[ArtifactType::Requirement],
            ArtifactType::TestCase => &[ArtifactType::Requirement],
            ArtifactType::Task => &[
                ArtifactType::Requirement,
                ArtifactType::Design,
                ArtifactType::TestCase,
            ],
        }
    }

    /// Returns true if this type requires at least one parent.
    pub fn requires_parent(&self) -> bool {
        !self.valid_parent_types().is_empty()
    }
}

impl fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.directory())
    }
}

/// Progress states for artifacts (matches ACE Progress enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    #[default]
    Backlog,
    InProgress,
    Blocked,
    Parked,
    ReadyToValidate,
    Done,
    Discarded,
}

impl Progress {
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Progress::Backlog => "backlog",
            Progress::InProgress => "in_progress",
            Progress::Blocked => "blocked",
            Progress::Parked => "parked",
            Progress::ReadyToValidate => "ready_to_validate",
            Progress::Done => "done",
            Progress::Discarded => "discarded",
        }
    }

    /// Parse from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "backlog" => Some(Progress::Backlog),
            "in_progress" => Some(Progress::InProgress),
            "blocked" => Some(Progress::Blocked),
            "parked" => Some(Progress::Parked),
            "ready_to_validate" => Some(Progress::ReadyToValidate),
            "done" => Some(Progress::Done),
            "discarded" => Some(Progress::Discarded),
            _ => None,
        }
    }

    /// Returns true if this is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Progress::Done | Progress::Discarded)
    }

    /// Get display icon for the progress state.
    pub fn icon(&self) -> &'static str {
        match self {
            Progress::Backlog => "○",
            Progress::InProgress => "▶",
            Progress::Blocked => "⊘",
            Progress::Parked => "⏸",
            Progress::ReadyToValidate => "◎",
            Progress::Done => "●",
            Progress::Discarded => "✗",
        }
    }
}

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    #[default]
    Medium,
    Low,
}

impl Priority {
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }

    /// Parse from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "high" => Some(Priority::High),
            "medium" => Some(Priority::Medium),
            "low" => Some(Priority::Low),
            _ => None,
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Workflow phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowPhase {
    #[default]
    Requirements,
    Analysis,
    Design,
    Implementation,
    Verification,
    Deployment,
}

impl WorkflowPhase {
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowPhase::Requirements => "requirements",
            WorkflowPhase::Analysis => "analysis",
            WorkflowPhase::Design => "design",
            WorkflowPhase::Implementation => "implementation",
            WorkflowPhase::Verification => "verification",
            WorkflowPhase::Deployment => "deployment",
        }
    }

    /// Parse from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "requirements" => Some(WorkflowPhase::Requirements),
            "analysis" => Some(WorkflowPhase::Analysis),
            "design" => Some(WorkflowPhase::Design),
            "implementation" => Some(WorkflowPhase::Implementation),
            "verification" => Some(WorkflowPhase::Verification),
            "deployment" => Some(WorkflowPhase::Deployment),
            _ => None,
        }
    }

    /// Get the next phase in the workflow.
    pub fn next(&self) -> Option<Self> {
        match self {
            WorkflowPhase::Requirements => Some(WorkflowPhase::Analysis),
            WorkflowPhase::Analysis => Some(WorkflowPhase::Design),
            WorkflowPhase::Design => Some(WorkflowPhase::Implementation),
            WorkflowPhase::Implementation => Some(WorkflowPhase::Verification),
            WorkflowPhase::Verification => Some(WorkflowPhase::Deployment),
            WorkflowPhase::Deployment => None,
        }
    }

    /// Get all phases in order.
    pub fn all() -> &'static [WorkflowPhase] {
        &[
            WorkflowPhase::Requirements,
            WorkflowPhase::Analysis,
            WorkflowPhase::Design,
            WorkflowPhase::Implementation,
            WorkflowPhase::Verification,
            WorkflowPhase::Deployment,
        ]
    }
}

impl fmt::Display for WorkflowPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Phase status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    #[default]
    Pending,
    InProgress,
    AwaitingApproval,
    Completed,
}

impl PhaseStatus {
    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            PhaseStatus::Pending => "pending",
            PhaseStatus::InProgress => "in_progress",
            PhaseStatus::AwaitingApproval => "awaiting_approval",
            PhaseStatus::Completed => "completed",
        }
    }
}

impl fmt::Display for PhaseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Artifact metadata (YAML frontmatter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub id: String,
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    pub progress: Progress,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub priority: Priority,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    #[serde(default = "default_author")]
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<DateTime<Utc>>,
}

fn default_author() -> String {
    "agent".to_string()
}

/// Complete artifact with metadata and content.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub metadata: ArtifactMetadata,
    pub title: String,
    pub content: String,
    pub path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_type_from_str() {
        assert_eq!(ArtifactType::parse("use-case"), Some(ArtifactType::UseCase));
        assert_eq!(
            ArtifactType::parse("requirement"),
            Some(ArtifactType::Requirement)
        );
        assert_eq!(ArtifactType::parse("design"), Some(ArtifactType::Design));
        assert_eq!(
            ArtifactType::parse("test-case"),
            Some(ArtifactType::TestCase)
        );
        assert_eq!(ArtifactType::parse("task"), Some(ArtifactType::Task));
        assert_eq!(ArtifactType::parse("invalid"), None);
    }

    #[test]
    fn test_artifact_type_prefix() {
        assert_eq!(ArtifactType::UseCase.prefix(), "UC");
        assert_eq!(ArtifactType::Requirement.prefix(), "REQ");
        assert_eq!(ArtifactType::Design.prefix(), "DES");
        assert_eq!(ArtifactType::TestCase.prefix(), "TC");
        assert_eq!(ArtifactType::Task.prefix(), "TASK");
    }

    #[test]
    fn test_artifact_type_valid_parents() {
        assert!(ArtifactType::UseCase.valid_parent_types().is_empty());
        assert_eq!(
            ArtifactType::Requirement.valid_parent_types(),
            &[ArtifactType::UseCase]
        );
        assert_eq!(
            ArtifactType::Design.valid_parent_types(),
            &[ArtifactType::Requirement]
        );
        assert_eq!(
            ArtifactType::TestCase.valid_parent_types(),
            &[ArtifactType::Requirement]
        );
        assert_eq!(
            ArtifactType::Task.valid_parent_types(),
            &[
                ArtifactType::Requirement,
                ArtifactType::Design,
                ArtifactType::TestCase
            ]
        );
    }

    #[test]
    fn test_progress_is_terminal() {
        assert!(!Progress::Backlog.is_terminal());
        assert!(!Progress::InProgress.is_terminal());
        assert!(!Progress::Blocked.is_terminal());
        assert!(Progress::Done.is_terminal());
        assert!(Progress::Discarded.is_terminal());
    }

    #[test]
    fn test_workflow_phase_next() {
        assert_eq!(
            WorkflowPhase::Requirements.next(),
            Some(WorkflowPhase::Analysis)
        );
        assert_eq!(WorkflowPhase::Analysis.next(), Some(WorkflowPhase::Design));
        assert_eq!(
            WorkflowPhase::Design.next(),
            Some(WorkflowPhase::Implementation)
        );
        assert_eq!(
            WorkflowPhase::Implementation.next(),
            Some(WorkflowPhase::Verification)
        );
        assert_eq!(
            WorkflowPhase::Verification.next(),
            Some(WorkflowPhase::Deployment)
        );
        assert_eq!(WorkflowPhase::Deployment.next(), None);
    }
}
