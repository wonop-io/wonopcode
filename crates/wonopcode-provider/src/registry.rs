//! Centralized provider and model registry.
//!
//! This module provides a single source of truth for all providers and models
//! supported by the application. Instead of duplicating definitions across
//! multiple files (runner.rs, processor.rs, prompt.rs, routes.rs, etc.),
//! all consumers should use this registry.
//!
//! # Usage
//!
//! ```rust
//! use wonopcode_provider::registry;
//!
//! // Get a provider definition
//! if let Some(provider) = registry::get_provider("anthropic") {
//!     println!("Provider: {}", provider.name);
//! }
//!
//! // Get model info
//! let model = registry::get_model_info("claude-sonnet-4-5-20250929", "anthropic");
//!
//! // Infer provider from model name
//! let provider_id = registry::infer_provider("claude-sonnet-4-5-20250929");
//!
//! // Get providers that need API key settings
//! let api_key_providers = registry::providers_needing_api_key();
//! ```

use crate::model::ModelInfo;

// =============================================================================
// Authentication Types
// =============================================================================

/// Authentication methods supported by providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethodType {
    /// API key authentication (most common)
    ApiKey,
    /// CLI-based authentication (e.g., Claude CLI)
    Cli,
    /// OAuth device code flow (e.g., ChatGPT subscription)
    OAuthDeviceCode,
}

/// Configuration for API key authentication.
#[derive(Debug, Clone, Copy)]
pub struct ApiKeyConfig {
    /// Environment variable name (e.g., "ANTHROPIC_API_KEY")
    pub env_var: &'static str,
    /// Credentials manager key name (e.g., "anthropic")
    pub creds_key: &'static str,
    /// Expected key prefix for validation (e.g., "sk-ant-")
    pub prefix: Option<&'static str>,
    /// UI hint text (e.g., "sk-ant-...")
    pub hint: &'static str,
    /// Header name for API requests (e.g., "x-api-key", "Authorization")
    pub header_name: &'static str,
    /// Header format - use {} as placeholder for key (e.g., "Bearer {}" or "{}")
    pub header_format: &'static str,
}

impl ApiKeyConfig {
    /// Create a new API key config with defaults.
    pub const fn new(env_var: &'static str, creds_key: &'static str) -> Self {
        Self {
            env_var,
            creds_key,
            prefix: None,
            hint: "Enter your API key",
            header_name: "Authorization",
            header_format: "Bearer {}",
        }
    }

    /// Set expected key prefix.
    pub const fn with_prefix(mut self, prefix: &'static str) -> Self {
        self.prefix = Some(prefix);
        self
    }

    /// Set UI hint text.
    pub const fn with_hint(mut self, hint: &'static str) -> Self {
        self.hint = hint;
        self
    }

    /// Set custom header name.
    pub const fn with_header(mut self, name: &'static str, format: &'static str) -> Self {
        self.header_name = name;
        self.header_format = format;
        self
    }

    /// Format the authorization header value.
    pub fn format_header(&self, api_key: &str) -> String {
        self.header_format.replace("{}", api_key)
    }
}

/// Configuration for CLI authentication.
#[derive(Debug, Clone, Copy)]
pub struct CliConfig {
    /// CLI binary name (e.g., "claude", "codex")
    pub binary_name: &'static str,
    /// Directory where CLI stores auth (e.g., ".claude", ".codex")
    pub auth_dir: &'static str,
    /// Install command/instructions
    pub install_command: &'static str,
    /// Login command
    pub login_command: &'static str,
}

impl CliConfig {
    /// Create a new CLI config.
    pub const fn new(
        binary_name: &'static str,
        auth_dir: &'static str,
        install_command: &'static str,
        login_command: &'static str,
    ) -> Self {
        Self {
            binary_name,
            auth_dir,
            install_command,
            login_command,
        }
    }
}

/// Configuration for testing provider connectivity.
#[derive(Debug, Clone, Copy)]
pub struct TestConfig {
    /// Test endpoint URL
    pub endpoint: &'static str,
    /// HTTP method (GET or POST)
    pub method: &'static str,
    /// Additional headers (as key-value pairs)
    pub headers: &'static [(&'static str, &'static str)],
    /// Request body for POST (if needed)
    pub body: Option<&'static str>,
}

impl TestConfig {
    /// Create a new test config for GET request.
    pub const fn get(endpoint: &'static str) -> Self {
        Self {
            endpoint,
            method: "GET",
            headers: &[],
            body: None,
        }
    }

    /// Create a new test config for POST request.
    pub const fn post(endpoint: &'static str, body: &'static str) -> Self {
        Self {
            endpoint,
            method: "POST",
            headers: &[],
            body: Some(body),
        }
    }

    /// Add custom headers.
    pub const fn with_headers(mut self, headers: &'static [(&'static str, &'static str)]) -> Self {
        self.headers = headers;
        self
    }
}

// =============================================================================
// Provider Definition
// =============================================================================

