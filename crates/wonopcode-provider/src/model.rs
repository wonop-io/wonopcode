//! Model information types.

use serde::{Deserialize, Serialize};

/// Information about an AI model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelInfo {
    /// Model ID (e.g., "claude-sonnet-4-20250514").
    pub id: String,
    /// Provider ID (e.g., "anthropic").
    pub provider_id: String,
    /// Human-readable name.
    pub name: String,
    /// Model family (e.g., "claude-4").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Model capabilities.
    pub capabilities: ModelCapabilities,
    /// Pricing information.
    pub cost: ModelCost,
    /// Token limits.
    pub limit: ModelLimit,
    /// Model status.
    #[serde(default)]
    pub status: ModelStatus,
}

/// Model capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Supports temperature parameter.
    #[serde(default)]
    pub temperature: bool,
    /// Supports reasoning/thinking mode.
    #[serde(default)]
    pub reasoning: bool,
    /// Supports file attachments.
    #[serde(default)]
    pub attachment: bool,
    /// Supports tool/function calling.
    #[serde(default = "default_true")]
    pub tool_call: bool,
    /// Input modality support.
    #[serde(default)]
    pub input: ModalitySupport,
    /// Output modality support.
    #[serde(default)]
    pub output: ModalitySupport,
    /// Supports interleaved content (text + thinking).
    #[serde(default)]
    pub interleaved: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            temperature: true,
            reasoning: false,
            attachment: false,
            tool_call: true,
            input: ModalitySupport::default(),
            output: ModalitySupport::default(),
            interleaved: false,
        }
    }
}

/// Modality support (input or output).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModalitySupport {
    /// Supports text.
    #[serde(default = "default_true")]
    pub text: bool,
    /// Supports images.
    #[serde(default)]
    pub image: bool,
    /// Supports audio.
    #[serde(default)]
    pub audio: bool,
    /// Supports video.
    #[serde(default)]
    pub video: bool,
    /// Supports PDF documents.
    #[serde(default)]
    pub pdf: bool,
}

/// Model pricing (per million tokens).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelCost {
    /// Input token cost (per million).
    pub input: f64,
    /// Output token cost (per million).
    pub output: f64,
    /// Cache read cost (per million).
    #[serde(default)]
    pub cache_read: f64,
    /// Cache write cost (per million).
    #[serde(default)]
    pub cache_write: f64,
}

impl ModelCost {
    /// Calculate the cost for a given usage.
    pub fn calculate(&self, input_tokens: u32, output_tokens: u32) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output;
        input_cost + output_cost
    }

    /// Calculate cost including cache tokens.
    pub fn calculate_with_cache(
        &self,
        input_tokens: u32,
        output_tokens: u32,
        cache_read: u32,
        cache_write: u32,
    ) -> f64 {
        self.calculate(input_tokens, output_tokens)
            + (cache_read as f64 / 1_000_000.0) * self.cache_read
            + (cache_write as f64 / 1_000_000.0) * self.cache_write
    }
}

/// Model token limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelLimit {
    /// Maximum context length (input + output).
    pub context: u32,
    /// Maximum output tokens.
    pub output: u32,
}

/// Model status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    /// Model is in alpha testing.
    Alpha,
    /// Model is in beta testing.
    Beta,
    /// Model is deprecated.
    Deprecated,
    /// Model is active and stable.
    #[default]
    Active,
}

impl ModelInfo {
    /// Create a new model info with defaults.
    pub fn new(id: impl Into<String>, provider_id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider_id: provider_id.into(),
            name: String::new(),
            family: None,
            capabilities: ModelCapabilities::default(),
            cost: ModelCost::default(),
            limit: ModelLimit::default(),
            status: ModelStatus::default(),
        }
    }

    /// Set the model name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the model capabilities.
    pub fn with_capabilities(mut self, capabilities: ModelCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set the model cost.
    pub fn with_cost(mut self, cost: ModelCost) -> Self {
        self.cost = cost;
        self
    }

    /// Set the model limits.
    pub fn with_limit(mut self, limit: ModelLimit) -> Self {
        self.limit = limit;
        self
    }
}

/// Built-in model definitions for Anthropic.
pub mod anthropic {
    use super::*;

    // ==================== Latest Models (Claude 4.5) ====================

