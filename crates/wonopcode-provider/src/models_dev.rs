//! models.dev integration for dynamic model fetching.
//!
//! This module fetches model information from <https://models.dev/api.json>
//! and provides a unified view of all available models across providers.

use crate::model::{
    ModalitySupport, ModelCapabilities, ModelCost, ModelInfo, ModelLimit, ModelStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// URL for models.dev API
const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// Cache duration in seconds (1 hour)
const CACHE_DURATION_SECS: u64 = 3600;

/// User agent for requests
const USER_AGENT: &str = concat!("wonopcode/", env!("CARGO_PKG_VERSION"));

/// models.dev provider structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevProvider {
    /// Provider API base URL
    #[serde(default)]
    pub api: Option<String>,

    /// Display name
    pub name: String,

    /// Environment variable names for API key
    #[serde(default)]
    pub env: Vec<String>,

    /// Provider ID
    pub id: String,

    /// NPM package name
    #[serde(default)]
    pub npm: Option<String>,

    /// Models offered by this provider
    #[serde(default)]
    pub models: HashMap<String, ModelsDevModel>,
}

/// models.dev model structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevModel {
    /// Model ID
    pub id: String,

    /// Display name
    pub name: String,

    /// Model family
    #[serde(default)]
    pub family: Option<String>,

    /// Release date
    #[serde(default)]
    pub release_date: Option<String>,

    /// Supports file attachments
    #[serde(default)]
    pub attachment: bool,

    /// Supports reasoning/thinking
    #[serde(default)]
    pub reasoning: bool,

    /// Supports temperature parameter
    #[serde(default)]
    pub temperature: bool,

    /// Supports tool/function calling
    #[serde(default)]
    pub tool_call: bool,

    /// Supports interleaved thinking
    #[serde(default)]
    pub interleaved: Option<serde_json::Value>,

    /// Cost information
    #[serde(default)]
    pub cost: Option<ModelsDevCost>,

    /// Token limits
    pub limit: ModelsDevLimit,

    /// Input/output modalities
    #[serde(default)]
    pub modalities: Option<ModelsDevModalities>,

    /// Experimental model
    #[serde(default)]
    pub experimental: Option<bool>,

    /// Model status
    #[serde(default)]
    pub status: Option<String>,

    /// Additional options
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,

    /// Custom headers
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,

    /// Provider info override
    #[serde(default)]
    pub provider: Option<ModelsDevProviderRef>,
}

/// Cost information from models.dev
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevCost {
    /// Input cost per million tokens
    #[serde(default)]
    pub input: f64,

    /// Output cost per million tokens
    #[serde(default)]
    pub output: f64,

    /// Cache read cost per million tokens
    #[serde(default)]
    pub cache_read: Option<f64>,

    /// Cache write cost per million tokens
    #[serde(default)]
    pub cache_write: Option<f64>,

    /// Cost for context over 200k tokens
    #[serde(default)]
    pub context_over_200k: Option<ModelsDevCostOverride>,
}

/// Cost override for large contexts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevCostOverride {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// Token limits from models.dev
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevLimit {
    /// Maximum context window
    pub context: u32,

    /// Maximum output tokens
    pub output: u32,
}

/// Modalities from models.dev
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevModalities {
    /// Input modalities
    #[serde(default)]
    pub input: Vec<String>,

    /// Output modalities
    #[serde(default)]
    pub output: Vec<String>,
}

/// Provider reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDevProviderRef {
    pub npm: String,
}

/// Cache entry
#[derive(Debug)]
struct CacheEntry {
    data: HashMap<String, ModelsDevProvider>,
    fetched_at: std::time::Instant,
}

/// Models.dev client for fetching model information
pub struct ModelsDevClient {
    client: reqwest::Client,
    cache: Arc<RwLock<Option<CacheEntry>>>,
    cache_path: PathBuf,
}

impl ModelsDevClient {
    /// Create a new models.dev client
    pub fn new() -> Self {
        let cache_path = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("wonopcode")
            .join("models.json");

        Self {
            client: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            cache: Arc::new(RwLock::new(None)),
            cache_path,
        }
    }

    /// Get all providers and their models
    pub async fn get_providers(
        &self,
    ) -> Result<HashMap<String, ModelsDevProvider>, ModelsDevError> {
        // Check memory cache
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.as_ref() {
                if entry.fetched_at.elapsed().as_secs() < CACHE_DURATION_SECS {
                    debug!("Using memory-cached models.dev data");
                    return Ok(entry.data.clone());
                }
            }
        }