/// Provider definition with metadata and settings configuration.
#[derive(Debug, Clone)]
pub struct ProviderDefinition {
    /// Unique provider ID (e.g., "anthropic", "openai-codex")
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// Suffix to add to model names for this provider (e.g., " (CLI)")
    pub name_suffix: &'static str,
    /// Environment variables for API key (first found is used) - DEPRECATED, use api_key.env_var
    pub env_vars: &'static [&'static str],
    /// Parent provider ID (for CLI variants that share models)
    pub parent: Option<&'static str>,
    /// Default model ID for this provider
    pub default_model: &'static str,

    // === Authentication Settings ===
    /// Authentication methods supported by this provider
    pub auth_methods: &'static [AuthMethodType],
    /// API key configuration (if ApiKey auth is supported)
    pub api_key: Option<ApiKeyConfig>,
    /// CLI configuration (if CLI auth is supported)
    pub cli: Option<CliConfig>,

    // === Testing Configuration ===
    /// Configuration for testing provider connectivity
    pub test: Option<TestConfig>,

    // === Setup Information ===
    /// Setup instructions shown in UI
    pub setup_instructions: &'static str,
    /// Documentation URL
    pub docs_url: Option<&'static str>,
}

impl ProviderDefinition {
    /// Create a new provider definition.
    pub const fn new(id: &'static str, name: &'static str) -> Self {
        Self {
            id,
            name,
            name_suffix: "",
            env_vars: &[],
            parent: None,
            default_model: "",
            auth_methods: &[],
            api_key: None,
            cli: None,
            test: None,
            setup_instructions: "",
            docs_url: None,
        }
    }

    /// Set name suffix for models (e.g., " (CLI)").
    pub const fn with_name_suffix(mut self, suffix: &'static str) -> Self {
        self.name_suffix = suffix;
        self
    }

    /// Set environment variables for API key lookup.
    /// DEPRECATED: Use with_api_key instead.
    pub const fn with_env(mut self, env_vars: &'static [&'static str]) -> Self {
        self.env_vars = env_vars;
        self
    }

    /// Set parent provider (for CLI variants that share models).
    pub const fn with_parent(mut self, parent: &'static str) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Set default model ID.
    pub const fn with_default_model(mut self, model: &'static str) -> Self {
        self.default_model = model;
        self
    }

    /// Set authentication methods.
    pub const fn with_auth_methods(mut self, methods: &'static [AuthMethodType]) -> Self {
        self.auth_methods = methods;
        self
    }

    /// Set API key configuration.
    pub const fn with_api_key(mut self, config: ApiKeyConfig) -> Self {
        self.api_key = Some(config);
        self
    }

    /// Set CLI configuration.
    pub const fn with_cli(mut self, config: CliConfig) -> Self {
        self.cli = Some(config);
        self
    }

    /// Set test configuration.
    pub const fn with_test(mut self, config: TestConfig) -> Self {
        self.test = Some(config);
        self
    }

    /// Set setup instructions.
    pub const fn with_setup_instructions(mut self, instructions: &'static str) -> Self {
        self.setup_instructions = instructions;
        self
    }

    /// Set documentation URL.
    pub const fn with_docs_url(mut self, url: &'static str) -> Self {
        self.docs_url = Some(url);
        self
    }

    /// Check if this provider supports API key authentication.
    pub fn supports_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Check if this provider supports CLI authentication.
    pub fn supports_cli(&self) -> bool {
        self.cli.is_some()
    }

    /// Get the credentials manager key for this provider.
    pub fn creds_key(&self) -> Option<&'static str> {
        self.api_key.map(|k| k.creds_key)
    }
}

/// Model definition in the registry.
#[derive(Debug, Clone)]
pub struct ModelDefinition {
    /// Model ID (e.g., "claude-sonnet-4-5-20250929")
    pub id: &'static str,
    /// Provider ID this model belongs to
    pub provider_id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// Aliases for this model (for backward compatibility)
    pub aliases: &'static [&'static str],
    /// Function to create the full ModelInfo
    pub info_fn: fn() -> ModelInfo,
}

impl ModelDefinition {
    /// Create a new model definition.
    pub const fn new(
        id: &'static str,
        provider_id: &'static str,
        name: &'static str,
        info_fn: fn() -> ModelInfo,
    ) -> Self {
        Self {
            id,
            provider_id,
            name,
            aliases: &[],
            info_fn,
        }
    }

    /// Add aliases for this model.
    pub const fn with_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }
}

// =============================================================================
// Provider Registry
// =============================================================================

// Test request body for Anthropic API
const ANTHROPIC_TEST_BODY: &str = r#"{"model":"claude-3-haiku-20240307","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#;

// Test request body for OpenAI API
const OPENAI_TEST_BODY: &str = r#"{"model":"gpt-4o-mini","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#;

