//! Centralized provider and model registry.
//!
//! This module provides a single source of truth for all supported AI providers
//! and models. Instead of having provider/model definitions scattered across
//! multiple files, everything is defined here.
//!
//! ## Usage
//!
//! ```rust
//! use wonopcode_provider::registry::{get_provider, get_model_info, PROVIDERS};
//!
//! // Get provider info
//! let anthropic = get_provider("anthropic");
//!
//! // Get model info
//! let model = get_model_info("claude-sonnet-4-5", "anthropic");
//!
//! // Iterate all providers
//! for provider in PROVIDERS.iter() {
//!     println!("{}: {}", provider.id, provider.name);
//! }
//! ```

use crate::model::{ModelCapabilities, ModelCost, ModelInfo, ModelLimit, ModelStatus};

// ============================================================================
// Provider Registry
// ============================================================================

/// Information about an AI provider.
#[derive(Debug, Clone)]
pub struct ProviderDefinition {
    /// Unique provider ID (e.g., "anthropic", "openai").
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Environment variables for authentication.
    pub env_vars: &'static [&'static str],
    /// Parent provider ID (for CLI variants that share models with API providers).
    pub parent: Option<&'static str>,
    /// Whether this provider requires CLI tooling instead of API.
    pub is_cli: bool,
    /// Display name suffix (e.g., " (CLI)" for CLI variants).
    pub name_suffix: &'static str,
    /// Default model ID for this provider.
    pub default_model: &'static str,
}

impl ProviderDefinition {
    /// Create a new provider definition.
    pub const fn new(id: &'static str, name: &'static str) -> Self {
        Self {
            id,
            name,
            env_vars: &[],
            parent: None,
            is_cli: false,
            name_suffix: "",
            default_model: "",
        }
    }

    /// Add environment variables.
    pub const fn with_env(mut self, env_vars: &'static [&'static str]) -> Self {
        self.env_vars = env_vars;
        self
    }

    /// Set parent provider (for CLI variants).
    pub const fn with_parent(mut self, parent: &'static str) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Mark as CLI-based provider.
    pub const fn cli(mut self) -> Self {
        self.is_cli = true;
        self
    }

    /// Set name suffix.
    pub const fn with_suffix(mut self, suffix: &'static str) -> Self {
        self.name_suffix = suffix;
        self
    }

    /// Set default model.
    pub const fn with_default_model(mut self, model: &'static str) -> Self {
        self.default_model = model;
        self
    }
}

/// All supported providers.
/// 
/// The 4 core providers:
/// - `anthropic` - Anthropic API (requires API key)
/// - `anthropic-cli` - Claude CLI subscription (requires claude CLI)
/// - `openai` - OpenAI API (requires API key)
/// - `openai-codex` - OpenAI Codex (API key or ChatGPT subscription)
pub static PROVIDERS: &[ProviderDefinition] = &[
    ProviderDefinition::new("anthropic", "Anthropic API")
        .with_env(&["ANTHROPIC_API_KEY"])
        .with_default_model("claude-sonnet-4-5-20250929"),
    
    ProviderDefinition::new("anthropic-cli", "Claude CLI (Subscription)")
        .with_parent("anthropic")
        .cli()
        .with_suffix(" (CLI)")
        .with_default_model("claude-sonnet-4-5-20250929"),
    
    ProviderDefinition::new("openai", "OpenAI API")
        .with_env(&["OPENAI_API_KEY"])
        .with_default_model("gpt-4o"),
    
    ProviderDefinition::new("openai-codex", "OpenAI Codex")
        .with_env(&["OPENAI_API_KEY"])
        .with_default_model("codex"),
    
    // Test provider for development/UI testing
    ProviderDefinition::new("test", "Test Provider")
        .with_default_model("test-128b"),
];

/// Get a provider by ID.
pub fn get_provider(id: &str) -> Option<&'static ProviderDefinition> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Get all provider IDs.
pub fn provider_ids() -> impl Iterator<Item = &'static str> {
    PROVIDERS.iter().map(|p| p.id)
}

/// Get the primary (non-CLI) providers.
pub fn primary_providers() -> impl Iterator<Item = &'static ProviderDefinition> {
    PROVIDERS.iter().filter(|p| !p.is_cli && p.id != "test")
}

/// Get the environment variable name for a provider's API key.
pub fn get_api_key_env_var(provider_id: &str) -> Option<&'static str> {
    get_provider(provider_id)
        .and_then(|p| p.env_vars.first())
        .copied()
}

// ============================================================================
// Model Registry
// ============================================================================

