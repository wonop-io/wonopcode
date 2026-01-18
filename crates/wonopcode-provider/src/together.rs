//! Together AI provider implementation.
//!
//! Uses OpenAI-compatible API with Together's base URL.
//! Together provides access to many open-source models.

use crate::{model::ModelInfo, openai::OpenAIProvider, LanguageModel, ProviderResult};

/// Together AI provider.
pub struct TogetherProvider {
    inner: OpenAIProvider,
}

impl TogetherProvider {
    /// Create a new Together provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let inner = OpenAIProvider::with_base_url(api_key, "https://api.together.xyz/v1", model)?;
        Ok(Self { inner })
    }
}

use crate::{GenerateOptions, Message, StreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;

#[async_trait]
impl LanguageModel for TogetherProvider {
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
        "together"
    }
}

/// Built-in model definitions for Together.
pub mod models {
    use crate::model::*;

    /// DeepSeek V3.
    pub fn deepseek_v3() -> ModelInfo {
        ModelInfo {
            id: "deepseek-ai/DeepSeek-V3".to_string(),
            provider_id: "together".to_string(),
            name: "DeepSeek V3".to_string(),
            family: Some("deepseek".to_string()),
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
                input: 0.49,
                output: 0.89,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 64_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// DeepSeek R1.
    pub fn deepseek_r1() -> ModelInfo {
        ModelInfo {
            id: "deepseek-ai/DeepSeek-R1".to_string(),
            provider_id: "together".to_string(),
            name: "DeepSeek R1".to_string(),
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
                interleaved: true,
            },
            cost: ModelCost {
                input: 3.0,
                output: 7.0,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 164_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Llama 3.3 70B.
    pub fn llama_3_3_70b() -> ModelInfo {
        ModelInfo {
            id: "meta-llama/Llama-3.3-70B-Instruct-Turbo".to_string(),
            provider_id: "together".to_string(),
            name: "Llama 3.3 70B Turbo".to_string(),
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
                input: 0.88,
                output: 0.88,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 131_072,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Qwen 2.5 72B.
    pub fn qwen_2_5_72b() -> ModelInfo {
        ModelInfo {
            id: "Qwen/Qwen2.5-72B-Instruct-Turbo".to_string(),
            provider_id: "together".to_string(),
            name: "Qwen 2.5 72B Turbo".to_string(),
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
                input: 0.6,
                output: 0.6,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 32_768,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Qwen 2.5 Coder 32B.
    pub fn qwen_2_5_coder() -> ModelInfo {
        ModelInfo {
            id: "Qwen/Qwen2.5-Coder-32B-Instruct".to_string(),
            provider_id: "together".to_string(),
            name: "Qwen 2.5 Coder 32B".to_string(),
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
                input: 0.8,
                output: 0.8,
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
