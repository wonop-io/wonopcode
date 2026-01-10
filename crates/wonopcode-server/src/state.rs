//! Server state.

use crate::prompt::{new_session_runners, SessionRunners};
use std::sync::Arc;
use tokio::sync::RwLock;
use wonopcode_core::{Bus, Instance, PermissionManager};
use wonopcode_tools::todo::TodoItem;

/// Shared todo storage for the server.
/// Uses Arc<RwLock<...>> to allow concurrent access from multiple routes and
/// updates from the prompt runner.
pub type SharedTodoStore = Arc<RwLock<Vec<TodoItem>>>;

/// Create a new empty shared todo store.
pub fn new_todo_store() -> SharedTodoStore {
    Arc::new(RwLock::new(Vec::new()))
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// The core instance.
    pub instance: Arc<RwLock<Instance>>,
    /// Event bus.
    pub bus: Bus,
    /// Active session runners for abort support.
    pub session_runners: SessionRunners,
    /// Permission manager.
    pub permission_manager: Arc<PermissionManager>,
    /// Shared todo store - held by the server and pulled by clients.
    pub todo_store: SharedTodoStore,
}

impl AppState {
    /// Create a new app state.
    pub fn new(instance: Instance, bus: Bus) -> Self {
        let permission_manager = Arc::new(PermissionManager::new(bus.clone()));
        Self {
            instance: Arc::new(RwLock::new(instance)),
            bus,
            session_runners: new_session_runners(),
            permission_manager,
            todo_store: new_todo_store(),
        }
    }

    /// Create a new app state and initialize with default permission rules.
    pub async fn new_with_defaults(instance: Instance, bus: Bus) -> Self {
        let state = Self::new(instance, bus);

        // Add default permission rules
        for rule in PermissionManager::default_rules() {
            state.permission_manager.add_rule(rule).await;
        }

        state
    }

    /// Update the todo list with new items.
    /// This replaces the current todo list with the provided items.
    pub async fn set_todos(&self, todos: Vec<TodoItem>) {
        let mut store = self.todo_store.write().await;
        *store = todos;
    }

    /// Get a clone of the current todo list.
    pub async fn get_todos(&self) -> Vec<TodoItem> {
        let store = self.todo_store.read().await;
        store.clone()
    }

    /// Clear the todo list.
    pub async fn clear_todos(&self) {
        let mut store = self.todo_store.write().await;
        store.clear();
    }
}
