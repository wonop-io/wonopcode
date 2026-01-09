//! AI provider abstraction for wonopcode.
//!
//! This crate provides a unified interface for interacting with different AI providers:
//! - Anthropic (Claude)
//! - OpenAI
//! - Google (Gemini)
//! - Google Vertex AI
//! - OpenRouter
//! - Amazon Bedrock
//! - Azure OpenAI
//! - GitHub Copilot
//! - xAI (Grok)
//! - Mistral
//! - Groq
//! - DeepInfra
//! - Together AI
//! - OpenAI-compatible custom providers

pub mod error;
pub mod message;
pub mod model;
pub mod stream;

pub mod anthropic;
pub mod google;
pub mod openai;
pub mod openai_compatible;
pub mod openrouter;

// Enterprise/Cloud providers
pub mod azure;
pub mod bedrock;
pub mod copilot;
pub mod vertex;

// Additional providers (OpenAI-compatible)
pub mod deepinfra;
pub mod groq;
pub mod mistral;
pub mod together;
pub mod xai;

// CLI-based providers (subscription access)
pub mod claude_cli;

// Dynamic model fetching
pub mod models_dev;

// Testing providers
#[cfg(test)]
pub mod mock;
pub mod test;

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
}

/// A boxed language model for dynamic dispatch.
pub type BoxedLanguageModel = Arc<dyn LanguageModel>;
