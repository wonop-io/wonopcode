//! Workstream state management.
//!
//! Manages the workflow state stored in `.wonopcode/state.yaml`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::types::{PhaseStatus, WorkflowPhase};

/// Workstream-specific state stored in .wonopcode/state.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstreamState {
    /// Ticket ID this workstream is associated with.
    pub ticket_id: String,
    /// Ticket title (optional).
    #[serde(default)]
    pub ticket_title: Option<String>,
    /// Source of the ticket (linear, github_issues, etc.).
    #[serde(default)]
    pub ticket_source: Option<String>,
    /// Tracker ID that owns this ticket.
    /// This is discovered automatically when the workstream is first used
    /// and cached for subsequent operations.
    #[serde(default)]
    pub ticket_tracker_id: Option<String>,
    /// Session artifact ID for this workstream.
    /// Auto-created when the workstream is initialized.
    #[serde(default)]
    pub session_id: Option<String>,
    /// When this workstream was created.
    pub created_at: DateTime<Utc>,
    /// When this workstream was last updated.
    pub updated_at: DateTime<Utc>,
    /// Workflow state.
    pub workflow: WorkflowState,
    /// Currently active task ID.
    #[serde(default)]
    pub active_task: Option<String>,
    /// Sequence counters for artifact ID generation.
    #[serde(default)]
    pub sequences: SequenceCounters,
}

/// Workflow state tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    /// Current workflow phase.
    pub current_phase: WorkflowPhase,
    /// Status of each phase.
    pub phases: HashMap<String, PhaseState>,
}

/// State of a single workflow phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseState {
    /// Current status of the phase.
    pub status: PhaseStatus,
    /// When the phase was started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    /// When the phase was completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl Default for PhaseState {
    fn default() -> Self {
        Self {
            status: PhaseStatus::Pending,
            started_at: None,
            completed_at: None,
        }
    }
}

/// Sequence counters for generating artifact IDs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SequenceCounters {
    /// Session sequence counter.
    #[serde(default)]
    pub session: u32,
    /// Use case sequence counter.
    #[serde(rename = "use-case", default)]
    pub use_case: u32,
    /// Requirement sequence counter.
    #[serde(default)]
    pub requirement: u32,
    /// Design sequence counter.
    #[serde(default)]
    pub design: u32,
    /// Test case sequence counter.
    #[serde(rename = "test-case", default)]
    pub test_case: u32,
    /// Task sequence counter.
    #[serde(default)]
    pub task: u32,
}

impl WorkstreamState {
    /// Create a new workstream state for a ticket.
    pub fn new(ticket_id: &str) -> Self {
        let now = Utc::now();
        let mut phases = HashMap::new();

        // Initialize all phases - requirements starts as in_progress
        for phase in WorkflowPhase::all() {
            let status = if *phase == WorkflowPhase::Requirements {
                PhaseStatus::InProgress
            } else {
                PhaseStatus::Pending
            };
            phases.insert(
                phase.as_str().to_string(),
                PhaseState {
                    status,
                    started_at: if *phase == WorkflowPhase::Requirements {
                        Some(now)
                    } else {
                        None
                    },
                    completed_at: None,
                },
            );
        }

        Self {
            ticket_id: ticket_id.to_string(),
            ticket_title: None,
            ticket_source: None,
            ticket_tracker_id: None,
            session_id: None, // Session is created lazily when first needed
            created_at: now,
            updated_at: now,
            workflow: WorkflowState {
                current_phase: WorkflowPhase::Requirements,
                phases,
            },
            active_task: None,
            sequences: SequenceCounters::default(),
        }
    }

    /// Create a new workstream state with a known tracker.
    pub fn new_with_tracker(ticket_id: &str, tracker_id: &str) -> Self {
        let mut state = Self::new(ticket_id);
        state.ticket_tracker_id = Some(tracker_id.to_string());
        state
    }

    /// Set the tracker ID for this workstream.
    ///
    /// This is typically called when the tracker is discovered automatically
    /// by searching for the ticket across all configured trackers.
    pub fn set_tracker_id(&mut self, tracker_id: String) {
        self.ticket_tracker_id = Some(tracker_id);
    }

    /// Get the tracker ID if known.
    pub fn tracker_id(&self) -> Option<&str> {
        self.ticket_tracker_id.as_deref()
    }