/// All supported providers.
pub static PROVIDERS: &[ProviderDefinition] = &[
    // =========================================================================
    // Anthropic API
    // =========================================================================
    ProviderDefinition::new("anthropic", "Anthropic API")
        .with_env(&["ANTHROPIC_API_KEY"])
        .with_default_model("claude-sonnet-4-5-20250929")
        .with_auth_methods(&[AuthMethodType::ApiKey])
        .with_api_key(
            ApiKeyConfig::new("ANTHROPIC_API_KEY", "anthropic")
                .with_prefix("sk-ant-")
                .with_hint("sk-ant-...")
                .with_header("x-api-key", "{}"),
        )
        .with_test(
            TestConfig::post("https://api.anthropic.com/v1/messages", ANTHROPIC_TEST_BODY)
                .with_headers(&[
                    ("anthropic-version", "2023-06-01"),
                    ("content-type", "application/json"),
                ]),
        )
        .with_setup_instructions("Enter your Anthropic API key or use Claude CLI")
        .with_docs_url("https://console.anthropic.com/"),

    // =========================================================================
    // Anthropic CLI (Claude subscription via CLI)
    // =========================================================================
    ProviderDefinition::new("anthropic-cli", "Claude CLI (Subscription)")
        .with_name_suffix(" (CLI)")
        .with_parent("anthropic")
        .with_default_model("claude-sonnet-4-5-20250929")
        .with_auth_methods(&[AuthMethodType::Cli])
        .with_cli(CliConfig::new(
            "claude",
            ".claude",
            "npm install -g @anthropic-ai/claude-code",
            "claude login",
        ))
        .with_setup_instructions("Install and authenticate the Claude CLI")
        .with_docs_url("https://docs.anthropic.com/claude-code"),

    // =========================================================================
    // OpenAI API
    // =========================================================================
    ProviderDefinition::new("openai", "OpenAI API")
        .with_env(&["OPENAI_API_KEY"])
        .with_default_model("gpt-4o")
        .with_auth_methods(&[AuthMethodType::ApiKey])
        .with_api_key(
            ApiKeyConfig::new("OPENAI_API_KEY", "openai")
                .with_prefix("sk-")
                .with_hint("sk-..."),
        )
        .with_test(
            TestConfig::post("https://api.openai.com/v1/chat/completions", OPENAI_TEST_BODY)
                .with_headers(&[("content-type", "application/json")]),
        )
        .with_setup_instructions("Enter your OpenAI API key")
        .with_docs_url("https://platform.openai.com/api-keys"),

    // =========================================================================
    // OpenAI Codex (Responses API - ChatGPT subscription or API key)
    // =========================================================================
    ProviderDefinition::new("openai-codex", "OpenAI Codex")
        .with_env(&["CODEX_API_KEY", "OPENAI_API_KEY"]) // CODEX_API_KEY takes precedence
        .with_default_model("codex")
        .with_auth_methods(&[AuthMethodType::ApiKey, AuthMethodType::OAuthDeviceCode])
        .with_api_key(
            ApiKeyConfig::new("OPENAI_API_KEY", "openai")
                .with_prefix("sk-")
                .with_hint("sk-..."),
        )
        .with_cli(CliConfig::new(
            "codex",
            ".codex",
            "npm install -g @openai/codex",
            "codex login",
        ))
        .with_setup_instructions("Use Codex CLI with ChatGPT subscription or enter OpenAI API key")
        .with_docs_url("https://openai.com/codex"),

    // =========================================================================
    // Compound Coders
    // =========================================================================
    ProviderDefinition::new("compoundcoders", "Compound Coders")
        .with_env(&["COMPOUNDCODERS_API_KEY"])
        .with_default_model("wonop/gpt")
        .with_auth_methods(&[AuthMethodType::ApiKey])
        .with_api_key(
            ApiKeyConfig::new("COMPOUNDCODERS_API_KEY", "compoundcoders")
                .with_hint("Enter your API key"),
        )
        .with_test(TestConfig::get("https://api.compoundcoders.com/v1/models"))
        .with_setup_instructions("Enter your Compound Coders API key")
        .with_docs_url("https://compoundcoders.com"),

    // =========================================================================
    // Test provider (for unit tests)
    // =========================================================================
    ProviderDefinition::new("test", "Test Provider")
        .with_default_model("test-model"),
];

// =============================================================================
// Model Registry
// =============================================================================

