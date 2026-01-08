//! Server state.

use crate::prompt::{new_session_runners, SessionRunners};
use std::sync::Arc;
use tokio::sync::RwLock;
use wonopcode_core::{Bus, Instance, PermissionManager};

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
}