    /// Load state from .wonopcode/state.yaml.
    ///
    /// If the state file doesn't exist but we're in a worktree with a ticket-style
    /// name (e.g., `feature-won-125--description`), auto-create the state.
    ///
    /// Returns None if no state file exists AND we can't infer a ticket ID.
    pub fn load(root_dir: &Path) -> Result<Option<Self>> {
        tracing::debug!(
            "🔄 WorkstreamState::load: Looking for state in root_dir={}",
            root_dir.display()
        );

        let state_path = root_dir.join(".wonopcode").join("state.yaml");
        tracing::debug!("🔄 WorkstreamState::load: state_path={}", state_path.display());

        if state_path.exists() {
            tracing::info!("🔄 WorkstreamState::load: Found existing state file");
            let content = std::fs::read_to_string(&state_path)
                .with_context(|| format!("Failed to read {}", state_path.display()))?;

            let state: Self = serde_yaml::from_str(&content)
                .with_context(|| format!("Failed to parse {}", state_path.display()))?;

            tracing::info!(
                "🔄 WorkstreamState::load: Loaded state - ticket_id={}, active_task={:?}",
                state.ticket_id,
                state.active_task
            );
            return Ok(Some(state));
        }

        tracing::debug!("🔄 WorkstreamState::load: No state file exists, trying to infer from directory name");

        // Try to infer ticket ID from worktree directory name
        // Pattern: feature-{TICKET_ID}--{description} or {prefix}-{TICKET_ID}-{suffix}
        if let Some(dir_name) = root_dir.file_name().and_then(|n| n.to_str()) {
            tracing::debug!("🔄 WorkstreamState::load: Directory name: '{}'", dir_name);
            if let Some(ticket_id) = Self::extract_ticket_id(dir_name) {
                tracing::info!(
                    "🔄 WorkstreamState::load: Inferred ticket_id='{}' from directory name",
                    ticket_id
                );
                // Auto-create state for this workstream
                let mut state = Self::new(&ticket_id);
                state.ticket_title = Self::extract_title(dir_name);

                // Save the state so it persists
                tracing::debug!("🔄 WorkstreamState::load: Saving auto-created state");
                state.save(root_dir)?;

                return Ok(Some(state));
            } else {
                tracing::debug!(
                    "🔄 WorkstreamState::load: Could not extract ticket ID from '{}'",
                    dir_name
                );
            }
        }

        tracing::debug!("🔄 WorkstreamState::load: No state could be loaded or inferred");
        Ok(None)
    }

    /// Ensure a workstream is initialized, creating one if necessary.
    ///
    /// This is called by ACE tools to auto-initialize the workstream when needed.
    /// Unlike `load()`, this will always return a state - creating a default one
    /// if no state exists and no ticket ID can be inferred from the directory name.
    ///
    /// The default ticket ID is generated from the directory name using a short hash.
    pub fn ensure_initialized(root_dir: &Path) -> Result<Self> {
        tracing::info!(
            "🔄 WorkstreamState::ensure_initialized: root_dir={}",
            root_dir.display()
        );

        // First try normal load (which may auto-create from directory name)
        if let Some(state) = Self::load(root_dir)? {
            tracing::info!(
                "🔄 WorkstreamState::ensure_initialized: Loaded existing state - ticket_id={}",
                state.ticket_id
            );
            return Ok(state);
        }

        // No state exists and couldn't infer ticket ID - create a default workstream
        let ticket_id = Self::generate_default_ticket_id(root_dir);
        tracing::info!(
            "🔄 WorkstreamState::ensure_initialized: Creating new state with generated ticket_id={}",
            ticket_id
        );

        let mut state = Self::new(&ticket_id);

        // Try to extract a title from the directory name
        if let Some(dir_name) = root_dir.file_name().and_then(|n| n.to_str()) {
            state.ticket_title = Some(Self::humanize_directory_name(dir_name));
            tracing::debug!(
                "🔄 WorkstreamState::ensure_initialized: Set ticket_title={:?}",
                state.ticket_title
            );
        }

        // Save the state so it persists
        tracing::debug!("🔄 WorkstreamState::ensure_initialized: Saving new state");
        state.save(root_dir)?;

        tracing::info!(
            "🔄 WorkstreamState::ensure_initialized: State created and saved - ticket_id={}",
            state.ticket_id
        );

        Ok(state)
    }

