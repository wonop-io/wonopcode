//! Model-related utilities and commands.
//!
//! This module provides utilities for working with AI models, including:
//! - Parsing model specifications in provider/model format
//! - Inferring providers from well-known model names
//! - Managing default models per provider
//! - Listing available models
//! - Parsing release channels

/// Parse a release channel string into a ReleaseChannel enum.
///
/// Accepts "stable", "beta", or "nightly" (case-insensitive).
/// Returns None for unknown channels and prints a warning.
pub fn parse_release_channel(s: &str) -> Option<wonopcode_core::version::ReleaseChannel> {
    match s.to_lowercase().as_str() {
        "stable" => Some(wonopcode_core::version::ReleaseChannel::Stable),
        "beta" => Some(wonopcode_core::version::ReleaseChannel::Beta),
        "nightly" => Some(wonopcode_core::version::ReleaseChannel::Nightly),
        _ => {
            eprintln!("Unknown release channel: {s}. Using 'stable'.");
            None
        }
    }
}

/// Parse model specification in provider/model format.
///
/// If the spec contains a '/', it's treated as "provider/model".
/// Otherwise, tries to infer the provider from well-known model names,
/// falling back to the provided default_provider.
///
/// # Arguments
/// * `spec` - Model specification (e.g., "openai/gpt-4o" or just "claude-sonnet-4-5-20250929")
/// * `default_provider` - Provider to use if inference fails
///
/// # Returns
/// A tuple of (provider, model)
pub fn parse_model_spec(spec: &str, default_provider: &str) -> (String, String) {
    if let Some((provider, model)) = spec.split_once('/') {
        (provider.to_string(), model.to_string())
    } else {
        // Try to infer provider from model name
        let provider = infer_provider_from_model(spec).unwrap_or(default_provider);
        (provider.to_string(), spec.to_string())
    }
}

/// Infer the provider from a model name.
///
/// Recognizes common model naming patterns:
/// - OpenAI: gpt-, o1, o3, chatgpt
/// - Anthropic: claude
/// - Google: gemini
///
/// Returns None if the provider cannot be inferred.
pub fn infer_provider_from_model(model: &str) -> Option<&'static str> {
    let model_lower = model.to_lowercase();

    // OpenAI models
    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("chatgpt")
    {
        return Some("openai");
    }

    // Anthropic models
    if model_lower.starts_with("claude") {
        return Some("anthropic");
    }

    // Google models
    if model_lower.starts_with("gemini") {
        return Some("google");
    }

    None
}

/// Get default model for a provider.
///
/// Returns the recommended default model for each provider:
/// - anthropic: claude-sonnet-4-5-20250929
/// - openai: gpt-4o
/// - openrouter: anthropic/claude-sonnet-4-5
/// - others: claude-sonnet-4-5-20250929
pub fn get_default_model(provider: &str) -> String {
    match provider {
        "anthropic" => "claude-sonnet-4-5-20250929".to_string(),
        "openai" => "gpt-4o".to_string(),
        "openrouter" => "anthropic/claude-sonnet-4-5".to_string(),
        _ => "claude-sonnet-4-5-20250929".to_string(),
    }
}

/// List available models.
///
/// Prints a formatted list of all supported models organized by provider
/// and generation, including descriptions and recommendations.
pub fn list_models() {
    println!("Available models:");
    println!();
    println!("Anthropic (Latest - Claude 4.5):");
    println!("  claude-sonnet-4-5-20250929  Claude Sonnet 4.5 (recommended)");
    println!("  claude-haiku-4-5-20251001   Claude Haiku 4.5 (fastest)");
    println!("  claude-opus-4-5-20251101    Claude Opus 4.5 (most intelligent)");
    println!();
    println!("Anthropic (Legacy - Claude 4.x):");
    println!("  claude-sonnet-4-20250514    Claude Sonnet 4");
    println!("  claude-opus-4-1-20250805    Claude Opus 4.1");
    println!("  claude-opus-4-20250514      Claude Opus 4");
    println!();
    println!("Anthropic (Legacy - Claude 3.x):");
    println!("  claude-3-7-sonnet-20250219  Claude 3.7 Sonnet (extended thinking)");
    println!("  claude-3-haiku-20240307     Claude 3 Haiku (economical)");
    println!();
    println!("OpenAI (GPT-5):");
    println!("  gpt-5.2                     GPT-5.2 (best for coding & agents)");
    println!("  gpt-5.1                     GPT-5.1 (configurable reasoning)");
    println!("  gpt-5                       GPT-5 (intelligent reasoning)");
    println!("  gpt-5-mini                  GPT-5 mini (fast, cost-efficient)");
    println!("  gpt-5-nano                  GPT-5 nano (fastest, cheapest)");
    println!();
    println!("OpenAI (GPT-4.1):");
    println!("  gpt-4.1                     GPT-4.1 (smartest non-reasoning)");
    println!("  gpt-4.1-mini                GPT-4.1 mini (fast, 1M context)");
    println!("  gpt-4.1-nano                GPT-4.1 nano (cheapest, 1M context)");
    println!();
    println!("OpenAI (O-Series):");
    println!("  o3                          o3 (reasoning model)");
    println!("  o3-mini                     o3-mini (fast reasoning)");
    println!("  o4-mini                     o4-mini (cost-efficient reasoning)");
    println!();
    println!("OpenAI (Legacy):");
    println!("  gpt-4o                      GPT-4o (previous flagship)");
    println!("  gpt-4o-mini                 GPT-4o mini (fast, affordable)");
    println!("  o1                          o1 (legacy reasoning)");
    println!();
    println!("OpenRouter:");
    println!("  Use any model ID from https://openrouter.ai/models");
}
