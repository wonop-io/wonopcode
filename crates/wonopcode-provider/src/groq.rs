//! Groq provider implementation.
//!
//! Uses OpenAI-compatible API with Groq's base URL.
//! Groq provides extremely fast inference on LPU hardware.

use crate::{model::ModelInfo, openai::OpenAIProvider, LanguageModel, ProviderResult};

/// Groq provider (ultra-fast inference).
pub struct GroqProvider {
    inner: OpenAIProvider,
}

impl GroqProvider {
    /// Create a new Groq provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let inner =
            OpenAIProvider::with_base_url(api_key, "https://api.groq.com/openai/v1", model)?;
        Ok(Self { inner })
    }
}

use crate::{GenerateOptions, Message, StreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;

#[async_trait]
impl LanguageModel for GroqProvider {
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
        "groq"
    }
}

/// Built-in model definitions for Groq.
pub mod models {
    use crate::model::*;

    /// Llama 3.3 70B (versatile, fast).
    pub fn llama_3_3_70b() -> ModelInfo {
        ModelInfo {
            id: "llama-3.3-70b-versatile".to_string(),
            provider_id: "groq".to_string(),
            name: "Llama 3.3 70B".to_string(),
            family: Some("llama-3".to_string()),
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
                input: 0.59,
                output: 0.79,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 128_000,
                output: 32_768,
            },
            status: ModelStatus::Active,
        }
    }

    /// Llama 3.1 8B (instant, very fast).
    pub fn llama_3_1_8b() -> ModelInfo {
        ModelInfo {
            id: "llama-3.1-8b-instant".to_string(),
            provider_id: "groq".to_string(),
            name: "Llama 3.1 8B Instant".to_string(),
            family: Some("llama-3".to_string()),
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
                input: 0.05,
                output: 0.08,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 128_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Mixtral 8x7B.
    pub fn mixtral_8x7b() -> ModelInfo {
        ModelInfo {
            id: "mixtral-8x7b-32768".to_string(),
            provider_id: "groq".to_string(),
            name: "Mixtral 8x7B".to_string(),
            family: Some("mixtral".to_string()),
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
                input: 0.24,
                output: 0.24,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 32_768,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Gemma 2 9B.
    pub fn gemma_2_9b() -> ModelInfo {
        ModelInfo {
            id: "gemma2-9b-it".to_string(),
            provider_id: "groq".to_string(),
            name: "Gemma 2 9B".to_string(),
            family: Some("gemma".to_string()),
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
                input: 0.2,
                output: 0.2,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 8_192,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// DeepSeek R1 Distill Llama 70B (reasoning).
    pub fn deepseek_r1_distill() -> ModelInfo {
        ModelInfo {
            id: "deepseek-r1-distill-llama-70b".to_string(),
            provider_id: "groq".to_string(),
            name: "DeepSeek R1 Distill 70B".to_string(),
            family: Some("deepseek".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
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
                input: 0.75,
                output: 0.99,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 128_000,
                output: 16_384,
            },
            status: ModelStatus::Active,
        }
    }
}