/// All supported models.
pub static MODELS: &[ModelDefinition] = &[
    // =========================================================================
    // Anthropic Models - Claude 4.6 (Latest)
    // =========================================================================
    ModelDefinition::new(
        "claude-opus-4-6",
        "anthropic",
        "Claude Opus 4.6",
        crate::model::anthropic::claude_opus_4_6,
    ),
    ModelDefinition::new(
        "claude-sonnet-4-6",
        "anthropic",
        "Claude Sonnet 4.6",
        crate::model::anthropic::claude_sonnet_4_6,
    ),
    // =========================================================================
    // Anthropic Models - Claude 4.5 (Current)
    // =========================================================================
    ModelDefinition::new(
        "claude-sonnet-4-5-20250929",
        "anthropic",
        "Claude Sonnet 4.5",
        crate::model::anthropic::claude_sonnet_4_5,
    )
    .with_aliases(&["claude-sonnet-4-5"]),
    ModelDefinition::new(
        "claude-haiku-4-5-20251001",
        "anthropic",
        "Claude Haiku 4.5",
        crate::model::anthropic::claude_haiku_4_5,
    )
    .with_aliases(&["claude-haiku-4-5"]),
    ModelDefinition::new(
        "claude-opus-4-5-20251101",
        "anthropic",
        "Claude Opus 4.5",
        crate::model::anthropic::claude_opus_4_5,
    )
    .with_aliases(&["claude-opus-4-5"]),
    // =========================================================================
    // Anthropic Models - Claude 4.x (Legacy)
    // =========================================================================
    ModelDefinition::new(
        "claude-sonnet-4-20250514",
        "anthropic",
        "Claude Sonnet 4",
        crate::model::anthropic::claude_sonnet_4,
    )
    .with_aliases(&["claude-sonnet-4"]),
    ModelDefinition::new(
        "claude-opus-4-1-20250805",
        "anthropic",
        "Claude Opus 4.1",
        crate::model::anthropic::claude_opus_4_1,
    )
    .with_aliases(&["claude-opus-4-1"]),
    ModelDefinition::new(
        "claude-opus-4-20250514",
        "anthropic",
        "Claude Opus 4",
        crate::model::anthropic::claude_opus_4,
    )
    .with_aliases(&["claude-opus-4"]),
    // =========================================================================
    // Anthropic Models - Claude 3.x (Legacy)
    // =========================================================================
    ModelDefinition::new(
        "claude-3-7-sonnet-20250219",
        "anthropic",
        "Claude 3.7 Sonnet",
        crate::model::anthropic::claude_sonnet_3_7,
    )
    .with_aliases(&["claude-3-7-sonnet"]),
    ModelDefinition::new(
        "claude-3-haiku-20240307",
        "anthropic",
        "Claude 3 Haiku",
        crate::model::anthropic::claude_haiku_3,
    )
    .with_aliases(&["claude-3-haiku"]),
    // =========================================================================
    // OpenAI Models - GPT-5.x (Latest)
    // =========================================================================
    ModelDefinition::new(
        "gpt-5.2",
        "openai",
        "GPT-5.2",
        crate::model::openai::gpt_5_2,
    ),
    ModelDefinition::new(
        "gpt-5.1",
        "openai",
        "GPT-5.1",
        crate::model::openai::gpt_5_1,
    ),
    ModelDefinition::new("gpt-5", "openai", "GPT-5", crate::model::openai::gpt_5),
    ModelDefinition::new(
        "gpt-5-mini",
        "openai",
        "GPT-5 mini",
        crate::model::openai::gpt_5_mini,
    ),
    ModelDefinition::new(
        "gpt-5-nano",
        "openai",
        "GPT-5 nano",
        crate::model::openai::gpt_5_nano,
    ),
    // =========================================================================
    // OpenAI Models - GPT-4.1
    // =========================================================================
    ModelDefinition::new(
        "gpt-4.1",
        "openai",
        "GPT-4.1",
        crate::model::openai::gpt_4_1,
    ),
    ModelDefinition::new(
        "gpt-4.1-mini",
        "openai",
        "GPT-4.1 mini",
        crate::model::openai::gpt_4_1_mini,
    ),
    ModelDefinition::new(
        "gpt-4.1-nano",
        "openai",
        "GPT-4.1 nano",
        crate::model::openai::gpt_4_1_nano,
    ),
    // =========================================================================
    // OpenAI Models - GPT-4o Series
    // =========================================================================
    ModelDefinition::new("gpt-4o", "openai", "GPT-4o", crate::model::openai::gpt_4o),
    ModelDefinition::new(
        "gpt-4o-mini",
        "openai",
        "GPT-4o mini",
        crate::model::openai::gpt_4o_mini,
    ),
    // =========================================================================
    // OpenAI Models - o-series (reasoning)
    // =========================================================================
    ModelDefinition::new("o1", "openai", "o1", crate::model::openai::o1),
    ModelDefinition::new("o3", "openai", "o3", crate::model::openai::o3),
    ModelDefinition::new(
        "o3-mini",
        "openai",
        "o3-mini",
        crate::model::openai::o3_mini,
    ),
    ModelDefinition::new(
        "o4-mini",
        "openai",
        "o4-mini",
        crate::model::openai::o4_mini,
    ),
    // =========================================================================
    // OpenAI Codex Models - GPT-5.x Codex Family (March 2026)
    // =========================================================================
    // Default: GPT-5.3-Codex (most capable)
    ModelDefinition::new(
        "gpt-5.3-codex",
        "openai-codex",
        "GPT-5.3 Codex",
        crate::codex::models::gpt_5_3_codex,
    )
    .with_aliases(&["codex", "codex-latest"]),
    ModelDefinition::new(
        "gpt-5.2-codex",
        "openai-codex",
        "GPT-5.2 Codex",
        crate::codex::models::gpt_5_2_codex,
    ),
    ModelDefinition::new(
        "gpt-5.1-codex",
        "openai-codex",
        "GPT-5.1 Codex",
        crate::codex::models::gpt_5_1_codex,
    ),
    ModelDefinition::new(
        "gpt-5.1-codex-max",
        "openai-codex",
        "GPT-5.1 Codex Max",
        crate::codex::models::gpt_5_1_codex_max,
    ),
    ModelDefinition::new(
        "gpt-5-codex",
        "openai-codex",
        "GPT-5 Codex",
        crate::codex::models::gpt_5_codex,
    ),
    ModelDefinition::new(
        "gpt-5.1-codex-mini",
        "openai-codex",
        "GPT-5.1 Codex Mini",
        crate::codex::models::gpt_5_1_codex_mini,
    ),
    // Legacy model (deprecated but still available)
    ModelDefinition::new(
        "codex-mini-latest",
        "openai-codex",
        "Codex Mini (Legacy)",
        crate::codex::models::codex_mini_latest,
    ),
    // Reasoning models via Codex API
    ModelDefinition::new(
        "codex-o3",
        "openai-codex",
        "OpenAI o3 (via Codex)",
        crate::codex::models::o3,
    )
    .with_aliases(&["codex/o3"]),
    ModelDefinition::new(
        "codex-o4-mini",
        "openai-codex",
        "OpenAI o4-mini (via Codex)",
        crate::codex::models::o4_mini,
    )
    .with_aliases(&["codex/o4-mini"]),
    // =========================================================================
    // Compound Coders Models
    // =========================================================================
    ModelDefinition::new(
        "wonop/gpt",
        "compoundcoders",
        "Wonop GPT",
        crate::compoundcoders::models::wonop_gpt,
    ),
    ModelDefinition::new(
        "wonop/qwen",
        "compoundcoders",
        "Wonop QWEN",
        crate::compoundcoders::models::wonop_qwen,
    ),
];