/// A model definition with aliases.
#[derive(Debug, Clone)]
pub struct ModelDefinition {
    /// Primary model ID (canonical).
    pub id: &'static str,
    /// Provider ID this model belongs to.
    pub provider: &'static str,
    /// Aliases that resolve to this model.
    pub aliases: &'static [&'static str],
    /// Factory function to create ModelInfo.
    pub info_fn: fn() -> ModelInfo,
}

/// All built-in model definitions.
/// 
/// Models are defined here with their canonical ID and aliases.
/// The `info_fn` returns the full ModelInfo with capabilities, costs, etc.
pub static MODELS: &[ModelDefinition] = &[
    // ========== Anthropic - Claude 4.6 (Latest) ==========
    ModelDefinition {
        id: "claude-opus-4-6",
        provider: "anthropic",
        aliases: &[],
        info_fn: crate::model::anthropic::claude_opus_4_6,
    },
    ModelDefinition {
        id: "claude-sonnet-4-6",
        provider: "anthropic",
        aliases: &[],
        info_fn: crate::model::anthropic::claude_sonnet_4_6,
    },
    
    // ========== Anthropic - Claude 4.5 ==========
    ModelDefinition {
        id: "claude-sonnet-4-5-20250929",
        provider: "anthropic",
        aliases: &["claude-sonnet-4-5"],
        info_fn: crate::model::anthropic::claude_sonnet_4_5,
    },
    ModelDefinition {
        id: "claude-haiku-4-5-20251001",
        provider: "anthropic",
        aliases: &["claude-haiku-4-5"],
        info_fn: crate::model::anthropic::claude_haiku_4_5,
    },
    ModelDefinition {
        id: "claude-opus-4-5-20251101",
        provider: "anthropic",
        aliases: &["claude-opus-4-5"],
        info_fn: crate::model::anthropic::claude_opus_4_5,
    },
    
    // ========== Anthropic - Claude 4.x (Legacy) ==========
    ModelDefinition {
        id: "claude-sonnet-4-20250514",
        provider: "anthropic",
        aliases: &["claude-sonnet-4-0", "claude-sonnet-4"],
        info_fn: crate::model::anthropic::claude_sonnet_4,
    },
    ModelDefinition {
        id: "claude-opus-4-1-20250805",
        provider: "anthropic",
        aliases: &["claude-opus-4-1"],
        info_fn: crate::model::anthropic::claude_opus_4_1,
    },
    ModelDefinition {
        id: "claude-opus-4-20250514",
        provider: "anthropic",
        aliases: &["claude-opus-4-0", "claude-opus-4"],
        info_fn: crate::model::anthropic::claude_opus_4,
    },
    
    // ========== Anthropic - Claude 3.x (Legacy) ==========
    ModelDefinition {
        id: "claude-3-7-sonnet-20250219",
        provider: "anthropic",
        aliases: &["claude-3-7-sonnet", "claude-3-7-sonnet-latest"],
        info_fn: crate::model::anthropic::claude_sonnet_3_7,
    },
    ModelDefinition {
        id: "claude-3-haiku-20240307",
        provider: "anthropic",
        aliases: &["claude-3-haiku"],
        info_fn: crate::model::anthropic::claude_haiku_3,
    },
    
    // ========== OpenAI - GPT-5 Series ==========
    ModelDefinition {
        id: "gpt-5.2",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_5_2,
    },
    ModelDefinition {
        id: "gpt-5.1",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_5_1,
    },
    ModelDefinition {
        id: "gpt-5",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_5,
    },
    ModelDefinition {
        id: "gpt-5-mini",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_5_mini,
    },
    ModelDefinition {
        id: "gpt-5-nano",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_5_nano,
    },
    
    // ========== OpenAI - GPT-4.1 Series ==========
    ModelDefinition {
        id: "gpt-4.1",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_4_1,
    },
    ModelDefinition {
        id: "gpt-4.1-mini",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_4_1_mini,
    },
    ModelDefinition {
        id: "gpt-4.1-nano",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_4_1_nano,
    },
    
    // ========== OpenAI - O-Series ==========
    ModelDefinition {
        id: "o3",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::o3,
    },
    ModelDefinition {
        id: "o3-mini",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::o3_mini,
    },
    ModelDefinition {
        id: "o4-mini",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::o4_mini,
    },
    
    // ========== OpenAI - Legacy ==========
    ModelDefinition {
        id: "gpt-4o",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_4o,
    },
    ModelDefinition {
        id: "gpt-4o-mini",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::gpt_4o_mini,
    },
    ModelDefinition {
        id: "o1",
        provider: "openai",
        aliases: &[],
        info_fn: crate::model::openai::o1,
    },
    
    // ========== OpenAI Codex ==========
    ModelDefinition {
        id: "codex",
        provider: "openai-codex",
        aliases: &["codex-mini"],
        info_fn: crate::codex::models::codex,
    },
    // Codex can also use o3 and o4-mini through the Responses API
    ModelDefinition {
        id: "codex-o3",
        provider: "openai-codex",
        aliases: &[],
        info_fn: crate::codex::models::o3,
    },
    ModelDefinition {
        id: "codex-o4-mini",
        provider: "openai-codex",
        aliases: &[],
        info_fn: crate::codex::models::o4_mini,
    },
    
    // ========== Test Provider ==========
    ModelDefinition {
        id: "test-128b",
        provider: "test",
        aliases: &[],
        info_fn: crate::test::TestProvider::test_128b,
    },
];