    /// Generate a default ticket ID from the directory path.
    ///
    /// Creates an ID like "WS-abc123" where abc123 is a short hash of the directory name.
    fn generate_default_ticket_id(root_dir: &Path) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let dir_name = root_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace");

        let mut hasher = DefaultHasher::new();
        dir_name.hash(&mut hasher);
        let hash = hasher.finish();

        // Use first 6 hex chars of hash for a short but unique ID
        format!("WS-{:06x}", hash & 0xFFFFFF)
    }

    /// Convert a directory name to a human-readable title.
    ///
    /// Examples:
    /// - "my-project" -> "My Project"
    /// - "some_feature_branch" -> "Some Feature Branch"
    fn humanize_directory_name(dir_name: &str) -> String {
        dir_name
            .split(|c| c == '-' || c == '_')
            .filter(|s| !s.is_empty())
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Extract ticket ID from a worktree directory name.
    ///
    /// Supports patterns like:
    /// - `feature-WON-125--description` → `WON-125`
    /// - `feature/won-125--description` → `WON-125`
    /// - `bugfix-PROJ-42--fix-thing` → `PROJ-42`
    /// - `feature-409--description` → `409` (numeric-only ticket ID)
    /// - `409--description` → `409` (numeric-only ticket ID)
    fn extract_ticket_id(dir_name: &str) -> Option<String> {
        // Common prefixes to strip
        let prefixes = [
            "feature-", "feature/", "bugfix-", "bugfix/", "fix-", "fix/", "hotfix-", "hotfix/",
        ];

        let mut name = dir_name;
        for prefix in &prefixes {
            if let Some(stripped) = name.strip_prefix(prefix) {
                name = stripped;
                break;
            }
        }

        // Try to match ticket patterns in order of specificity:
        // 1. {PROJECT}-{NUMBER} (e.g., WON-125, PROJ-42)
        // 2. {NUMBER} only (e.g., 409) - for numeric-only ticket IDs
        use std::sync::OnceLock;

        // Pattern 1: Letters/numbers followed by dash and digits (e.g., WON-125)
        static RE_PROJECT: OnceLock<Option<regex::Regex>> = OnceLock::new();
        let re_project = RE_PROJECT.get_or_init(|| {
            regex::Regex::new(r"^([A-Za-z][A-Za-z0-9]*-\d+)").ok()
        });

        if let Some(re) = re_project {
            if let Some(captures) = re.captures(name) {
                if let Some(m) = captures.get(1) {
                    return Some(m.as_str().to_uppercase());
                }
            }
        }

        // Pattern 2: Numeric-only ticket ID (e.g., 409)
        // Only matches when followed by -- (the description separator) to avoid
        // ambiguity with branch names like "feature-123" which may not be ticket IDs
        static RE_NUMERIC: OnceLock<Option<regex::Regex>> = OnceLock::new();
        let re_numeric = RE_NUMERIC.get_or_init(|| {
            regex::Regex::new(r"^(\d+)--").ok()
        });

        if let Some(re) = re_numeric {
            if let Some(captures) = re.captures(name) {
                if let Some(m) = captures.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }

        None
    }

    /// Extract a human-readable title from the directory name.
    fn extract_title(dir_name: &str) -> Option<String> {
        // Look for the part after -- which is typically the description
        if let Some(idx) = dir_name.find("--") {
            let desc = &dir_name[idx + 2..];
            if !desc.is_empty() {
                // Convert kebab-case to Title Case
                let title = desc
                    .split('-')
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => first.to_uppercase().chain(chars).collect(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                return Some(title);
            }
        }
        None
    }

    /// Save state to .wonopcode/state.yaml.
    pub fn save(&mut self, root_dir: &Path) -> Result<()> {
        self.updated_at = Utc::now();

        let wonopcode_dir = root_dir.join(".wonopcode");
        std::fs::create_dir_all(&wonopcode_dir)?;

        let state_path = wonopcode_dir.join("state.yaml");
        let content = serde_yaml::to_string(self)?;
        std::fs::write(&state_path, content)?;

        Ok(())
    }

    /// Get the next sequence number for an artifact type.
    ///
    /// This increments the counter and returns the new value.
    pub fn next_sequence(&mut self, artifact_type: &str) -> u32 {
        let counter = match artifact_type {
            "session" | "sessions" => &mut self.sequences.session,
            "use-case" | "use-cases" => &mut self.sequences.use_case,
            "requirement" | "requirements" => &mut self.sequences.requirement,
            "design" | "designs" => &mut self.sequences.design,
            "test-case" | "tests" => &mut self.sequences.test_case,
            "task" | "tasks" => &mut self.sequences.task,
            _ => return 1,
        };
        *counter += 1;
        *counter
    }

    /// Get the current phase status.
    pub fn current_phase_status(&self) -> PhaseStatus {
        self.workflow
            .phases
            .get(self.workflow.current_phase.as_str())
            .map(|p| p.status)
            .unwrap_or(PhaseStatus::Pending)
    }

    /// Advance to the next workflow phase.
    ///
    /// Returns the new phase, or None if already at the final phase.
    pub fn advance_phase(&mut self) -> Option<WorkflowPhase> {
        let now = Utc::now();

        // Mark current phase as completed
        if let Some(current_state) = self
            .workflow
            .phases
            .get_mut(self.workflow.current_phase.as_str())
        {
            current_state.status = PhaseStatus::Completed;
            current_state.completed_at = Some(now);
        }

        // Advance to next phase
        if let Some(next_phase) = self.workflow.current_phase.next() {
            self.workflow.current_phase = next_phase;

            // Start the new phase
            if let Some(next_state) = self.workflow.phases.get_mut(next_phase.as_str()) {
                next_state.status = PhaseStatus::InProgress;
                next_state.started_at = Some(now);
            }

            Some(next_phase)
        } else {
            None
        }
    }

    /// Set phase to awaiting approval.
    pub fn set_awaiting_approval(&mut self, phase: &str) {
        if let Some(state) = self.workflow.phases.get_mut(phase) {
            state.status = PhaseStatus::AwaitingApproval;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_new_workstream_state() {
        let state = WorkstreamState::new("WON-123");

        assert_eq!(state.ticket_id, "WON-123");
        assert_eq!(state.workflow.current_phase, WorkflowPhase::Requirements);
        assert!(state.active_task.is_none());
        assert_eq!(state.sequences.use_case, 0);
        assert!(state.ticket_tracker_id.is_none());

        // Requirements should be in_progress
        let req_status = state.workflow.phases.get("requirements").unwrap().status;
        assert_eq!(req_status, PhaseStatus::InProgress);

        // Other phases should be pending
        let design_status = state.workflow.phases.get("design").unwrap().status;
        assert_eq!(design_status, PhaseStatus::Pending);
    }

    #[test]
    fn test_new_with_tracker() {
        let state = WorkstreamState::new_with_tracker("WON-123", "linear-tracker-1");

        assert_eq!(state.ticket_id, "WON-123");
        assert_eq!(state.ticket_tracker_id, Some("linear-tracker-1".to_string()));
        assert_eq!(state.tracker_id(), Some("linear-tracker-1"));
    }

    #[test]
    fn test_set_tracker_id() {
        let mut state = WorkstreamState::new("WON-456");
        assert!(state.tracker_id().is_none());

        state.set_tracker_id("github-tracker-2".to_string());
        assert_eq!(state.tracker_id(), Some("github-tracker-2"));
    }

    #[test]
    fn test_next_sequence() {
        let mut state = WorkstreamState::new("WON-123");

        assert_eq!(state.next_sequence("use-case"), 1);
        assert_eq!(state.next_sequence("use-case"), 2);
        assert_eq!(state.next_sequence("requirement"), 1);
        assert_eq!(state.next_sequence("task"), 1);
        assert_eq!(state.next_sequence("task"), 2);
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let mut state = WorkstreamState::new("WON-456");
        state.ticket_title = Some("Test ticket".to_string());
        state.next_sequence("use-case");
        state.next_sequence("use-case");

        state.save(dir.path()).unwrap();

        let loaded = WorkstreamState::load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.ticket_id, "WON-456");
        assert_eq!(loaded.ticket_title, Some("Test ticket".to_string()));
        assert_eq!(loaded.sequences.use_case, 2);
    }

    #[test]
    fn test_save_and_load_with_tracker() {
        let dir = tempdir().unwrap();
        let mut state = WorkstreamState::new_with_tracker("WON-789", "linear-main");
        state.ticket_title = Some("Feature with tracker".to_string());

        state.save(dir.path()).unwrap();

        let loaded = WorkstreamState::load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.ticket_id, "WON-789");
        assert_eq!(loaded.tracker_id(), Some("linear-main"));
        assert_eq!(loaded.ticket_title, Some("Feature with tracker".to_string()));
    }

    #[test]
    fn test_load_legacy_state_without_tracker_id() {
        // Test backward compatibility: loading a state.yaml that doesn't have ticket_tracker_id
        let dir = tempdir().unwrap();
        let wonopcode_dir = dir.path().join(".wonopcode");
        std::fs::create_dir_all(&wonopcode_dir).unwrap();

        // Write a legacy state.yaml without ticket_tracker_id
        let legacy_yaml = r#"
ticket_id: WON-LEGACY
ticket_title: Legacy Ticket
created_at: 2024-01-01T00:00:00Z
updated_at: 2024-01-01T00:00:00Z
workflow:
  current_phase: requirements
  phases:
    requirements:
      status: in_progress
    analysis:
      status: pending
    design:
      status: pending
    implementation:
      status: pending
    verification:
      status: pending
    deployment:
      status: pending
sequences:
  use-case: 0
  requirement: 0
  design: 0
  test-case: 0
  task: 0
"#;
        std::fs::write(wonopcode_dir.join("state.yaml"), legacy_yaml).unwrap();

        let loaded = WorkstreamState::load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.ticket_id, "WON-LEGACY");
        assert!(loaded.tracker_id().is_none()); // Should default to None
    }

    #[test]
    fn test_load_missing() {
        // A temp dir has a random name, so no ticket ID can be inferred
        let dir = tempdir().unwrap();
        let result = WorkstreamState::load(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_ticket_id() {
        // Standard feature branch format
        assert_eq!(
            WorkstreamState::extract_ticket_id("feature-WON-125--test-calculator"),
            Some("WON-125".to_string())
        );

        // With slash prefix
        assert_eq!(
            WorkstreamState::extract_ticket_id("feature/won-42--description"),
            Some("WON-42".to_string())
        );

        // Different project prefix
        assert_eq!(
            WorkstreamState::extract_ticket_id("bugfix-PROJ-999--fix-bug"),
            Some("PROJ-999".to_string())
        );

        // Without prefix
        assert_eq!(
            WorkstreamState::extract_ticket_id("ABC-123--some-feature"),
            Some("ABC-123".to_string())
        );

        // Numeric-only ticket ID with feature prefix
        assert_eq!(
            WorkstreamState::extract_ticket_id("feature-409--description"),
            Some("409".to_string())
        );

        // Numeric-only ticket ID with slash prefix
        assert_eq!(
            WorkstreamState::extract_ticket_id("feature/409--description"),
            Some("409".to_string())
        );

        // Numeric-only ticket ID without prefix
        assert_eq!(
            WorkstreamState::extract_ticket_id("409--some-feature"),
            Some("409".to_string())
        );

        // No ticket ID
        assert_eq!(
            WorkstreamState::extract_ticket_id("some-random-directory"),
            None
        );

        // Just numbers without description separator (ambiguous, returns None)
        assert_eq!(WorkstreamState::extract_ticket_id("feature-123"), None);
    }

    #[test]
    fn test_extract_title() {
        assert_eq!(
            WorkstreamState::extract_title("feature-WON-125--test-calculator"),
            Some("Test Calculator".to_string())
        );

        assert_eq!(
            WorkstreamState::extract_title("bugfix-PROJ-42--fix-memory-leak"),
            Some("Fix Memory Leak".to_string())
        );

        // No description after --
        assert_eq!(WorkstreamState::extract_title("feature-WON-125--"), None);

        // No -- separator
        assert_eq!(WorkstreamState::extract_title("feature-WON-125"), None);
    }

    #[test]
    fn test_auto_create_state_from_worktree_name() {
        // Create a temp dir with a ticket-style name
        let parent = tempdir().unwrap();
        let worktree_dir = parent.path().join("feature-WON-999--test-auto-create");
        std::fs::create_dir_all(&worktree_dir).unwrap();

        // Load should auto-create state
        let state = WorkstreamState::load(&worktree_dir).unwrap().unwrap();
        assert_eq!(state.ticket_id, "WON-999");
        assert_eq!(state.ticket_title, Some("Test Auto Create".to_string()));

        // State file should now exist
        let state_path = worktree_dir.join(".wonopcode").join("state.yaml");
        assert!(state_path.exists());
    }

    #[test]
    fn test_advance_phase() {
        let mut state = WorkstreamState::new("WON-789");

        assert_eq!(state.workflow.current_phase, WorkflowPhase::Requirements);

        let next = state.advance_phase();
        assert_eq!(next, Some(WorkflowPhase::Analysis));
        assert_eq!(state.workflow.current_phase, WorkflowPhase::Analysis);

        // Requirements should be completed
        let req_state = state.workflow.phases.get("requirements").unwrap();
        assert_eq!(req_state.status, PhaseStatus::Completed);
        assert!(req_state.completed_at.is_some());

        // Analysis should be in_progress
        let analysis_state = state.workflow.phases.get("analysis").unwrap();
        assert_eq!(analysis_state.status, PhaseStatus::InProgress);
        assert!(analysis_state.started_at.is_some());
    }

    #[test]
    fn test_advance_phase_final() {
        let mut state = WorkstreamState::new("WON-999");
        state.workflow.current_phase = WorkflowPhase::Deployment;

        let next = state.advance_phase();
        assert!(next.is_none());
        assert_eq!(state.workflow.current_phase, WorkflowPhase::Deployment);
    }

    #[test]
    fn test_ensure_initialized_with_existing_state() {
        let dir = tempdir().unwrap();

        // Create existing state
        let mut state = WorkstreamState::new("EXISTING-123");
        state.ticket_title = Some("Existing State".to_string());
        state.save(dir.path()).unwrap();

        // ensure_initialized should return existing state
        let loaded = WorkstreamState::ensure_initialized(dir.path()).unwrap();
        assert_eq!(loaded.ticket_id, "EXISTING-123");
        assert_eq!(loaded.ticket_title, Some("Existing State".to_string()));
    }

    #[test]
    fn test_ensure_initialized_with_ticket_directory() {
        // Create a temp dir with a ticket-style name
        let parent = tempdir().unwrap();
        let worktree_dir = parent.path().join("feature-WON-888--ensure-test");
        std::fs::create_dir_all(&worktree_dir).unwrap();

        // ensure_initialized should auto-create from directory name
        let state = WorkstreamState::ensure_initialized(&worktree_dir).unwrap();
        assert_eq!(state.ticket_id, "WON-888");
        assert_eq!(state.ticket_title, Some("Ensure Test".to_string()));

        // State file should exist
        let state_path = worktree_dir.join(".wonopcode").join("state.yaml");
        assert!(state_path.exists());
    }

    #[test]
    fn test_ensure_initialized_creates_default() {
        // A temp dir has a random name, so no ticket ID can be inferred
        let dir = tempdir().unwrap();

        // ensure_initialized should create a default workstream
        let state = WorkstreamState::ensure_initialized(dir.path()).unwrap();

        // Should have a WS- prefixed ID
        assert!(state.ticket_id.starts_with("WS-"));

        // Should have a title derived from the directory name
        assert!(state.ticket_title.is_some());

        // State file should exist
        let state_path = dir.path().join(".wonopcode").join("state.yaml");
        assert!(state_path.exists());
    }

    #[test]
    fn test_generate_default_ticket_id() {
        let parent = tempdir().unwrap();
        let dir1 = parent.path().join("my-project");
        let dir2 = parent.path().join("another-project");
        std::fs::create_dir_all(&dir1).unwrap();
        std::fs::create_dir_all(&dir2).unwrap();

        let id1 = WorkstreamState::generate_default_ticket_id(&dir1);
        let id2 = WorkstreamState::generate_default_ticket_id(&dir2);

        // Both should start with WS-
        assert!(id1.starts_with("WS-"));
        assert!(id2.starts_with("WS-"));

        // Different directories should have different IDs
        assert_ne!(id1, id2);

        // Same directory should produce same ID
        let id1_again = WorkstreamState::generate_default_ticket_id(&dir1);
        assert_eq!(id1, id1_again);
    }

    #[test]
    fn test_humanize_directory_name() {
        assert_eq!(
            WorkstreamState::humanize_directory_name("my-project"),
            "My Project"
        );
        assert_eq!(
            WorkstreamState::humanize_directory_name("some_feature_branch"),
            "Some Feature Branch"
        );
        assert_eq!(
            WorkstreamState::humanize_directory_name("mixed-style_name"),
            "Mixed Style Name"
        );
        assert_eq!(
            WorkstreamState::humanize_directory_name("simple"),
            "Simple"
        );
        assert_eq!(
            WorkstreamState::humanize_directory_name("a-b-c"),
            "A B C"
        );
    }
}