// =============================================================================
// Provider Lookup Functions
// =============================================================================

/// Get a provider by ID.
pub fn get_provider(id: &str) -> Option<&'static ProviderDefinition> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Get all providers.
pub fn all_providers() -> &'static [ProviderDefinition] {
    PROVIDERS
}

/// Get the environment variable name for a provider's API key.
pub fn get_api_key_env_var(provider_id: &str) -> Option<&'static str> {
    get_provider(provider_id)
        .and_then(|p| p.api_key.map(|k| k.env_var).or_else(|| p.env_vars.first().copied()))
}

// =============================================================================
// Settings Helper Functions
// =============================================================================

/// Get all providers that support a given auth method.
pub fn providers_with_auth(method: AuthMethodType) -> Vec<&'static ProviderDefinition> {
    PROVIDERS
        .iter()
        .filter(|p| p.auth_methods.contains(&method))
        .collect()
}

/// Get all providers that need API key settings in the UI.
pub fn providers_needing_api_key() -> Vec<&'static ProviderDefinition> {
    PROVIDERS
        .iter()
        .filter(|p| p.api_key.is_some())
        .collect()
}

/// API key setting for the UI (deduped by creds_key).
#[derive(Debug, Clone)]
pub struct ApiKeySetting {
    /// The credentials key (e.g., "anthropic", "openai")
    pub creds_key: &'static str,
    /// Human-readable label for the setting
    pub label: String,
    /// Environment variable name
    pub env_var: &'static str,
    /// Hint text for the UI
    pub hint: &'static str,
    /// Provider IDs that use this key
    pub providers: Vec<&'static str>,
}

/// Get unique API key settings (deduped by creds_key).
/// This avoids showing duplicate settings for providers that share the same API key
/// (e.g., openai and openai-codex both use OPENAI_API_KEY).
pub fn unique_api_key_settings() -> Vec<ApiKeySetting> {
    use std::collections::HashMap;
    
    let mut by_creds_key: HashMap<&'static str, ApiKeySetting> = HashMap::new();
    
    for provider in PROVIDERS.iter() {
        if let Some(api_key) = provider.api_key {
            let entry = by_creds_key.entry(api_key.creds_key).or_insert_with(|| {
                ApiKeySetting {
                    creds_key: api_key.creds_key,
                    label: format!("{} API Key", provider.name.replace(" API", "")),
                    env_var: api_key.env_var,
                    hint: api_key.hint,
                    providers: vec![],
                }
            });
            entry.providers.push(provider.id);
        }
    }
    
    // Sort by creds_key for consistent ordering
    let mut settings: Vec<_> = by_creds_key.into_values().collect();
    settings.sort_by(|a, b| a.creds_key.cmp(b.creds_key));
    settings
}

/// Get CLI providers with their binary info.
/// Returns tuples of (provider_id, binary_name, display_name).
pub fn cli_providers_with_paths() -> Vec<(&'static str, &'static str, &'static str)> {
    PROVIDERS
        .iter()
        .filter_map(|p| {
            p.cli.map(|cli| (p.id, cli.binary_name, p.name))
        })
        .collect()
}

/// Get all providers that support CLI authentication.
pub fn providers_with_cli() -> Vec<&'static ProviderDefinition> {
    PROVIDERS
        .iter()
        .filter(|p| p.cli.is_some())
        .collect()
}

/// Get all providers that have test configuration.
pub fn providers_with_test() -> Vec<&'static ProviderDefinition> {
    PROVIDERS
        .iter()
        .filter(|p| p.test.is_some())
        .collect()
}

/// Get API key config for a provider.
pub fn get_api_key_config(provider_id: &str) -> Option<ApiKeyConfig> {
    get_provider(provider_id).and_then(|p| p.api_key)
}

/// Get CLI config for a provider.
pub fn get_cli_config(provider_id: &str) -> Option<CliConfig> {
    get_provider(provider_id).and_then(|p| p.cli)
}

/// Get test config for a provider.
pub fn get_test_config(provider_id: &str) -> Option<TestConfig> {
    get_provider(provider_id).and_then(|p| p.test)
}

// =============================================================================
// Model Lookup Functions
// =============================================================================

/// Find a model definition by ID (checks primary ID and aliases).
pub fn find_model(model_id: &str) -> Option<&'static ModelDefinition> {
    MODELS.iter().find(|m| {
        m.id == model_id || m.aliases.contains(&model_id)
    })
}

