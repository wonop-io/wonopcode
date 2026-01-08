//! Mistral AI provider implementation.
//!
//! Uses OpenAI-compatible API with Mistral's base URL.

use crate::{model::ModelInfo, openai::OpenAIProvider, LanguageModel, ProviderResult};

/// Mistral AI provider.
pub struct MistralProvider {
    inner: OpenAIProvider,
}

impl MistralProvider {
    /// Create a new Mistral provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let inner = OpenAIProvider::with_base_url(api_key, "https://api.mistral.ai/v1", model)?;
        Ok(Self { inner })
    }
}

use crate::{GenerateOptions, Message, StreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;

#[async_trait]
impl LanguageModel for MistralProvider {
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
        "mistral"
    }
}

/// Built-in model definitions for Mistral.
pub mod models {
    use crate::model::*;

    /// Mistral Large (latest).
    pub fn mistral_large() -> ModelInfo {
        ModelInfo {
            id: "mistral-large-latest".to_string(),
            provider_id: "mistral".to_string(),
            name: "Mistral Large".to_string(),
            family: Some("mistral".to_string()),
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
                output: 6.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 128_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Mistral Small (faster, cheaper).
    pub fn mistral_small() -> ModelInfo {
        ModelInfo {
            id: "mistral-small-latest".to_string(),
            provider_id: "mistral".to_string(),
            name: "Mistral Small".to_string(),
            family: Some("mistral".to_string()),
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
                input: 0.1,
                output: 0.3,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 32_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Codestral (code-focused).
    pub fn codestral() -> ModelInfo {
        ModelInfo {
            id: "codestral-latest".to_string(),
            provider_id: "mistral".to_string(),
            name: "Codestral".to_string(),
            family: Some("codestral".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: false,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
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
                output: 0.9,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 256_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Pixtral Large (multimodal).
    pub fn pixtral_large() -> ModelInfo {
        ModelInfo {
            id: "pixtral-large-latest".to_string(),
            provider_id: "mistral".to_string(),
            name: "Pixtral Large".to_string(),
            family: Some("pixtral".to_string()),
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
                output: 6.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 128_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }
}