    /// Claude Sonnet 4.5 - Smart model for complex agents and coding.
    pub fn claude_sonnet_4_5() -> ModelInfo {
        ModelInfo {
            id: "claude-sonnet-4-5-20250929".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude Sonnet 4.5".to_string(),
            family: Some("claude-4.5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
            },
            limit: ModelLimit {
                context: 200_000, // 1M available with beta header
                output: 64_000,
            },
            status: ModelStatus::Active,
        }
    }

    /// Claude Haiku 4.5 - Fastest model with near-frontier intelligence.
    pub fn claude_haiku_4_5() -> ModelInfo {
        ModelInfo {
            id: "claude-haiku-4-5-20251001".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude Haiku 4.5".to_string(),
            family: Some("claude-4.5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 1.0,
                output: 5.0,
                cache_read: 0.1,
                cache_write: 1.25,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 64_000,
            },
            status: ModelStatus::Active,
        }
    }

    /// Claude Opus 4.5 - Premium model with maximum intelligence.
    pub fn claude_opus_4_5() -> ModelInfo {
        ModelInfo {
            id: "claude-opus-4-5-20251101".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude Opus 4.5".to_string(),
            family: Some("claude-4.5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 5.0,
                output: 25.0,
                cache_read: 0.5,
                cache_write: 6.25,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 64_000,
            },
            status: ModelStatus::Active,
        }
    }

    // ==================== Legacy Models (Claude 4.x) ====================

    /// Claude Sonnet 4 (legacy).
    pub fn claude_sonnet_4() -> ModelInfo {
        ModelInfo {
            id: "claude-sonnet-4-20250514".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude Sonnet 4".to_string(),
            family: Some("claude-4".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 64_000,
            },
            status: ModelStatus::Active,
        }
    }

    /// Claude Opus 4.1 (legacy).
    pub fn claude_opus_4_1() -> ModelInfo {
        ModelInfo {
            id: "claude-opus-4-1-20250805".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude Opus 4.1".to_string(),
            family: Some("claude-4".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 15.0,
                output: 75.0,
                cache_read: 1.5,
                cache_write: 18.75,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 32_000,
            },
            status: ModelStatus::Active,
        }
    }

    /// Claude Opus 4 (legacy).
    pub fn claude_opus_4() -> ModelInfo {
        ModelInfo {
            id: "claude-opus-4-20250514".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude Opus 4".to_string(),
            family: Some("claude-4".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 15.0,
                output: 75.0,
                cache_read: 1.5,
                cache_write: 18.75,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 32_000,
            },
            status: ModelStatus::Active,
        }
    }

    // ==================== Legacy Models (Claude 3.x) ====================

    /// Claude 3.7 Sonnet (extended thinking).
    pub fn claude_sonnet_3_7() -> ModelInfo {
        ModelInfo {
            id: "claude-3-7-sonnet-20250219".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude 3.7 Sonnet".to_string(),
            family: Some("claude-3.7".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 64_000, // 128K with beta header
            },
            status: ModelStatus::Active,
        }
    }

    /// Claude Haiku 3 (fast, economical).
    pub fn claude_haiku_3() -> ModelInfo {
        ModelInfo {
            id: "claude-3-haiku-20240307".to_string(),
            provider_id: "anthropic".to_string(),
            name: "Claude 3 Haiku".to_string(),
            family: Some("claude-3".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: false,
                    video: false,
                    pdf: false,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.25,
                output: 1.25,
                cache_read: 0.03,
                cache_write: 0.30,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 4_096,
            },
            status: ModelStatus::Active,
        }
    }
}

/// Built-in model definitions for OpenAI.
pub mod openai {
    use super::*;

    // ==================== GPT-5.x Series (Latest) ====================

    /// GPT-5.2 - Best model for coding and agentic tasks.
    pub fn gpt_5_2() -> ModelInfo {
        ModelInfo {
            id: "gpt-5.2".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-5.2".to_string(),
            family: Some("gpt-5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 2.5,
                output: 10.0,
                cache_read: 1.25,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 256_000,
                output: 32_768,
            },
            status: ModelStatus::Active,
        }
    }

    /// GPT-5.1 - Previous flagship with configurable reasoning.
    pub fn gpt_5_1() -> ModelInfo {
        ModelInfo {
            id: "gpt-5.1".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-5.1".to_string(),
            family: Some("gpt-5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 2.5,
                output: 10.0,
                cache_read: 1.25,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 256_000,
                output: 32_768,
            },
            status: ModelStatus::Active,
        }
    }