/// Get model info by ID, falling back to creating a generic ModelInfo if not found.
pub fn get_model_info(model_id: &str, provider_id: &str) -> ModelInfo {
    // First try to find in the registry
    if let Some(model_def) = find_model(model_id) {
        let mut info = (model_def.info_fn)();
        // Override provider_id if caller specifies a different one (e.g., anthropic-cli)
        if provider_id != model_def.provider_id && !provider_id.is_empty() {
            info.provider_id = provider_id.to_string();
        }
        return info;
    }

    // Fall back to creating a generic ModelInfo
    create_fallback_model_info(model_id, provider_id)
}

/// Create a fallback ModelInfo for unknown models.
fn create_fallback_model_info(model_id: &str, provider_id: &str) -> ModelInfo {
    use crate::model::{ModelCapabilities, ModelCost, ModelLimit, ModelStatus, ModalitySupport};

    // Provider-specific defaults
    let (context, output) = match provider_id {
        "anthropic" | "anthropic-cli" => (200_000, 8_192),
        "openai" => (128_000, 16_384),
        "openai-codex" => (256_000, 128_000), // GPT-5.x Codex models
        _ => (32_000, 4_096),
    };

    ModelInfo {
        id: model_id.to_string(),
        provider_id: provider_id.to_string(),
        name: model_id.to_string(),
        family: None,
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
        cost: ModelCost::default(),
        limit: ModelLimit {
            context,
            output,
        },
        status: ModelStatus::Active,
    }
}

/// Get all models for a specific provider.
pub fn get_models_for_provider(provider_id: &str) -> Vec<&'static ModelDefinition> {
    // For CLI variants (like anthropic-cli), also include parent provider's models
    let effective_provider = get_provider(provider_id)
        .and_then(|p| p.parent)
        .unwrap_or(provider_id);

    MODELS
        .iter()
        .filter(|m| m.provider_id == effective_provider)
        .collect()
}

/// Check if a model is compatible with a provider.
pub fn is_model_compatible(provider_id: &str, model_id: &str) -> bool {
    // For CLI variants, delegate to parent
    let effective_provider = get_provider(provider_id)
        .and_then(|p| p.parent)
        .unwrap_or(provider_id);

    // Check if model exists in registry for this provider
    if let Some(model_def) = find_model(model_id) {
        return model_def.provider_id == effective_provider;
    }

    // Fall back to prefix matching for unknown models
    let model_lower = model_id.to_lowercase();
    match effective_provider {
        "anthropic" => model_lower.starts_with("claude"),
        "openai" => {
            model_lower.starts_with("gpt-")
                || model_lower.starts_with("o1")
                || model_lower.starts_with("o3")
                || model_lower.starts_with("o4")
        }
        "openai-codex" => {
            model_lower == "codex"
                || model_lower.starts_with("codex-")
                || model_lower.starts_with("gpt-5.3-codex")
                || model_lower.starts_with("gpt-5.2-codex")
                || model_lower.starts_with("gpt-5.1-codex")
                || model_lower.starts_with("gpt-5-codex")
        }
        "compoundcoders" => {
            model_lower.starts_with("wonop/")
        }
        _ => false,
    }
}

