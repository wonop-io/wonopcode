//! Server state.

use crate::prompt::{new_session_runners, SessionRunners};
use std::sync::Arc;
use tokio::sync::RwLock;
use wonopcode_core::{Bus, Instance, PermissionManager};
use wonopcode_tools::todo::TodoItem;

#[cfg(test)]
mod tests {
    use super::*;
    use wonopcode_tools::todo::{TodoPriority, TodoStatus};

    #[tokio::test]
    async fn test_new_todo_store_is_empty() {
        let store = new_todo_store();
        let todos = store.read().await;
        assert!(todos.is_empty());
    }

    #[tokio::test]
    async fn test_shared_todo_store_set_and_get() {
        let store = new_todo_store();

        let todos = vec![
            TodoItem {
                id: "1".to_string(),
                content: "First task".to_string(),
                status: TodoStatus::Pending,
                priority: TodoPriority::High,
            },
            TodoItem {
                id: "2".to_string(),
                content: "Second task".to_string(),
                status: TodoStatus::InProgress,
                priority: TodoPriority::Medium,
            },
        ];

        {
            let mut write_guard = store.write().await;
            *write_guard = todos;
        }

        let retrieved = store.read().await;
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].id, "1");
        assert_eq!(retrieved[0].content, "First task");
        assert_eq!(retrieved[1].id, "2");
        assert_eq!(retrieved[1].content, "Second task");
    }

    #[tokio::test]
    async fn test_shared_todo_store_clear() {
        let store = new_todo_store();

        {
            let mut write_guard = store.write().await;
            write_guard.push(TodoItem {
                id: "1".to_string(),
                content: "Task".to_string(),
                status: TodoStatus::Pending,
                priority: TodoPriority::Low,
            });
        }

        assert_eq!(store.read().await.len(), 1);

        {
            let mut write_guard = store.write().await;
            write_guard.clear();
        }

        assert!(store.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_shared_todo_store_clone_shares_data() {
        let store = new_todo_store();
        let cloned = store.clone();

        {
            let mut write_guard = store.write().await;
            write_guard.push(TodoItem {
                id: "1".to_string(),
                content: "Task".to_string(),
                status: TodoStatus::Completed,
                priority: TodoPriority::Medium,
            });
        }

        // Both stores should see the same data
        let original_todos = store.read().await;
        let cloned_todos = cloned.read().await;

        assert_eq!(original_todos.len(), cloned_todos.len());
        assert_eq!(original_todos[0].id, cloned_todos[0].id);
    }

    #[tokio::test]
    async fn test_shared_todo_store_replaces_existing() {
        let store = new_todo_store();

        let todos1 = vec![TodoItem {
            id: "1".to_string(),
            content: "First".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        }];

        let todos2 = vec![
            TodoItem {
                id: "2".to_string(),
                content: "Second".to_string(),
                status: TodoStatus::Pending,
                priority: TodoPriority::Low,
            },
            TodoItem {
                id: "3".to_string(),
                content: "Third".to_string(),
                status: TodoStatus::Completed,
                priority: TodoPriority::Medium,
            },
        ];

        {
            let mut write_guard = store.write().await;
            *write_guard = todos1;
        }
        assert_eq!(store.read().await.len(), 1);

        {
            let mut write_guard = store.write().await;
            *write_guard = todos2;
        }
        let retrieved = store.read().await;
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].id, "2");
        assert_eq!(retrieved[1].id, "3");
    }

    #[tokio::test]
    async fn test_shared_todo_store_concurrent_access() {
        let store = new_todo_store();
        let store_clone = store.clone();

        // Simulate concurrent writes
        let handle1 = tokio::spawn({
            let store = store.clone();
            async move {
                for i in 0..10 {
                    let mut guard = store.write().await;
                    guard.push(TodoItem {
                        id: format!("a{}", i),
                        content: format!("Task A{}", i),
                        status: TodoStatus::Pending,
                        priority: TodoPriority::High,
                    });
                }
            }
        });

        let handle2 = tokio::spawn({
            let store = store_clone;
            async move {
                for i in 0..10 {
                    let mut guard = store.write().await;
                    guard.push(TodoItem {
                        id: format!("b{}", i),
                        content: format!("Task B{}", i),
                        status: TodoStatus::InProgress,
                        priority: TodoPriority::Medium,
                    });
                }
            }
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        // All 20 items should be present
        let todos = store.read().await;
        assert_eq!(todos.len(), 20);
    }
}

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
