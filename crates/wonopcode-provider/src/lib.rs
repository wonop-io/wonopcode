//! AI provider abstraction for wonopcode.
//!
//! This crate provides a unified interface for interacting with different AI providers:
//! - Anthropic (Claude) - API and subscription access
//! - OpenAI - API access
//! - OpenAI Codex (Responses API) - API and subscription access
//!
//! ## Provider Registry
//!
//! The [`registry`] module provides a centralized source of truth for all supported
//! providers and models. Use it instead of hardcoding provider/model information.
//!
//! ```rust
//! use wonopcode_provider::registry::{get_provider, get_model_info, PROVIDERS};
//!
//! // Get all providers
//! for provider in PROVIDERS.iter() {
//!     println!("{}: {}", provider.id, provider.name);
//! }
//!
//! // Get model info
//! let model = get_model_info("claude-sonnet-4-5", "anthropic");
//! ```

pub mod error;
pub mod message;
pub mod model;
pub mod registry;
pub mod stream;

// Core providers (always enabled)
pub mod anthropic;
pub mod codex;
pub mod openai;

// CLI-based providers (subscription access)
pub mod claude_cli;

// OpenAI-compatible base (required by openai and codex)
pub mod openai_compatible;

// Dynamic model fetching
pub mod models_dev;

// Testing providers
#[cfg(test)]
pub mod mock;
pub mod test;

// ============================================================================
// Disabled providers (uncomment to re-enable)
// ============================================================================

// Google (Gemini)
// pub mod google;

// Enterprise/Cloud providers
// pub mod azure;
// pub mod bedrock;
// pub mod copilot;
// pub mod vertex;

// OpenRouter
// pub mod openrouter;

// Additional providers (OpenAI-compatible)
// pub mod compoundcoder;
// pub mod deepinfra;
// pub mod groq;
// pub mod mistral;
// pub mod together;
// pub mod xai;

pub use error::{ProviderError, ProviderResult};
pub use message::{ContentPart, Message, Role};
pub use model::{ModelCapabilities, ModelCost, ModelInfo, ModelLimit};
pub use stream::StreamChunk;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::Value;
use std::sync::Arc;

/// Options for text generation.
#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    /// Temperature for sampling (0.0-1.0).
    pub temperature: Option<f32>,
    /// Top-p (nucleus) sampling.
    pub top_p: Option<f32>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// System prompt.
    pub system: Option<String>,
    /// Available tools.
    pub tools: Vec<ToolDefinition>,
    /// Cancellation token.
    pub abort: Option<tokio_util::sync::CancellationToken>,
    /// Provider-specific options.
    pub provider_options: Option<Value>,
}

/// A tool definition for the AI.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// JSON Schema for the tool parameters.
    pub parameters: Value,
}

/// The main trait for AI language models.
///
/// Implementations of this trait provide access to different AI providers.
#[async_trait]
pub trait LanguageModel: Send + Sync {
    /// Generate a streaming response.
    ///
    /// Returns a stream of `StreamChunk` items representing the response.
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>>;

    /// Get information about this model.
    fn model_info(&self) -> &ModelInfo;

    /// Get the provider ID (e.g., "anthropic", "openai").
    fn provider_id(&self) -> &str;

    /// Check if this provider maintains stateful sessions.
    ///
    /// Stateful providers (like Claude CLI with --resume) maintain conversation
    /// history on the server/process side. When context compaction occurs,
    /// these providers must reset their session and start fresh with the
    /// compacted message history.
    ///
    /// Returns `false` by default (API-based providers are stateless).
    fn is_stateful(&self) -> bool {
        false
    }

    /// Reset the provider's session state.
    ///
    /// Called after context compaction to ensure stateful providers start
    /// a fresh session with the compacted message history. For stateless
    /// providers, this is a no-op.
    ///
    /// This clears any session ID, cached state, or server-side context
    /// that would cause the provider to use outdated conversation history.
    async fn reset_session(&self) {
        // Default implementation: clear CLI session ID if any
        self.set_cli_session_id(None).await;
    }

    /// Get the CLI session ID if this provider uses CLI-based access.
    ///
    /// This is used for session persistence with providers like Claude CLI.
    /// Returns `None` for API-based providers.
    async fn get_cli_session_id(&self) -> Option<String> {
        None
    }

    /// Set the CLI session ID for session resumption.
    ///
    /// This is used to restore session state when recreating providers.
    /// Has no effect on API-based providers.
    async fn set_cli_session_id(&self, _session_id: Option<String>) {
        // Default implementation does nothing
    }

    /// Get the tool execution timeout for this provider, if any.
    ///
    /// When set, tool calls (including permission requests) will be cancelled
    /// after this duration. This is needed for providers like Claude CLI that
    /// have a hard timeout on MCP tool calls.
    ///
    /// Returns `None` by default, meaning no timeout (wait indefinitely).
    fn tool_timeout(&self) -> Option<std::time::Duration> {
        None
    }
}

/// A boxed language model for dynamic dispatch.
pub type BoxedLanguageModel = Arc<dyn LanguageModel>;