/// Infer provider from model ID.
pub fn infer_provider(model_id: &str) -> &'static str {
    // First check the registry
    if let Some(model_def) = find_model(model_id) {
        return model_def.provider_id;
    }

    // Fall back to prefix matching
    let model_lower = model_id.to_lowercase();

    // Check for Codex models first (before generic gpt- check)
    if model_lower == "codex"
        || model_lower.starts_with("codex-")
        || model_lower.starts_with("gpt-5.3-codex")
        || model_lower.starts_with("gpt-5.2-codex")
        || model_lower.starts_with("gpt-5.1-codex")
        || model_lower.starts_with("gpt-5-codex")
    {
        return "openai-codex";
    }
    // Generic OpenAI models
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
    {
        return "openai";
    }
    if model_lower.starts_with("claude") {
        return "anthropic";
    }
    if model_lower.starts_with("wonop/") {
        return "compoundcoders";
    }

    // Default
    "anthropic"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_provider() {
        let anthropic = get_provider("anthropic").expect("anthropic should exist");
        assert_eq!(anthropic.id, "anthropic");
        assert_eq!(anthropic.env_vars, &["ANTHROPIC_API_KEY"]);
        assert!(anthropic.parent.is_none());

        let cli = get_provider("anthropic-cli").expect("anthropic-cli should exist");
        assert_eq!(cli.parent, Some("anthropic"));
    }

    #[test]
    fn test_find_model() {
        // Direct lookup
        let sonnet = find_model("claude-sonnet-4-5-20250929").expect("model should exist");
        assert_eq!(sonnet.provider_id, "anthropic");

        // Alias lookup
        let sonnet_alias = find_model("claude-sonnet-4-5").expect("alias should work");
        assert_eq!(sonnet_alias.id, "claude-sonnet-4-5-20250929");
    }

    #[test]
    fn test_get_model_info() {
        let info = get_model_info("claude-sonnet-4-5-20250929", "anthropic");
        assert_eq!(info.id, "claude-sonnet-4-5-20250929");
        assert_eq!(info.provider_id, "anthropic");

        // Test provider override for CLI
        let cli_info = get_model_info("claude-sonnet-4-5-20250929", "anthropic-cli");
        assert_eq!(cli_info.provider_id, "anthropic-cli");
    }

    #[test]
    fn test_fallback_model_info() {
        let info = get_model_info("unknown-model", "anthropic");
        assert_eq!(info.id, "unknown-model");
        assert_eq!(info.provider_id, "anthropic");
    }

    #[test]
    fn test_infer_provider() {
        assert_eq!(infer_provider("claude-sonnet-4-5-20250929"), "anthropic");
        assert_eq!(infer_provider("gpt-4o"), "openai");
        assert_eq!(infer_provider("codex"), "openai-codex");
        assert_eq!(infer_provider("o3"), "openai");
        assert_eq!(infer_provider("wonop/gpt"), "compoundcoders");
        assert_eq!(infer_provider("wonop/qwen"), "compoundcoders");
    }

    #[test]
    fn test_compoundcoders_provider() {
        let provider = get_provider("compoundcoders").expect("compoundcoders should exist");
        assert_eq!(provider.id, "compoundcoders");
        assert_eq!(provider.env_vars, &["COMPOUNDCODERS_API_KEY"]);
        assert_eq!(provider.default_model, "wonop/gpt");

        let gpt = find_model("wonop/gpt").expect("wonop/gpt should exist");
        assert_eq!(gpt.provider_id, "compoundcoders");

        let qwen = find_model("wonop/qwen").expect("wonop/qwen should exist");
        assert_eq!(qwen.provider_id, "compoundcoders");

        assert!(is_model_compatible("compoundcoders", "wonop/gpt"));
        assert!(is_model_compatible("compoundcoders", "wonop/qwen"));
        assert!(!is_model_compatible("anthropic", "wonop/gpt"));
    }

    #[test]
    fn test_is_model_compatible() {
        assert!(is_model_compatible("anthropic", "claude-sonnet-4-5-20250929"));
        assert!(is_model_compatible("anthropic-cli", "claude-sonnet-4-5-20250929"));
        assert!(!is_model_compatible("anthropic", "gpt-4o"));
        assert!(is_model_compatible("openai", "gpt-4o"));
    }

    #[test]
    fn test_get_models_for_provider() {
        let anthropic_models = get_models_for_provider("anthropic");
        assert!(!anthropic_models.is_empty());
        assert!(anthropic_models.iter().any(|m| m.id == "claude-sonnet-4-5-20250929"));

        // CLI variant should get same models as parent
        let cli_models = get_models_for_provider("anthropic-cli");
        assert_eq!(anthropic_models.len(), cli_models.len());
    }

    #[test]
    fn test_codex_models() {
        // "codex" is now an alias for gpt-5.3-codex
        let codex = find_model("codex").expect("codex alias should exist");
        assert_eq!(codex.id, "gpt-5.3-codex");
        assert_eq!(codex.provider_id, "openai-codex");

        // Test GPT-5.x Codex models
        let gpt_5_3 = find_model("gpt-5.3-codex").expect("gpt-5.3-codex should exist");
        assert_eq!(gpt_5_3.provider_id, "openai-codex");

        let gpt_5_2 = find_model("gpt-5.2-codex").expect("gpt-5.2-codex should exist");
        assert_eq!(gpt_5_2.provider_id, "openai-codex");

        // Test legacy model
        let codex_mini = find_model("codex-mini-latest").expect("codex-mini-latest should exist");
        assert_eq!(codex_mini.provider_id, "openai-codex");

        // Test reasoning models via Codex
        let codex_o3 = find_model("codex-o3").expect("codex-o3 should exist");
        assert_eq!(codex_o3.provider_id, "openai-codex");

        // codex/o3 is an alias for codex-o3
        let codex_o3_alias = find_model("codex/o3");
        assert!(codex_o3_alias.is_some());
    }

    #[test]
    fn test_infer_provider_codex_models() {
        // GPT-5.x Codex models should infer to openai-codex
        assert_eq!(infer_provider("gpt-5.3-codex"), "openai-codex");
        assert_eq!(infer_provider("gpt-5.2-codex"), "openai-codex");
        assert_eq!(infer_provider("gpt-5.1-codex"), "openai-codex");
        assert_eq!(infer_provider("gpt-5-codex"), "openai-codex");
        assert_eq!(infer_provider("codex-mini-latest"), "openai-codex");
        
        // Generic GPT models should infer to openai
        assert_eq!(infer_provider("gpt-5.2"), "openai");
        assert_eq!(infer_provider("gpt-4o"), "openai");
    }

    // =========================================================================
    // Settings Tests
    // =========================================================================

    #[test]
    fn test_providers_needing_api_key() {
        let providers = providers_needing_api_key();
        
        // Should include anthropic, openai, openai-codex, compoundcoders
        assert!(providers.iter().any(|p| p.id == "anthropic"));
        assert!(providers.iter().any(|p| p.id == "openai"));
        assert!(providers.iter().any(|p| p.id == "openai-codex"));
        assert!(providers.iter().any(|p| p.id == "compoundcoders"));
        
        // Should NOT include anthropic-cli or test
        assert!(!providers.iter().any(|p| p.id == "anthropic-cli"));
        assert!(!providers.iter().any(|p| p.id == "test"));
    }

    #[test]
    fn test_providers_with_cli() {
        let providers = providers_with_cli();
        
        // Should include anthropic-cli and openai-codex
        assert!(providers.iter().any(|p| p.id == "anthropic-cli"));
        assert!(providers.iter().any(|p| p.id == "openai-codex"));
        
        // Should NOT include anthropic, openai, or test
        assert!(!providers.iter().any(|p| p.id == "anthropic"));
        assert!(!providers.iter().any(|p| p.id == "openai"));
    }

    #[test]
    fn test_providers_with_auth() {
        let api_key_providers = providers_with_auth(AuthMethodType::ApiKey);
        assert!(api_key_providers.iter().any(|p| p.id == "anthropic"));
        assert!(api_key_providers.iter().any(|p| p.id == "openai"));
        
        let cli_providers = providers_with_auth(AuthMethodType::Cli);
        assert!(cli_providers.iter().any(|p| p.id == "anthropic-cli"));
    }

    #[test]
    fn test_api_key_config() {
        let anthropic_config = get_api_key_config("anthropic").expect("should have config");
        assert_eq!(anthropic_config.env_var, "ANTHROPIC_API_KEY");
        assert_eq!(anthropic_config.creds_key, "anthropic");
        assert_eq!(anthropic_config.prefix, Some("sk-ant-"));
        assert_eq!(anthropic_config.header_name, "x-api-key");
        assert_eq!(anthropic_config.header_format, "{}");
        
        // Test header formatting
        assert_eq!(anthropic_config.format_header("my-key"), "my-key");
        
        let openai_config = get_api_key_config("openai").expect("should have config");
        assert_eq!(openai_config.header_name, "Authorization");
        assert_eq!(openai_config.header_format, "Bearer {}");
        assert_eq!(openai_config.format_header("my-key"), "Bearer my-key");
    }

    #[test]
    fn test_cli_config() {
        let cli_config = get_cli_config("anthropic-cli").expect("should have config");
        assert_eq!(cli_config.binary_name, "claude");
        assert_eq!(cli_config.auth_dir, ".claude");
        assert!(cli_config.login_command.contains("login"));
    }

    #[test]
    fn test_test_config() {
        let anthropic_test = get_test_config("anthropic").expect("should have config");
        assert_eq!(anthropic_test.method, "POST");
        assert!(anthropic_test.endpoint.contains("anthropic.com"));
        assert!(anthropic_test.body.is_some());
        
        let compoundcoders_test = get_test_config("compoundcoders").expect("should have config");
        assert_eq!(compoundcoders_test.method, "GET");
        assert!(compoundcoders_test.endpoint.contains("compoundcoders.com"));
        assert!(compoundcoders_test.body.is_none());
    }

    #[test]
    fn test_provider_supports_methods() {
        let anthropic = get_provider("anthropic").expect("should exist");
        assert!(anthropic.supports_api_key());
        assert!(!anthropic.supports_cli());
        
        let cli = get_provider("anthropic-cli").expect("should exist");
        assert!(!cli.supports_api_key());
        assert!(cli.supports_cli());
        
        let codex = get_provider("openai-codex").expect("should exist");
        assert!(codex.supports_api_key());
        assert!(codex.supports_cli()); // Codex has both
    }

    #[test]
    fn test_provider_creds_key() {
        let anthropic = get_provider("anthropic").expect("should exist");
        assert_eq!(anthropic.creds_key(), Some("anthropic"));
        
        let cli = get_provider("anthropic-cli").expect("should exist");
        assert_eq!(cli.creds_key(), None); // CLI doesn't use API key
        
        let compoundcoders = get_provider("compoundcoders").expect("should exist");
        assert_eq!(compoundcoders.creds_key(), Some("compoundcoders"));
    }

    #[test]
    fn test_unique_api_key_settings() {
        let settings = unique_api_key_settings();
        
        // Should have 3 unique keys: anthropic, compoundcoders, openai
        // (openai and openai-codex share the same key)
        assert_eq!(settings.len(), 3);
        
        // Check anthropic
        let anthropic = settings.iter().find(|s| s.creds_key == "anthropic").unwrap();
        assert!(anthropic.providers.contains(&"anthropic"));
        
        // Check openai - should include both openai and openai-codex
        let openai = settings.iter().find(|s| s.creds_key == "openai").unwrap();
        assert!(openai.providers.contains(&"openai"));
        assert!(openai.providers.contains(&"openai-codex"));
        
        // Check compoundcoders
        let cc = settings.iter().find(|s| s.creds_key == "compoundcoders").unwrap();
        assert!(cc.providers.contains(&"compoundcoders"));
    }

    #[test]
    fn test_cli_providers_with_paths() {
        let cli_providers = cli_providers_with_paths();
        
        // Should have anthropic-cli and openai-codex
        assert!(cli_providers.iter().any(|(id, _, _)| *id == "anthropic-cli"));
        assert!(cli_providers.iter().any(|(id, _, _)| *id == "openai-codex"));
        
        // Check binary names
        let claude = cli_providers.iter().find(|(id, _, _)| *id == "anthropic-cli").unwrap();
        assert_eq!(claude.1, "claude");
        
        let codex = cli_providers.iter().find(|(id, _, _)| *id == "openai-codex").unwrap();
        assert_eq!(codex.1, "codex");
    }
}