        // Try to load from disk cache
        if let Some(data) = self.load_from_disk().await {
            debug!("Loaded models.dev data from disk cache");
            let mut cache = self.cache.write().await;
            *cache = Some(CacheEntry {
                data: data.clone(),
                fetched_at: std::time::Instant::now(),
            });

            // Trigger background refresh
            let client = self.clone();
            tokio::spawn(async move {
                let _ = client.refresh().await;
            });

            return Ok(data);
        }

        // Fetch from API
        self.fetch_and_cache().await
    }

    /// Refresh the cache from the API
    pub async fn refresh(&self) -> Result<(), ModelsDevError> {
        info!("Refreshing models.dev data");
        let _ = self.fetch_and_cache().await?;
        Ok(())
    }

    /// Fetch from API and update cache
    async fn fetch_and_cache(&self) -> Result<HashMap<String, ModelsDevProvider>, ModelsDevError> {
        let response = self
            .client
            .get(MODELS_DEV_URL)
            .send()
            .await
            .map_err(|e| ModelsDevError::Fetch(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(ModelsDevError::Fetch(format!(
                "HTTP {status}: {}",
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        let text = response
            .text()
            .await
            .map_err(|e| ModelsDevError::Fetch(e.to_string()))?;

        let data: HashMap<String, ModelsDevProvider> =
            serde_json::from_str(&text).map_err(|e| ModelsDevError::Parse(e.to_string()))?;

        // Update memory cache
        {
            let mut cache = self.cache.write().await;
            *cache = Some(CacheEntry {
                data: data.clone(),
                fetched_at: std::time::Instant::now(),
            });
        }

        // Save to disk cache
        if let Err(e) = self.save_to_disk(&text).await {
            warn!("Failed to save models.dev cache: {e}");
        }

        info!(
            "Successfully fetched {} providers from models.dev",
            data.len()
        );
        Ok(data)
    }

    /// Load from disk cache
    async fn load_from_disk(&self) -> Option<HashMap<String, ModelsDevProvider>> {
        match tokio::fs::read_to_string(&self.cache_path).await {
            Ok(content) => serde_json::from_str(&content).ok(),
            Err(_) => None,
        }
    }

    /// Save to disk cache
    async fn save_to_disk(&self, content: &str) -> Result<(), std::io::Error> {
        if let Some(parent) = self.cache_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&self.cache_path, content).await
    }

    /// Get all models as ModelInfo
    pub async fn get_all_models(&self) -> Result<Vec<ModelInfo>, ModelsDevError> {
        let providers = self.get_providers().await?;
        let mut models = Vec::new();

        for (provider_id, provider) in providers {
            for (_model_key, model) in provider.models {
                models.push(model_to_info(&provider_id, &model));
            }
        }

        Ok(models)
    }

    /// Get models for a specific provider
    pub async fn get_provider_models(
        &self,
        provider_id: &str,
    ) -> Result<Vec<ModelInfo>, ModelsDevError> {
        let providers = self.get_providers().await?;

        if let Some(provider) = providers.get(provider_id) {
            Ok(provider
                .models
                .values()
                .map(|m| model_to_info(provider_id, m))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }
}

impl Clone for ModelsDevClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            cache: self.cache.clone(),
            cache_path: self.cache_path.clone(),
        }
    }
}

impl Default for ModelsDevClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert models.dev model to our ModelInfo
fn model_to_info(provider_id: &str, model: &ModelsDevModel) -> ModelInfo {
    let status = match model.status.as_deref() {
        Some("alpha") => ModelStatus::Alpha,
        Some("beta") => ModelStatus::Beta,
        Some("deprecated") => ModelStatus::Deprecated,
        _ => ModelStatus::Active,
    };

    let modalities = model.modalities.as_ref();

    let input_modality = ModalitySupport {
        text: modalities
            .map(|m| m.input.contains(&"text".to_string()))
            .unwrap_or(true),
        image: modalities
            .map(|m| m.input.contains(&"image".to_string()))
            .unwrap_or(false),
        audio: modalities
            .map(|m| m.input.contains(&"audio".to_string()))
            .unwrap_or(false),
        video: modalities
            .map(|m| m.input.contains(&"video".to_string()))
            .unwrap_or(false),
        pdf: modalities
            .map(|m| m.input.contains(&"pdf".to_string()))
            .unwrap_or(false),
    };

    let output_modality = ModalitySupport {
        text: modalities
            .map(|m| m.output.contains(&"text".to_string()))
            .unwrap_or(true),
        image: modalities
            .map(|m| m.output.contains(&"image".to_string()))
            .unwrap_or(false),
        audio: modalities
            .map(|m| m.output.contains(&"audio".to_string()))
            .unwrap_or(false),
        video: modalities
            .map(|m| m.output.contains(&"video".to_string()))
            .unwrap_or(false),
        pdf: false,
    };

    let interleaved = model
        .interleaved
        .as_ref()
        .map(|v| v.as_bool().unwrap_or(false) || v.is_object())
        .unwrap_or(false);

    ModelInfo {
        id: model.id.clone(),
        provider_id: provider_id.to_string(),
        name: model.name.clone(),
        family: model.family.clone(),
        capabilities: ModelCapabilities {
            temperature: model.temperature,
            reasoning: model.reasoning,
            attachment: model.attachment,
            tool_call: model.tool_call,
            input: input_modality,
            output: output_modality,
            interleaved,
        },
        cost: model
            .cost
            .as_ref()
            .map(|c| ModelCost {
                input: c.input,
                output: c.output,
                cache_read: c.cache_read.unwrap_or(0.0),
                cache_write: c.cache_write.unwrap_or(0.0),
            })
            .unwrap_or_default(),
        limit: ModelLimit {
            context: model.limit.context,
            output: model.limit.output,
        },
        status,
    }
}

/// Error type for models.dev operations
#[derive(Debug, thiserror::Error)]
pub enum ModelsDevError {
    #[error("Failed to fetch models: {0}")]
    Fetch(String),

    #[error("Failed to parse models: {0}")]
    Parse(String),
}

/// Global singleton for models.dev client
static MODELS_DEV_CLIENT: std::sync::OnceLock<ModelsDevClient> = std::sync::OnceLock::new();

/// Get the global models.dev client
pub fn client() -> &'static ModelsDevClient {
    MODELS_DEV_CLIENT.get_or_init(ModelsDevClient::new)
}

/// Get all models from models.dev
pub async fn get_all_models() -> Result<Vec<ModelInfo>, ModelsDevError> {
    client().get_all_models().await
}

/// Get models for a specific provider
pub async fn get_provider_models(provider_id: &str) -> Result<Vec<ModelInfo>, ModelsDevError> {
    client().get_provider_models(provider_id).await
}

/// Refresh the models.dev cache
pub async fn refresh() -> Result<(), ModelsDevError> {
    client().refresh().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse_models_dev() {
        // Test parsing a sample response
        let sample = r#"{
            "anthropic": {
                "api": "https://api.anthropic.com",
                "name": "Anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "id": "anthropic",
                "npm": "@ai-sdk/anthropic",
                "models": {
                    "claude-sonnet-4-20250514": {
                        "id": "claude-sonnet-4-20250514",
                        "name": "Claude Sonnet 4",
                        "family": "claude-4",
                        "release_date": "2025-05-14",
                        "attachment": true,
                        "reasoning": true,
                        "temperature": true,
                        "tool_call": true,
                        "interleaved": true,
                        "cost": {
                            "input": 3,
                            "output": 15,
                            "cache_read": 0.3,
                            "cache_write": 3.75
                        },
                        "limit": {
                            "context": 200000,
                            "output": 64000
                        },
                        "modalities": {
                            "input": ["text", "image", "pdf"],
                            "output": ["text"]
                        },
                        "options": {}
                    }
                }
            }
        }"#;

        let providers: HashMap<String, ModelsDevProvider> = serde_json::from_str(sample).unwrap();
        assert!(providers.contains_key("anthropic"));

        let anthropic = &providers["anthropic"];
        assert_eq!(anthropic.name, "Anthropic");
        assert!(anthropic.models.contains_key("claude-sonnet-4-20250514"));

        let model = &anthropic.models["claude-sonnet-4-20250514"];
        assert_eq!(model.name, "Claude Sonnet 4");
        assert!(model.reasoning);
        assert!(model.tool_call);
    }
}
