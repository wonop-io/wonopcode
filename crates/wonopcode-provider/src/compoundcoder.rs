//! CompoundCoder provider implementation.
//!
//! Uses OpenAI-compatible API with CompoundCoder's base URL.
//! CompoundCoder provides access to various models including Wonop GPT and Qwen.

use crate::{model::ModelInfo, openai::OpenAIProvider, LanguageModel, ProviderResult};

/// CompoundCoder provider.
pub struct CompoundCoderProvider {
    inner: OpenAIProvider,
}

impl CompoundCoderProvider {
    /// Create a new CompoundCoder provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let inner =
            OpenAIProvider::with_base_url(api_key, "https://api.compoundcoders.com/v1", model)?;
        Ok(Self { inner })
    }
}

use crate::{GenerateOptions, Message, StreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;

#[async_trait]
impl LanguageModel for CompoundCoderProvider {
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
        "compoundcoder"
    }
}

/// Built-in model definitions for CompoundCoder.
pub mod models {
    use crate::model::*;

    /// Wonop GPT.
    pub fn wonop_gpt() -> ModelInfo {
        ModelInfo {
            id: "wonop/gpt".to_string(),
            provider_id: "compoundcoder".to_string(),
            name: "Wonop GPT".to_string(),
            family: Some("gpt".to_string()),
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
                input: 3.0,
                output: 15.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 128_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Wonop Qwen.
    pub fn wonop_qwen() -> ModelInfo {
        ModelInfo {
            id: "wonop/qwen".to_string(),
            provider_id: "compoundcoder".to_string(),
            name: "Wonop Qwen".to_string(),
            family: Some("qwen".to_string()),
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
                input: 3.0,
                output: 15.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 200_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Wonop Devstral2.
    pub fn wonop_devstral2() -> ModelInfo {
        ModelInfo {
            id: "wonop/devstral2".to_string(),
            provider_id: "compoundcoder".to_string(),
            name: "Wonop Devstral2".to_string(),
            family: Some("devstral".to_string()),
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
                input: 2.5,
                output: 10.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 32_768,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }
}
