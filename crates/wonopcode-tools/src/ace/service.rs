//! ACE service implementation for the TypeScript runtime.
//!
//! This provides an `AceService` implementation that wraps the file-based
//! ACE workflow tools, making them accessible from within TypeScript code.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use wonopcode_codemode::{
    AceService, Artifact, ArtifactPriority, ArtifactStatus, ArtifactType, CheckpointAction,
    CheckpointResult, CheckpointType, NewArtifact, NewTask, ServiceError, ServiceResult, TaskEntry,
    TaskTree, WorkflowGuidance,
};

use super::config::WonopCodeConfig;
use super::state::WorkstreamState;
use super::store::ArtifactStore;
use super::types::{Priority, Progress};

/// Shared ACE service type.
pub type SharedAceService = Arc<dyn AceService>;

/// File-based ACE service implementation.
///
/// This implementation reads and writes ACE artifacts to the filesystem,
/// using the same underlying logic as the standalone ACE tools.
pub struct FileAceService {
    /// Project root directory.
    root_dir: PathBuf,
}

impl FileAceService {
    /// Create a new file-based ACE service.
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    /// Create a shared ACE service.
    pub fn shared(root_dir: PathBuf) -> SharedAceService {
        Arc::new(Self::new(root_dir))
    }

    fn to_service_error(msg: impl Into<String>) -> ServiceError {
        ServiceError::new("ACE_ERROR", msg)
    }

    fn convert_artifact(&self, artifact: super::types::Artifact, store: &ArtifactStore) -> Artifact {
        let staging = store.is_staged(&artifact);
        Artifact {
            id: artifact.metadata.id,
            artifact_type: self.convert_type(&artifact.metadata.artifact_type),
            title: artifact.title,
            content: artifact.content,
            parents: artifact.metadata.parents,
            priority: self.convert_priority(&artifact.metadata.priority),
            status: Some(self.convert_status(&artifact.metadata.progress)),
            staging,
        }
    }

    fn convert_type(&self, t: &super::types::ArtifactType) -> ArtifactType {
        match t {
            super::types::ArtifactType::UseCase => ArtifactType::UseCase,
            super::types::ArtifactType::Requirement => ArtifactType::Requirement,
            super::types::ArtifactType::Design => ArtifactType::Design,
            super::types::ArtifactType::TestCase => ArtifactType::TestCase,
            super::types::ArtifactType::Task => ArtifactType::Task,
            // Session artifacts are internal; map to UseCase for external API
            super::types::ArtifactType::Session => ArtifactType::UseCase,
        }
    }

    fn convert_status(&self, p: &Progress) -> ArtifactStatus {
        match p {
            Progress::Backlog => ArtifactStatus::Backlog,
            Progress::InProgress => ArtifactStatus::InProgress,
            Progress::Blocked => ArtifactStatus::Blocked,
            Progress::Parked => ArtifactStatus::Parked,
            Progress::ReadyToValidate => ArtifactStatus::ReadyToValidate,
            Progress::Done => ArtifactStatus::Done,
            Progress::Discarded => ArtifactStatus::Discarded,
        }
    }

    fn convert_priority(&self, p: &Priority) -> ArtifactPriority {
        match p {
            Priority::High => ArtifactPriority::High,
            Priority::Medium => ArtifactPriority::Medium,
            Priority::Low => ArtifactPriority::Low,
        }
    }

    fn parse_type(&self, t: &ArtifactType) -> super::types::ArtifactType {
        match t {
            ArtifactType::UseCase => super::types::ArtifactType::UseCase,
            ArtifactType::Requirement => super::types::ArtifactType::Requirement,
            ArtifactType::Design => super::types::ArtifactType::Design,
            ArtifactType::TestCase => super::types::ArtifactType::TestCase,
            ArtifactType::Task => super::types::ArtifactType::Task,
        }
    }

    fn parse_status(&self, s: &ArtifactStatus) -> Progress {
        match s {
            ArtifactStatus::Backlog => Progress::Backlog,
            ArtifactStatus::InProgress => Progress::InProgress,
            ArtifactStatus::Blocked => Progress::Blocked,
            ArtifactStatus::Parked => Progress::Parked,
            ArtifactStatus::ReadyToValidate => Progress::ReadyToValidate,
            ArtifactStatus::Done => Progress::Done,
            ArtifactStatus::Discarded => Progress::Discarded,
        }
    }