/// Find a model definition by ID or alias.
pub fn find_model(model_id: &str) -> Option<&'static ModelDefinition> {
    MODELS.iter().find(|m| {
        m.id == model_id || m.aliases.contains(&model_id)
    })
}

/// Find a model definition by ID or alias, with provider context.
/// 
/// Some models (like "o3") exist in multiple providers (openai, openai-codex).
/// The provider context helps resolve the correct one.
pub fn find_model_for_provider(model_id: &str, provider: &str) -> Option<&'static ModelDefinition> {
    // For openai-codex provider, check for codex-specific models first
    if provider == "openai-codex" {
        // Map o3/o4-mini to codex variants when using codex provider
        let codex_id = match model_id {
            "o3" => "codex-o3",
            "o4-mini" => "codex-o4-mini",
            _ => model_id,
        };
        if let Some(m) = MODELS.iter().find(|m| {
            m.provider == "openai-codex" && (m.id == codex_id || m.aliases.contains(&codex_id))
        }) {
            return Some(m);
        }
    }
    
    // First try to find a model that matches the provider exactly
    if let Some(m) = MODELS.iter().find(|m| {
        m.provider == provider && (m.id == model_id || m.aliases.contains(&model_id))
    }) {
        return Some(m);
    }
    
    // For CLI providers, check the parent provider
    if let Some(provider_def) = get_provider(provider) {
        if let Some(parent) = provider_def.parent {
            if let Some(m) = MODELS.iter().find(|m| {
                m.provider == parent && (m.id == model_id || m.aliases.contains(&model_id))
            }) {
                return Some(m);
            }
        }
    }
    
    // Fall back to any matching model
    find_model(model_id)
}

/// Get ModelInfo for a model ID and provider.
/// 
/// This is the main entry point for getting model information.
/// It handles aliases, provider inheritance (CLI providers), and fallbacks.
pub fn get_model_info(model_id: &str, provider: &str) -> ModelInfo {
    if let Some(model_def) = find_model_for_provider(model_id, provider) {
        let mut info = (model_def.info_fn)();
        
        // For CLI providers, update the provider_id but keep other info
        if let Some(provider_def) = get_provider(provider) {
            if provider_def.parent.is_some() {
                info.provider_id = provider.to_string();
                // Add suffix to name if defined
                if !provider_def.name_suffix.is_empty() {
                    info.name = format!("{}{}", info.name, provider_def.name_suffix);
                }
            }
        }
        
        return info;
    }
    
    // Fallback for unknown models
    build_fallback_model_info(model_id, provider)
}

/// Build a reasonable fallback ModelInfo for unknown models.
fn build_fallback_model_info(model_id: &str, provider: &str) -> ModelInfo {
    let (context, output) = match provider {
        "anthropic" | "anthropic-cli" => (200_000, 8_192),
        "openai" | "openai-codex" => (128_000, 16_384),
        _ => (32_000, 4_096),
    };
    
    ModelInfo {
        id: model_id.to_string(),
        provider_id: provider.to_string(),
        name: model_id.to_string(),
        family: None,
        capabilities: ModelCapabilities::default(),
        cost: ModelCost::default(),
        limit: ModelLimit { context, output },
        status: ModelStatus::Active,
    }
}

/// Infer the provider from a model ID.
/// 
/// This is used when only a model name is provided without an explicit provider.
pub fn infer_provider(model_id: &str) -> &'static str {
    let model_lower = model_id.to_lowercase();
    
    // OpenAI Codex models
    if model_lower == "codex" || model_lower.starts_with("codex-") {
        return "openai-codex";
    }
    
    // Anthropic models
    if model_lower.starts_with("claude") {
        return "anthropic";
    }
    
    // OpenAI models
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.starts_with("chatgpt")
    {
        return "openai";
    }
    
    // Test provider
    if model_lower.starts_with("test-") {
        return "test";
    }
    
    // Default to anthropic
    "anthropic"
}

