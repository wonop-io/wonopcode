//! Adapter implementing codemode AgentService trait.
//!
//! Implements:
//! - enter_plan_mode/exit_plan_mode: Simple mode tracking
//! - spawn: Returns "not available" (requires subagent infrastructure)

use async_trait::async_trait;
use std::sync::RwLock;
use wonopcode_codemode::{
    AgentService as CodemodeService,
    SpawnAgentInput, AgentResult, PlanModeResult,
    ServiceError, ServiceResult,
};

/// Agent mode (build or plan).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMode {
    Build,
    Plan,
}

impl AgentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Build => "build",
            AgentMode::Plan => "plan",
        }
    }
}

/// Adapter that implements codemode AgentService trait.
pub struct AgentServiceAdapter {
    mode: RwLock<AgentMode>,
}

impl AgentServiceAdapter {
    /// Create a new AgentServiceAdapter.
    pub fn new() -> Self {
        Self {
            mode: RwLock::new(AgentMode::Build),
        }
    }
}

impl Default for AgentServiceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CodemodeService for AgentServiceAdapter {
    async fn spawn(&self, _input: SpawnAgentInput) -> ServiceResult<AgentResult> {
        // Subagent spawning requires complex infrastructure
        // - Creating a new agent loop
        // - Managing subagent lifecycle
        // - Handling permissions
        Err(ServiceError::not_available("agents.spawn"))
    }

    async fn enter_plan_mode(&self, reason: Option<String>) -> ServiceResult<PlanModeResult> {
        let mut mode = self.mode.write().map_err(|_| {
            ServiceError::new("LOCK_ERROR", "Failed to acquire mode lock")
        })?;
        
        if *mode == AgentMode::Plan {
            return Ok(PlanModeResult {
                mode: "plan".to_string(),
                message: "Already in plan mode".to_string(),
                changed: false,
            });
        }
        
        *mode = AgentMode::Plan;
        
        Ok(PlanModeResult {
            mode: "plan".to_string(),
            message: reason.unwrap_or_else(|| "Entered plan mode (read-only)".to_string()),
            changed: true,
        })
    }

    async fn exit_plan_mode(&self, summary: Option<String>) -> ServiceResult<PlanModeResult> {
        let mut mode = self.mode.write().map_err(|_| {
            ServiceError::new("LOCK_ERROR", "Failed to acquire mode lock")
        })?;
        
        if *mode == AgentMode::Build {
            return Ok(PlanModeResult {
                mode: "build".to_string(),
                message: "Already in build mode".to_string(),
                changed: false,
            });
        }
        
        *mode = AgentMode::Build;
        
        Ok(PlanModeResult {
            mode: "build".to_string(),
            message: summary.unwrap_or_else(|| "Exited plan mode (back to build)".to_string()),
            changed: true,
        })
    }

    fn current_mode(&self) -> String {
        self.mode
            .read()
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|_| "build".to_string())
    }
}
