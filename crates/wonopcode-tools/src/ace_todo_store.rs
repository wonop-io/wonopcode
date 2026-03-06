//! ACE-backed todo storage.
//!
//! This module provides a TodoStore implementation that uses ACE Task artifacts
//! as the storage backend. Tasks are parented to the session artifact and grouped
//! by phase.

use std::collections::HashMap;
use std::path::Path;

use crate::ace::{
    ArtifactStore, ArtifactType, Priority, Progress, WorkstreamState,
};
use crate::todo::{Phase, PhasedTodos, TodoItem, TodoPriority, TodoStatus, TodoStore, TodoStoreError};

/// ACE-backed todo storage.
///
/// Uses ACE Task artifacts stored in `specs/tasks/` as the backend.
/// Tasks are parented to the session artifact.
pub struct AceTodoStore;

impl AceTodoStore {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AceTodoStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoStore for AceTodoStore {
    fn get_phased(&self, project_root: &Path) -> PhasedTodos {
        // Try to get state and tasks
        let state = match WorkstreamState::load(project_root) {
            Ok(Some(s)) => s,
            _ => return PhasedTodos::new(),
        };

        let store = match ArtifactStore::new(project_root) {
            Ok(s) => s,
            Err(_) => return PhasedTodos::new(),
        };

        // Get all tasks for this ticket
        let tasks = match store.list_artifacts_for_ticket(ArtifactType::Task, &state.ticket_id) {
            Ok(t) => t,
            Err(_) => return PhasedTodos::new(),
        };

        // Group tasks by phase
        let mut phase_map: HashMap<String, Vec<TodoItem>> = HashMap::new();

        for task in tasks {
            let phase_name = task
                .metadata
                .phase
                .clone()
                .unwrap_or_else(|| "Unphased".to_string());

            let status = match task.metadata.progress {
                Progress::InProgress => TodoStatus::InProgress,
                Progress::Done => TodoStatus::Completed,
                Progress::Discarded => TodoStatus::Cancelled,
                _ => TodoStatus::Pending,
            };

            let priority = match task.metadata.priority {
                Priority::High => TodoPriority::High,
                Priority::Medium => TodoPriority::Medium,
                Priority::Low => TodoPriority::Low,
            };

            let item = TodoItem {
                id: task.metadata.id,
                content: task.title,
                status,
                priority,
                parents: task.metadata.parents,
            };

            phase_map.entry(phase_name).or_default().push(item);
        }

        // Build PhasedTodos with sorted phases
        let mut phased_todos = PhasedTodos::new();
        let mut phase_names: Vec<String> = phase_map.keys().cloned().collect();
        phase_names.sort_by(|a, b| {
            if a == "Unphased" {
                std::cmp::Ordering::Greater
            } else if b == "Unphased" {
                std::cmp::Ordering::Less
            } else {
                a.cmp(b)
            }
        });

        for phase_name in phase_names {
            if let Some(items) = phase_map.remove(&phase_name) {
                let phase_id = phase_name.to_lowercase().replace(' ', "_");
                let mut phase = Phase::new(phase_id, phase_name);
                phase.todos = items;
                phased_todos.add_phase(phase);
            }
        }

        phased_todos
    }

    fn set_phased(&self, project_root: &Path, todos: PhasedTodos) -> Result<(), TodoStoreError> {
        // Get or create state
        let mut state = WorkstreamState::ensure_initialized(project_root)
            .map_err(|e| TodoStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to initialize workstream: {}", e),
            )))?;

        let store = ArtifactStore::new(project_root)
            .map_err(|e| TodoStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create store: {}", e),
            )))?;

        store.ensure_directories()
            .map_err(|e| TodoStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create directories: {}", e),
            )))?;

        // Ensure session exists
        let session_id = store.ensure_session(&mut state, project_root)
            .map_err(|e| TodoStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to ensure session: {}", e),
            )))?;

        // Get existing tasks to compare
        let existing_tasks = store.list_artifacts_for_ticket(ArtifactType::Task, &state.ticket_id)
            .unwrap_or_default();
        let existing_ids: std::collections::HashSet<String> = existing_tasks
            .iter()
            .map(|t| t.metadata.id.clone())
            .collect();

        // Track which IDs we've seen in the input
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Process each phase and todo
        for phase in &todos.phases {
            for todo in &phase.todos {
                seen_ids.insert(todo.id.clone());

                if existing_ids.contains(&todo.id) {
                    // Update existing task
                    let progress = match todo.status {
                        TodoStatus::Pending => Progress::Backlog,
                        TodoStatus::InProgress => Progress::InProgress,
                        TodoStatus::Completed => Progress::Done,
                        TodoStatus::Cancelled => Progress::Discarded,
                    };
                    let _ = store.update_artifact_progress(&todo.id, progress);
                } else {
                    // Create new task
                    let priority = match todo.priority {
                        TodoPriority::High => Priority::High,
                        TodoPriority::Medium => Priority::Medium,
                        TodoPriority::Low => Priority::Low,
                    };

                    let _ = store.create_artifact_with_phase(
                        &mut state,
                        ArtifactType::Task,
                        &todo.content,
                        "",
                        vec![session_id.clone()],
                        priority,
                        false,
                        Some(phase.name.clone()),
                    );
                }
            }
        }

        // Mark tasks not in input as discarded
        for existing_id in existing_ids {
            if !seen_ids.contains(&existing_id) {
                let _ = store.update_artifact_progress(&existing_id, Progress::Discarded);
            }
        }

        state.save(project_root)
            .map_err(|e| TodoStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to save state: {}", e),
            )))?;

        Ok(())
    }

    fn clear(&self, project_root: &Path) {
        // Mark all tasks as discarded
        if let Ok(Some(state)) = WorkstreamState::load(project_root) {
            if let Ok(store) = ArtifactStore::new(project_root) {
                if let Ok(tasks) = store.list_artifacts_for_ticket(ArtifactType::Task, &state.ticket_id) {
                    for task in tasks {
                        let _ = store.update_artifact_progress(&task.metadata.id, Progress::Discarded);
                    }
                }
            }
        }
    }
}