    /// GPT-5 - Intelligent reasoning model.
    pub fn gpt_5() -> ModelInfo {
        ModelInfo {
            id: "gpt-5".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-5".to_string(),
            family: Some("gpt-5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: true,
            },
            cost: ModelCost {
                input: 2.5,
                output: 10.0,
                cache_read: 1.25,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 256_000,
                output: 32_768,
            },
            status: ModelStatus::Active,
        }
    }

    /// GPT-5 mini - Faster, cost-efficient version of GPT-5.
    pub fn gpt_5_mini() -> ModelInfo {
        ModelInfo {
            id: "gpt-5-mini".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-5 mini".to_string(),
            family: Some("gpt-5".to_string()),
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
                interleaved: true,
            },
            cost: ModelCost {
                input: 0.4,
                output: 1.6,
                cache_read: 0.2,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 128_000,
                output: 16_384,
            },
            status: ModelStatus::Active,
        }
    }

    /// GPT-5 nano - Fastest, most cost-efficient GPT-5.
    pub fn gpt_5_nano() -> ModelInfo {
        ModelInfo {
            id: "gpt-5-nano".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-5 nano".to_string(),
            family: Some("gpt-5".to_string()),
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
                output: 0.4,
                cache_read: 0.05,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 128_000,
                output: 16_384,
            },
            status: ModelStatus::Active,
        }
    }

    // ==================== GPT-4.1 Series ====================

    /// GPT-4.1 - Smartest non-reasoning model.
    pub fn gpt_4_1() -> ModelInfo {
        ModelInfo {
            id: "gpt-4.1".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-4.1".to_string(),
            family: Some("gpt-4".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: false,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    audio: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 2.0,
                output: 8.0,
                cache_read: 1.0,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 1_000_000,
                output: 32_768,
            },
            status: ModelStatus::Active,
        }
    }

    /// GPT-4.1 mini - Smaller, faster version of GPT-4.1.
    pub fn gpt_4_1_mini() -> ModelInfo {
        ModelInfo {
            id: "gpt-4.1-mini".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-4.1 mini".to_string(),
            family: Some("gpt-4".to_string()),
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
                input: 0.4,
                output: 1.6,
                cache_read: 0.2,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 1_000_000,
                output: 32_768,
            },
            status: ModelStatus::Active,
        }
    }

    /// GPT-4.1 nano - Fastest, most cost-efficient GPT-4.1.
    pub fn gpt_4_1_nano() -> ModelInfo {
        ModelInfo {
            id: "gpt-4.1-nano".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-4.1 nano".to_string(),
            family: Some("gpt-4".to_string()),
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
                output: 0.4,
                cache_read: 0.05,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 1_000_000,
                output: 32_768,
            },
            status: ModelStatus::Active,
        }
    }

    // ==================== O-Series (Reasoning) ====================

    /// O3 - Reasoning model for complex tasks.
    pub fn o3() -> ModelInfo {
        ModelInfo {
            id: "o3".to_string(),
            provider_id: "openai".to_string(),
            name: "o3".to_string(),
            family: Some("o3".to_string()),
            capabilities: ModelCapabilities {
                temperature: false,
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
                interleaved: true,
            },
            cost: ModelCost {
                input: 10.0,
                output: 40.0,
                cache_read: 5.0,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 100_000,
            },
            status: ModelStatus::Active,
        }
    }

    /// O3 mini - Small model alternative to o3.
    pub fn o3_mini() -> ModelInfo {
        ModelInfo {
            id: "o3-mini".to_string(),
            provider_id: "openai".to_string(),
            name: "o3-mini".to_string(),
            family: Some("o3".to_string()),
            capabilities: ModelCapabilities {
                temperature: false,
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
                interleaved: true,
            },
            cost: ModelCost {
                input: 1.1,
                output: 4.4,
                cache_read: 0.55,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 100_000,
            },
            status: ModelStatus::Active,
        }
    }

    /// O4 mini - Fast, cost-efficient reasoning model.
    pub fn o4_mini() -> ModelInfo {
        ModelInfo {
            id: "o4-mini".to_string(),
            provider_id: "openai".to_string(),
            name: "o4-mini".to_string(),
            family: Some("o4".to_string()),
            capabilities: ModelCapabilities {
                temperature: false,
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
                interleaved: true,
            },
            cost: ModelCost {
                input: 1.1,
                output: 4.4,
                cache_read: 0.55,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 100_000,
            },
            status: ModelStatus::Active,
        }
    }

    // ==================== Legacy Models ====================

    /// GPT-4o - Fast, intelligent, flexible GPT model.
    pub fn gpt_4o() -> ModelInfo {
        ModelInfo {
            id: "gpt-4o".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-4o".to_string(),
            family: Some("gpt-4".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: false,
                    pdf: false,
                },
                output: ModalitySupport {
                    text: true,
                    audio: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 2.5,
                output: 10.0,
                cache_read: 1.25,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 128_000,
                output: 16_384,
            },
            status: ModelStatus::Active,
        }
    }

    /// GPT-4o mini - Fast, affordable small model.
    pub fn gpt_4o_mini() -> ModelInfo {
        ModelInfo {
            id: "gpt-4o-mini".to_string(),
            provider_id: "openai".to_string(),
            name: "GPT-4o mini".to_string(),
            family: Some("gpt-4".to_string()),
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
                input: 0.15,
                output: 0.6,
                cache_read: 0.075,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 128_000,
                output: 16_384,
            },
            status: ModelStatus::Active,
        }
    }

    /// O1 reasoning model (legacy).
    pub fn o1() -> ModelInfo {
        ModelInfo {
            id: "o1".to_string(),
            provider_id: "openai".to_string(),
            name: "o1".to_string(),
            family: Some("o1".to_string()),
            capabilities: ModelCapabilities {
                temperature: false,
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
                interleaved: true,
            },
            cost: ModelCost {
                input: 15.0,
                output: 60.0,
                cache_read: 7.5,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 200_000,
                output: 100_000,
            },
            status: ModelStatus::Active,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_calculation() {
        let cost = ModelCost {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_write: 3.75,
        };

        // 1000 input, 500 output
        let total = cost.calculate(1000, 500);
        assert!((total - 0.0105).abs() < 0.0001);
    }

    #[test]
    fn test_model_builder() {
        let model = ModelInfo::new("test-model", "test-provider")
            .with_name("Test Model")
            .with_limit(ModelLimit {
                context: 8000,
                output: 2000,
            });

        assert_eq!(model.id, "test-model");
        assert_eq!(model.provider_id, "test-provider");
        assert_eq!(model.name, "Test Model");
        assert_eq!(model.limit.context, 8000);
    }

    #[test]
    fn test_builtin_models() {
        let claude = anthropic::claude_sonnet_4();
        assert_eq!(claude.provider_id, "anthropic");
        assert!(claude.capabilities.reasoning);

        let gpt = openai::gpt_4o();
        assert_eq!(gpt.provider_id, "openai");
        assert!(!gpt.capabilities.reasoning);
    }
}

/// Built-in model definitions for Google.
pub mod google {
    use super::*;

    /// Gemini 2.0 Flash.
    pub fn gemini_2_flash() -> ModelInfo {
        ModelInfo {
            id: "gemini-2.0-flash".to_string(),
            provider_id: "google".to_string(),
            name: "Gemini 2.0 Flash".to_string(),
            family: Some("gemini-2".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: true,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.1, // Very affordable
                output: 0.4,
                cache_read: 0.025,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 1_000_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Gemini 1.5 Pro.
    pub fn gemini_1_5_pro() -> ModelInfo {
        ModelInfo {
            id: "gemini-1.5-pro".to_string(),
            provider_id: "google".to_string(),
            name: "Gemini 1.5 Pro".to_string(),
            family: Some("gemini-1.5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: true,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 1.25,
                output: 5.0,
                cache_read: 0.3125,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 2_000_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }

    /// Gemini 1.5 Flash.
    pub fn gemini_1_5_flash() -> ModelInfo {
        ModelInfo {
            id: "gemini-1.5-flash".to_string(),
            provider_id: "google".to_string(),
            name: "Gemini 1.5 Flash".to_string(),
            family: Some("gemini-1.5".to_string()),
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: false,
                attachment: true,
                tool_call: true,
                input: ModalitySupport {
                    text: true,
                    image: true,
                    audio: true,
                    video: true,
                    pdf: true,
                },
                output: ModalitySupport {
                    text: true,
                    ..Default::default()
                },
                interleaved: false,
            },
            cost: ModelCost {
                input: 0.075,
                output: 0.3,
                cache_read: 0.01875,
                cache_write: 0.0,
            },
            limit: ModelLimit {
                context: 1_000_000,
                output: 8_192,
            },
            status: ModelStatus::Active,
        }
    }
}
