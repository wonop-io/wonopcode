//! Shared types for the ACE framework integration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Artifact types supported at ticket level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactType {
    /// Session artifact - root of all artifacts in a workstream.
    /// Contains a changelog of decisions and discoveries.
    Session,
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
            ArtifactType::Session => "SESSION",
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
            ArtifactType::Session => "sessions",
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
            "session" => Some(ArtifactType::Session),
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
            "SESSION" => Some(ArtifactType::Session),
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
            ArtifactType::Session => &[], // Session is the root, no parents
            ArtifactType::UseCase => &[ArtifactType::Session], // UC parents to Session
            ArtifactType::Requirement => &[ArtifactType::UseCase],
            ArtifactType::Design => &[ArtifactType::Requirement],
            ArtifactType::TestCase => &[ArtifactType::Requirement],
            ArtifactType::Task => &[
                ArtifactType::Session, // Tasks can parent directly to Session (for todowrite)
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

/// Importance levels for session log entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLogImportance {
    /// 🔴 Important - key decisions, critical info
    Important,
    /// 🟡 Maybe important - worth noting but not critical
    MaybeImportant,
    /// 🟢 Info only - general observations
    InfoOnly,
}

impl SessionLogImportance {
    /// Get the emoji for this importance level.
    pub fn emoji(&self) -> &'static str {
        match self {
            SessionLogImportance::Important => "🔴",
            SessionLogImportance::MaybeImportant => "🟡",
            SessionLogImportance::InfoOnly => "🟢",
        }
    }

    /// Parse from string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "important" | "high" | "critical" => Some(SessionLogImportance::Important),
            "maybe_important" | "maybe" | "medium" => Some(SessionLogImportance::MaybeImportant),
            "info_only" | "info" | "low" => Some(SessionLogImportance::InfoOnly),
            _ => None,
        }
    }
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
    /// Optional phase for grouping tasks in the plan view.
    /// When set, tasks are grouped by phase in the implementation plan.
    /// For backwards compatibility, this is optional - legacy tasks without a phase
    /// are placed in an "Unphased" group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
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

impl Artifact {
    /// Extract the ticket ID from this artifact's ID.
    ///
    /// Artifact IDs follow the pattern `{TYPE}-{TICKET_ID}-{SEQUENCE}` where:
    /// - TYPE: UC, REQ, DES, TC, TASK
    /// - TICKET_ID: e.g., WON-125, PROJ-42, or just 413
    /// - SEQUENCE: 3-digit number (001, 002, etc.)
    ///
    /// Examples:
    /// - `TASK-WON-157-006` → `WON-157`
    /// - `UC-413-001` → `413`
    /// - `REQ-WON-123-001` → `WON-123`
    pub fn ticket_id(&self) -> Option<String> {
        extract_ticket_id_from_artifact_id(&self.metadata.id)
    }

    /// Check if this artifact belongs to the given ticket.
    pub fn belongs_to_ticket(&self, ticket_id: &str) -> bool {
        self.ticket_id()
            .map(|id| id.eq_ignore_ascii_case(ticket_id))
            .unwrap_or(false)
    }
}

/// Extract the ticket ID from an artifact ID string.
///
/// Artifact IDs follow the pattern `{TYPE}-{TICKET_ID}-{SEQUENCE}` where:
/// - TYPE: UC, REQ, DES, TC, TASK
/// - TICKET_ID: e.g., WON-125, PROJ-42, or just 413
/// - SEQUENCE: 3-digit number (001, 002, etc.)
///
/// Examples:
/// - `TASK-WON-157-006` → `WON-157`
/// - `UC-413-001` → `413`
/// - `REQ-WON-123-001` → `WON-123`
pub fn extract_ticket_id_from_artifact_id(artifact_id: &str) -> Option<String> {
    // The artifact ID format is: {PREFIX}-{TICKET_ID}-{3-DIGIT-SEQUENCE}
    // We need to:
    // 1. Remove the 3-digit sequence at the end (and its preceding dash)
    // 2. Remove the prefix (UC, REQ, DES, TC, TASK)
    // 3. What remains is the ticket ID

    // Must have at least PREFIX-X-NNN (minimum 8 chars: "UC-1-001")
    if artifact_id.len() < 8 {
        return None;
    }

    // Check that it ends with a 3-digit sequence
    let parts: Vec<&str> = artifact_id.rsplitn(2, '-').collect();
    if parts.len() != 2 {
        return None;
    }

    let sequence = parts[0];
    if sequence.len() != 3 || !sequence.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let without_sequence = parts[1]; // e.g., "TASK-WON-157" or "UC-413"

    // Now strip the prefix
    let prefixes = ["SESSION-", "TASK-", "UC-", "REQ-", "DES-", "TC-"];
    for prefix in prefixes {
        if let Some(ticket_id) = without_sequence.strip_prefix(prefix) {
            if !ticket_id.is_empty() {
                return Some(ticket_id.to_string());
            }
        }
    }

    None
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
        // Session is root, has no parents
        assert!(ArtifactType::Session.valid_parent_types().is_empty());
        // UseCase requires Session
        assert_eq!(
            ArtifactType::UseCase.valid_parent_types(),
            &[ArtifactType::Session]
        );
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
        // Tasks can parent to Session (for ad-hoc todos) or to other artifacts
        assert_eq!(
            ArtifactType::Task.valid_parent_types(),
            &[
                ArtifactType::Session,
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

    #[test]
    fn test_extract_ticket_id_from_artifact_id() {
        // Standard project-number format
        assert_eq!(
            extract_ticket_id_from_artifact_id("TASK-WON-157-006"),
            Some("WON-157".to_string())
        );
        assert_eq!(
            extract_ticket_id_from_artifact_id("UC-WON-123-001"),
            Some("WON-123".to_string())
        );
        assert_eq!(
            extract_ticket_id_from_artifact_id("REQ-WON-123-002"),
            Some("WON-123".to_string())
        );
        assert_eq!(
            extract_ticket_id_from_artifact_id("DES-PROJ-42-001"),
            Some("PROJ-42".to_string())
        );
        assert_eq!(
            extract_ticket_id_from_artifact_id("TC-ABC-999-003"),
            Some("ABC-999".to_string())
        );

        // Numeric-only ticket ID
        assert_eq!(
            extract_ticket_id_from_artifact_id("UC-413-001"),
            Some("413".to_string())
        );
        assert_eq!(
            extract_ticket_id_from_artifact_id("TASK-413-005"),
            Some("413".to_string())
        );

        // Invalid formats
        assert_eq!(extract_ticket_id_from_artifact_id("invalid"), None);
        assert_eq!(extract_ticket_id_from_artifact_id("UC-123"), None); // No sequence
        assert_eq!(extract_ticket_id_from_artifact_id("TASK--001"), None); // Empty ticket ID
        assert_eq!(extract_ticket_id_from_artifact_id("XX-WON-123-001"), None); // Invalid prefix
    }
}
