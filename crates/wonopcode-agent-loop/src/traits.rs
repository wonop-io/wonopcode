//! Core trait for agent loop implementations.

use async_trait::async_trait;

use crate::{LoopCapabilities, LoopContext, LoopError};

/// Core trait for implementing agentic loops.
///
/// An agentic loop handles the interaction pattern between:
/// - User prompts
/// - LLM provider streaming
/// - Tool execution
/// - Response generation
///
/// # Implementing
///
/// Implementations must be `Send + Sync` to support async execution.
///
/// ```rust,ignore
/// use wonopcode_agent_loop::{AgentLoop, LoopContext, LoopError, LoopCapabilities};
/// use async_trait::async_trait;
///
/// struct MyLoop;
///
/// #[async_trait]
/// impl AgentLoop for MyLoop {
///     async fn run_prompt(
///         &self,
///         ctx: &mut LoopContext<'_>,
///         user_input: &str,
///     ) -> Result<String, LoopError> {
///         // Implementation
///         Ok("response".to_string())
///     }
///
///     fn name(&self) -> &'static str {
///         "my-loop"
///     }
/// }
/// ```
#[async_trait]
pub trait AgentLoop: Send + Sync {
    /// Execute a single prompt through the agentic loop.
    ///
    /// # Arguments
    /// * `ctx` - Mutable context containing conversation state and resources
    /// * `user_input` - The user's prompt text
    ///
    /// # Returns
    /// The final response text, or an error if the loop failed.
    ///
    /// # Updates
    /// The loop should send updates via `ctx.send_update()` for:
    /// - Streaming text deltas
    /// - Tool execution progress
    /// - Token usage
    async fn run_prompt(
        &self,
        ctx: &mut LoopContext<'_>,
        user_input: &str,
    ) -> Result<String, LoopError>;

    /// Called before the first prompt in a session.
    ///
    /// Use for initialization that requires context access.
    /// Default implementation does nothing.
    async fn on_start(&self, _ctx: &mut LoopContext<'_>) -> Result<(), LoopError> {
        Ok(())
    }

    /// Called after a prompt completes successfully.
    ///
    /// Use for cleanup or logging.
    /// Default implementation does nothing.
    async fn on_complete(
        &self,
        _ctx: &mut LoopContext<'_>,
        _result: &str,
    ) -> Result<(), LoopError> {
        Ok(())
    }

    /// Called when a prompt is cancelled.
    ///
    /// Use for cleanup after cancellation.
    /// Default implementation does nothing.
    async fn on_cancel(&self, _ctx: &mut LoopContext<'_>) -> Result<(), LoopError> {
        Ok(())
    }

    /// Get the name of this loop implementation.
    ///
    /// Used for logging and debugging.
    fn name(&self) -> &'static str;

    /// Get the capabilities of this loop implementation.
    ///
    /// Informs the system what features this loop supports.
    fn capabilities(&self) -> LoopCapabilities {
        LoopCapabilities::default()
    }
}

/// A boxed agent loop for dynamic dispatch.
pub type BoxedAgentLoop = Box<dyn AgentLoop>;

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLoop;

    #[async_trait]
    impl AgentLoop for TestLoop {
        async fn run_prompt(
            &self,
            _ctx: &mut LoopContext<'_>,
            user_input: &str,
        ) -> Result<String, LoopError> {
            Ok(format!("Echo: {}", user_input))
        }

        fn name(&self) -> &'static str {
            "test-loop"
        }

        fn capabilities(&self) -> LoopCapabilities {
            LoopCapabilities::minimal()
        }
    }

    #[test]
    fn test_loop_name() {
        let loop_impl = TestLoop;
        assert_eq!(loop_impl.name(), "test-loop");
    }

    #[test]
    fn test_loop_capabilities() {
        let loop_impl = TestLoop;
        let caps = loop_impl.capabilities();
        assert!(!caps.streaming);
        assert!(!caps.parallel_tools);
    }
}