/// Check if a model is compatible with a provider.
pub fn is_model_compatible(model_id: &str, provider: &str) -> bool {
    // Check if the model directly belongs to this provider
    if let Some(model_def) = find_model(model_id) {
        if model_def.provider == provider {
            return true;
        }
    }
    
    // Check if this is a CLI provider and the model belongs to the parent
    if let Some(provider_def) = get_provider(provider) {
        if let Some(parent) = provider_def.parent {
            if let Some(model_def) = find_model(model_id) {
                return model_def.provider == parent;
            }
        }
    }
    
    false
}

/// Get all models for a provider.
/// 
/// For CLI providers, this includes models from the parent provider.
pub fn get_models_for_provider(provider: &str) -> Vec<&'static ModelDefinition> {
    let provider_def = get_provider(provider);
    let parent = provider_def.and_then(|p| p.parent);
    
    MODELS
        .iter()
        .filter(|m| m.provider == provider || parent.map_or(false, |p| m.provider == p))
        .collect()
}

/// Get the default model for a provider.
pub fn get_default_model(provider: &str) -> &'static str {
    get_provider(provider)
        .map(|p| p.default_model)
        .unwrap_or("claude-sonnet-4-5-20250929")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_provider() {
        let anthropic = get_provider("anthropic").unwrap();
        assert_eq!(anthropic.id, "anthropic");
        assert_eq!(anthropic.name, "Anthropic API");
        assert!(anthropic.env_vars.contains(&"ANTHROPIC_API_KEY"));
        
        let cli = get_provider("anthropic-cli").unwrap();
        assert_eq!(cli.parent, Some("anthropic"));
        assert!(cli.is_cli);
    }

    #[test]
    fn test_find_model() {
        // Primary ID
        let model = find_model("claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(model.id, "claude-sonnet-4-5-20250929");
        
        // Alias
        let model = find_model("claude-sonnet-4-5").unwrap();
        assert_eq!(model.id, "claude-sonnet-4-5-20250929");
    }

    #[test]
    fn test_get_model_info() {
        let info = get_model_info("claude-sonnet-4-5", "anthropic");
        assert_eq!(info.id, "claude-sonnet-4-5-20250929");
        assert_eq!(info.provider_id, "anthropic");
        
        // CLI provider should inherit from parent
        let cli_info = get_model_info("claude-sonnet-4-5", "anthropic-cli");
        assert_eq!(cli_info.id, "claude-sonnet-4-5-20250929");
        assert_eq!(cli_info.provider_id, "anthropic-cli");
        assert!(cli_info.name.contains("(CLI)"));
    }

    #[test]
    fn test_infer_provider() {
        assert_eq!(infer_provider("claude-sonnet-4-5"), "anthropic");
        assert_eq!(infer_provider("gpt-4o"), "openai");
        assert_eq!(infer_provider("o3"), "openai");
        assert_eq!(infer_provider("codex"), "openai-codex");
        assert_eq!(infer_provider("test-128b"), "test");
    }

    #[test]
    fn test_is_model_compatible() {
        // Direct compatibility
        assert!(is_model_compatible("claude-sonnet-4-5", "anthropic"));
        assert!(is_model_compatible("gpt-4o", "openai"));
        
        // CLI inherits from parent
        assert!(is_model_compatible("claude-sonnet-4-5", "anthropic-cli"));
        
        // Cross-provider is not compatible
        assert!(!is_model_compatible("claude-sonnet-4-5", "openai"));
    }

    #[test]
    fn test_get_models_for_provider() {
        let anthropic_models = get_models_for_provider("anthropic");
        assert!(anthropic_models.iter().any(|m| m.id == "claude-sonnet-4-5-20250929"));
        
        // CLI should include parent models
        let cli_models = get_models_for_provider("anthropic-cli");
        assert!(cli_models.iter().any(|m| m.id == "claude-sonnet-4-5-20250929"));
    }

    #[test]
    fn test_codex_models() {
        // Codex has its own models
        let codex = find_model("codex").unwrap();
        assert_eq!(codex.provider, "openai-codex");
        
        // And o3/o4-mini variants for codex
        let info = get_model_info("o3", "openai-codex");
        assert_eq!(info.provider_id, "openai-codex");
    }

    #[test]
    fn test_fallback_model_info() {
        let info = get_model_info("unknown-model", "anthropic");
        assert_eq!(info.id, "unknown-model");
        assert_eq!(info.provider_id, "anthropic");
        assert_eq!(info.limit.context, 200_000);
    }
}
