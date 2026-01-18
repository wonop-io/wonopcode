//! xAI (Grok) provider implementation.
//!
//! Uses OpenAI-compatible API with xAI's base URL.

use crate::{model::ModelInfo, openai::OpenAIProvider, LanguageModel, ProviderResult};

/// xAI provider (Grok models).
pub struct XaiProvider {
    inner: OpenAIProvider,
}

impl XaiProvider {
    /// Create a new xAI provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let inner = OpenAIProvider::with_base_url(api_key, "https://api.x.ai/v1", model)?;
        Ok(Self { inner })
    }
}

impl std::ops::Deref for XaiProvider {
    type Target = OpenAIProvider;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Re-export the LanguageModel implementation through Deref
// The OpenAIProvider already implements LanguageModel, so we can use it directly
// by wrapping in Arc<XaiProvider> and implementing LanguageModel

use crate::{GenerateOptions, Message, StreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;

#[async_trait]
impl LanguageModel for XaiProvider {
    async fn generate(
        &self,
        messages: Vec<Message>,
        options: GenerateOptions,
    ) -> ProviderResult<BoxStream<'static, ProviderResult<StreamChunk>>> {
        self.inner.generate(messages, options).await
    }

    fn model_info(&self) -> &ModelInfo {
        self.inner.model_info()
    }

    fn provider_id(&self) -> &str {
        "xai"
    }
}

/// Built-in model definitions for xAI.
pub mod models {
    use crate::model::*;

    /// Grok 3.
    pub fn grok_3() -> ModelInfo {
        ModelInfo {
            id: "grok-3".to_string(),
            provider_id: "xai".to_string(),
            name: "Grok 3".to_string(),
            family: Some("grok".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    ..Default::default()
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 131_072,
                output: 131_072,
            },
            status: ModelStatus::Active,
        }
    }

    /// Grok 3 Mini (faster, cheaper).
    pub fn grok_3_mini() -> ModelInfo {
        ModelInfo {
            id: "grok-3-mini".to_string(),
            provider_id: "xai".to_string(),
            name: "Grok 3 Mini".to_string(),
            family: Some("grok".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    ..Default::default()
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.3,
                output: 0.5,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 131_072,
                output: 131_072,
            },
            status: ModelStatus::Active,
        }
    }

    /// Grok 2.
    pub fn grok_2() -> ModelInfo {
        ModelInfo {
            id: "grok-2-1212".to_string(),
            provider_id: "xai".to_string(),
            name: "Grok 2".to_string(),
            family: Some("grok".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    ..Default::default()
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 2.0,
                output: 10.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 131_072,
                output: 131_072,
            },
            status: ModelStatus::Active,
        }
    }
}
