//! DeepInfra provider implementation.
//!
//! Uses OpenAI-compatible API with DeepInfra's base URL.
//! DeepInfra provides access to many open-source models.

use crate::{model::ModelInfo, openai::OpenAIProvider, LanguageModel, ProviderResult};

/// DeepInfra provider.
pub struct DeepInfraProvider {
    inner: OpenAIProvider,
}

impl DeepInfraProvider {
    /// Create a new DeepInfra provider.
    pub fn new(api_key: &str, model: ModelInfo) -> ProviderResult<Self> {
        let inner =
            OpenAIProvider::with_base_url(api_key, "https://api.deepinfra.com/v1/openai", model)?;
        Ok(Self { inner })
    }
}

use crate::{GenerateOptions, Message, StreamChunk};
use async_trait::async_trait;
use futures::stream::BoxStream;

#[async_trait]
impl LanguageModel for DeepInfraProvider {
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
        "deepinfra"
    }
}

/// Built-in model definitions for DeepInfra.
pub mod models {
    use crate::model::*;

    /// DeepSeek V3.
    pub fn deepseek_v3() -> ModelInfo {
        ModelInfo {
            id: "deepseek-ai/DeepSeek-V3".to_string(),
            provider_id: "deepinfra".to_string(),
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

    /// DeepSeek R1 (reasoning).
    pub fn deepseek_r1() -> ModelInfo {
        ModelInfo {
            id: "deepseek-ai/DeepSeek-R1".to_string(),
            provider_id: "deepinfra".to_string(),
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
                input: 0.55,
                output: 2.19,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 64_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Qwen 2.5 72B.
    pub fn qwen_2_5_72b() -> ModelInfo {
        ModelInfo {
            id: "Qwen/Qwen2.5-72B-Instruct".to_string(),
            provider_id: "deepinfra".to_string(),
            name: "Qwen 2.5 72B".to_string(),
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
                input: 0.35,
                output: 0.4,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 32_768,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Llama 3.1 405B.
    pub fn llama_3_1_405b() -> ModelInfo {
        ModelInfo {
            id: "meta-llama/Meta-Llama-3.1-405B-Instruct".to_string(),
            provider_id: "deepinfra".to_string(),
            name: "Llama 3.1 405B".to_string(),
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
                input: 1.79,
                output: 1.79,
                ..Default::default()
            },
            limit: ModelLimit {
                context: 32_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }
}