    fn parse_priority(&self, p: &Option<ArtifactPriority>) -> Priority {
        match p {
            Some(ArtifactPriority::High) => Priority::High,
            Some(ArtifactPriority::Low) => Priority::Low,
            _ => Priority::Medium,
        }
    }

    fn artifact_to_task_entry(&self, artifact: &super::types::Artifact) -> TaskEntry {
        TaskEntry {
            id: artifact.metadata.id.clone(),
            content: artifact.title.clone(),
            phase: artifact.metadata.phase.clone().unwrap_or_default(),
            parent: artifact.metadata.parents.first().cloned(),
            priority: self.convert_priority(&artifact.metadata.priority),
            status: self.convert_status(&artifact.metadata.progress),
        }
    }
}

#[async_trait]
impl AceService for FileAceService {
    async fn create_artifact(&self, artifact: NewArtifact) -> ServiceResult<Artifact> {
        let store = ArtifactStore::new(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to create store: {}", e)))?;

        let mut state = WorkstreamState::ensure_initialized(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to load state: {}", e)))?;

        let artifact_type = self.parse_type(&artifact.artifact_type);
        let priority = self.parse_priority(&artifact.priority);

        let created = store
            .create_artifact(
                &mut state,
                artifact_type,
                &artifact.title,
                &artifact.content,
                artifact.parents.unwrap_or_default(),
                priority,
                artifact.staging.unwrap_or(true),
            )
            .map_err(|e| Self::to_service_error(format!("Failed to create artifact: {}", e)))?;

        // Save updated state
        state
            .save(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to save state: {}", e)))?;

        Ok(self.convert_artifact(created, &store))
    }

    async fn read_artifact(&self, id: &str) -> ServiceResult<Artifact> {
        let store = ArtifactStore::new(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to create store: {}", e)))?;

        let artifact = store
            .read_artifact(id)
            .map_err(|e| Self::to_service_error(format!("Failed to read artifact: {}", e)))?
            .ok_or_else(|| ServiceError::new("NOT_FOUND", format!("Artifact not found: {}", id)))?;

        Ok(self.convert_artifact(artifact, &store))
    }

    async fn submit_checkpoint(
        &self,
        checkpoint: CheckpointType,
        action: CheckpointAction,
        comments: Option<String>,
    ) -> ServiceResult<CheckpointResult> {
        let mut state = WorkstreamState::ensure_initialized(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to load state: {}", e)))?;

        let checkpoint_name = match &checkpoint {
            CheckpointType::Requirements => "requirements",
            CheckpointType::Design => "design",
            CheckpointType::ImplementationPlan => "implementation_plan",
            CheckpointType::Verification => "verification",
        };

        match action {
            CheckpointAction::RequestReview => {
                // Just return info about the checkpoint
                Ok(CheckpointResult {
                    checkpoint,
                    status: "pending_review".to_string(),
                    message: comments.or_else(|| {
                        Some(format!("Review requested for {} checkpoint", checkpoint_name))
                    }),
                })
            }
            CheckpointAction::Approve => {
                // Mark phase as completed by advancing workflow phase
                // The phase state is stored in state.workflow.phases
                let phase_key = checkpoint_name.to_string();
                if let Some(phase) = state.workflow.phases.get_mut(&phase_key) {
                    phase.status = super::types::PhaseStatus::Completed;
                    phase.completed_at = Some(chrono::Utc::now());
                }

                state
                    .save(&self.root_dir)
                    .map_err(|e| Self::to_service_error(format!("Failed to save state: {}", e)))?;

                Ok(CheckpointResult {
                    checkpoint,
                    status: "approved".to_string(),
                    message: comments
                        .or_else(|| Some(format!("{} checkpoint approved", checkpoint_name))),
                })
            }
        }
    }

    async fn update_task_status(
        &self,
        task_id: &str,
        status: ArtifactStatus,
    ) -> ServiceResult<TaskEntry> {
        let store = ArtifactStore::new(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to create store: {}", e)))?;

        let progress = self.parse_status(&status);

        let artifact = store
            .update_artifact_progress(task_id, progress)
            .map_err(|e| Self::to_service_error(format!("Failed to update task: {}", e)))?;

        Ok(self.artifact_to_task_entry(&artifact))
    }

    async fn what_now(&self) -> ServiceResult<WorkflowGuidance> {
        let _config = WonopCodeConfig::load(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to load config: {}", e)))?;

        let state = WorkstreamState::ensure_initialized(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to load state: {}", e)))?;

        let store = ArtifactStore::new(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to create store: {}", e)))?;

        // Get active task if any
        let active_task = if let Some(ref task_id) = state.active_task {
            store
                .read_artifact(task_id)
                .ok()
                .flatten()
                .map(|a| self.artifact_to_task_entry(&a))
        } else {
            None
        };

        // Determine pending checkpoints based on workflow phase
        let pending_checkpoints = vec![]; // TODO: Calculate from state

        let phase = format!("{:?}", state.workflow.current_phase);
        let next_steps = if active_task.is_some() {
            vec!["Continue working on the active task.".to_string()]
        } else {
            vec![
                "Pick up a task from the backlog.".to_string(),
                "Review pending checkpoints.".to_string(),
            ]
        };

        Ok(WorkflowGuidance {
            current_phase: phase,
            next_steps,
            active_task,
            pending_checkpoints,
        })
    }

    async fn create_tasks(&self, tasks: Vec<NewTask>) -> ServiceResult<Vec<TaskEntry>> {
        let store = ArtifactStore::new(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to create store: {}", e)))?;

        let mut state = WorkstreamState::ensure_initialized(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to load state: {}", e)))?;

        let mut created = Vec::new();
        for task in tasks {
            let priority = self.parse_priority(&task.priority);
            let status = task
                .status
                .as_ref()
                .map(|s| self.parse_status(s))
                .unwrap_or(Progress::Backlog);

            // Use parent if specified
            let parents = task.parent.clone().map(|p| vec![p]).unwrap_or_default();

            let mut artifact = store
                .create_artifact_with_phase(
                    &mut state,
                    super::types::ArtifactType::Task,
                    &task.content,
                    &format!("Phase: {}", task.phase),
                    parents,
                    priority,
                    false, // Not staging
                    Some(task.phase.clone()),
                )
                .map_err(|e| Self::to_service_error(format!("Failed to create task: {}", e)))?;

            // Update status if not backlog
            if status != Progress::Backlog {
                artifact = store
                    .update_artifact_progress(&artifact.metadata.id, status)
                    .map_err(|e| Self::to_service_error(format!("Failed to update task: {}", e)))?;
            }

            // Set the phase in the task entry
            created.push(TaskEntry {
                id: artifact.metadata.id,
                content: artifact.title,
                phase: task.phase,
                parent: task.parent,
                priority: self.convert_priority(&artifact.metadata.priority),
                status: self.convert_status(&artifact.metadata.progress),
            });
        }

        // Save updated state
        state
            .save(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to save state: {}", e)))?;

        Ok(created)
    }

    async fn read_tasks(&self) -> ServiceResult<TaskTree> {
        let store = ArtifactStore::new(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to create store: {}", e)))?;

        let state = WorkstreamState::ensure_initialized(&self.root_dir)
            .map_err(|e| Self::to_service_error(format!("Failed to load state: {}", e)))?;

        let artifacts = store
            .list_all_artifacts_for_ticket(&state.ticket_id)
            .map_err(|e| Self::to_service_error(format!("Failed to list artifacts: {}", e)))?;

        let tasks: Vec<super::types::Artifact> = artifacts
            .into_iter()
            .filter(|a| matches!(a.metadata.artifact_type, super::types::ArtifactType::Task))
            .collect();

        // Group tasks by status
        let mut done = Vec::new();
        let mut in_progress = Vec::new();
        let mut backlog = Vec::new();
        let mut blocked = Vec::new();
        let mut discarded = Vec::new();

        for task in &tasks {
            let entry = self.artifact_to_task_entry(task);
            match task.metadata.progress {
                Progress::Done | Progress::ReadyToValidate => done.push(entry),
                Progress::InProgress => in_progress.push(entry),
                Progress::Backlog => backlog.push(entry),
                Progress::Blocked | Progress::Parked => blocked.push(entry),
                Progress::Discarded => discarded.push(entry),
            }
        }

        Ok(TaskTree {
            ticket_id: Some(state.ticket_id),
            phase: None,
            done,
            in_progress,
            backlog,
            blocked,
            discarded,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        // Create minimal structure
        std::fs::create_dir_all(dir.path().join(".wonopcode")).unwrap();
        std::fs::create_dir_all(dir.path().join("specs")).unwrap();
        dir
    }

    #[tokio::test]
    async fn test_what_now_initializes_state() {
        let dir = setup_test_dir();
        let service = FileAceService::new(dir.path().to_path_buf());

        let result = service.what_now().await;
        // Should initialize state and return guidance
        assert!(result.is_ok());
    }
}
