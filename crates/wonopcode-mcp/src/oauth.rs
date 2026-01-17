//! OAuth support for MCP remote servers.
//!
//! Implements OAuth 2.0 with PKCE for authenticating with remote MCP servers.

use crate::error::{McpError, McpResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// OAuth callback port.
pub const OAUTH_CALLBACK_PORT: u16 = 19876;

/// OAuth callback path.
pub const OAUTH_CALLBACK_PATH: &str = "/mcp/oauth/callback";

/// OAuth configuration.
#[derive(Debug, Clone, Default)]
pub struct OAuthConfig {
    /// Pre-registered client ID (optional).
    pub client_id: Option<String>,
    /// Pre-registered client secret (optional).
    pub client_secret: Option<String>,
    /// Requested scopes.
    pub scope: Option<String>,
}

/// OAuth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Client information (from dynamic registration or config).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id_issued_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<u64>,
}

/// OAuth state stored for an MCP server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OAuthState {
    /// Server URL this auth is for.
    pub server_url: Option<String>,
    /// Dynamic client registration info.
    pub client_info: Option<ClientInfo>,
    /// OAuth tokens.
    pub tokens: Option<StoredTokens>,
    /// PKCE code verifier (temporary, during auth flow).
    pub code_verifier: Option<String>,
    /// OAuth state parameter (temporary, during auth flow).
    pub oauth_state: Option<String>,
}

/// Tokens stored with expiration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp when token expires.
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}

/// OAuth provider for MCP authentication.
pub struct OAuthProvider {
    /// MCP server name.
    mcp_name: String,
    /// Server URL.
    server_url: String,
    /// OAuth configuration.
    config: OAuthConfig,
    /// Stored state.
    state: Arc<RwLock<OAuthState>>,
}

impl OAuthProvider {
    /// Create a new OAuth provider.
    pub fn new(mcp_name: String, server_url: String, config: OAuthConfig) -> Self {
        Self {
            mcp_name,
            server_url,
            config,
            state: Arc::new(RwLock::new(OAuthState::default())),
        }
    }

    /// Get the redirect URL.
    pub fn redirect_url(&self) -> String {
        format!("http://127.0.0.1:{OAUTH_CALLBACK_PORT}{OAUTH_CALLBACK_PATH}")
    }

    /// Get client metadata for dynamic registration.
    pub fn client_metadata(&self) -> HashMap<String, serde_json::Value> {
        let mut metadata = HashMap::new();
        metadata.insert(
            "redirect_uris".to_string(),
            serde_json::json!([self.redirect_url()]),
        );
        metadata.insert("client_name".to_string(), serde_json::json!("Wonopcode"));
        metadata.insert(
            "client_uri".to_string(),
            serde_json::json!("https://wonopcode.dev"),
        );
        metadata.insert(
            "grant_types".to_string(),
            serde_json::json!(["authorization_code", "refresh_token"]),
        );
        metadata.insert("response_types".to_string(), serde_json::json!(["code"]));

        let auth_method = if self.config.client_secret.is_some() {
            "client_secret_post"
        } else {
            "none"
        };
        metadata.insert(
            "token_endpoint_auth_method".to_string(),
            serde_json::json!(auth_method),
        );

        metadata
    }

    /// Get client information (from config or stored).
    pub async fn client_info(&self) -> Option<ClientInfo> {
        // Check config first
        if let Some(ref client_id) = self.config.client_id {
            return Some(ClientInfo {
                client_id: client_id.clone(),
                client_secret: self.config.client_secret.clone(),
                client_id_issued_at: None,
                client_secret_expires_at: None,
            });
        }

        // Check stored client info
        let state = self.state.read().await;

        // Validate URL matches
        if state.server_url.as_ref() != Some(&self.server_url) {
            return None;
        }

        if let Some(ref info) = state.client_info {
            // Check if client secret has expired
            if let Some(expires_at) = info.client_secret_expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if expires_at < now {
                    info!(mcp = %self.mcp_name, "Client secret expired");
                    return None;
                }
            }
            return Some(info.clone());
        }

        None
    }

    /// Save client information from dynamic registration.
    pub async fn save_client_info(&self, info: ClientInfo) {
        let mut state = self.state.write().await;
        state.server_url = Some(self.server_url.clone());
        state.client_info = Some(info);
        info!(mcp = %self.mcp_name, "Saved dynamically registered client");
    }

    /// Get stored tokens.
    pub async fn tokens(&self) -> Option<OAuthTokens> {
        let state = self.state.read().await;

        // Validate URL matches
        if state.server_url.as_ref() != Some(&self.server_url) {
            return None;
        }

        state.tokens.as_ref().map(|t| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let expires_in = t.expires_at.map(|exp| exp.saturating_sub(now));

            OAuthTokens {
                access_token: t.access_token.clone(),
                token_type: "Bearer".to_string(),
                refresh_token: t.refresh_token.clone(),
                expires_in,
                scope: t.scope.clone(),
            }
        })
    }

    /// Save tokens.
    pub async fn save_tokens(&self, tokens: OAuthTokens) {
        let mut state = self.state.write().await;
        state.server_url = Some(self.server_url.clone());

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        state.tokens = Some(StoredTokens {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_in.map(|exp| now + exp),
            scope: tokens.scope,
        });

        info!(mcp = %self.mcp_name, "Saved OAuth tokens");
    }

    /// Generate PKCE code verifier.
    pub fn generate_code_verifier() -> String {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        URL_SAFE_NO_PAD.encode(&bytes)
    }

    /// Generate PKCE code challenge from verifier.
    pub fn generate_code_challenge(verifier: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let result = hasher.finalize();
        URL_SAFE_NO_PAD.encode(result)
    }

    /// Generate OAuth state parameter.
    pub fn generate_state() -> String {
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
        URL_SAFE_NO_PAD.encode(&bytes)
    }

    /// Save code verifier (during auth flow).
    pub async fn save_code_verifier(&self, verifier: String) {
        let mut state = self.state.write().await;
        state.code_verifier = Some(verifier);
    }

    /// Get code verifier.
    pub async fn code_verifier(&self) -> McpResult<String> {
        let state = self.state.read().await;
        state.code_verifier.clone().ok_or_else(|| {
            McpError::AuthFailed(format!("No code verifier saved for {}", self.mcp_name))
        })
    }

    /// Save OAuth state parameter.
    pub async fn save_oauth_state(&self, oauth_state: String) {
        let mut state = self.state.write().await;
        state.oauth_state = Some(oauth_state);
    }

    /// Get OAuth state parameter.
    pub async fn oauth_state(&self) -> McpResult<String> {
        let state = self.state.read().await;
        state.oauth_state.clone().ok_or_else(|| {
            McpError::AuthFailed(format!("No OAuth state saved for {}", self.mcp_name))
        })
    }

    /// Clear temporary auth state after completion.
    pub async fn clear_temp_state(&self) {
        let mut state = self.state.write().await;
        state.code_verifier = None;
        state.oauth_state = None;
    }

    /// Check if tokens are valid (not expired).
    pub async fn has_valid_tokens(&self) -> bool {
        let state = self.state.read().await;

        if let Some(ref tokens) = state.tokens {
            if let Some(expires_at) = tokens.expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                // Consider expired if less than 60 seconds left
                return expires_at > now + 60;
            }
            // No expiration = valid
            return true;
        }

        false
    }

    /// Get the access token if available and valid.
    pub async fn access_token(&self) -> Option<String> {
        if self.has_valid_tokens().await {
            let state = self.state.read().await;
            state.tokens.as_ref().map(|t| t.access_token.clone())
        } else {
            None
        }
    }
}

/// Build authorization URL.
pub fn build_auth_url(
    auth_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: Option<&str>,
    state: &str,
    code_challenge: &str,
) -> String {
    let mut url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
        auth_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(state),
        urlencoding::encode(code_challenge),
    );

    if let Some(scope) = scope {
        url.push_str(&format!("&scope={}", urlencoding::encode(scope)));
    }

    url
}

/// Exchange authorization code for tokens.
pub async fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> McpResult<OAuthTokens> {
    let client = reqwest::Client::new();

    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", code_verifier),
    ];

    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }

    let response = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| McpError::AuthFailed(format!("Token request failed: {e}")))?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(McpError::AuthFailed(format!(
            "Token exchange failed: {text}"
        )));
    }

    let tokens: OAuthTokens = response
        .json()
        .await
        .map_err(|e| McpError::AuthFailed(format!("Invalid token response: {e}")))?;

    Ok(tokens)
}

/// Refresh tokens using refresh token.
pub async fn refresh_tokens(
    token_endpoint: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> McpResult<OAuthTokens> {
    let client = reqwest::Client::new();

    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];

    if let Some(secret) = client_secret {
        params.push(("client_secret", secret));
    }

    let response = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| McpError::AuthFailed(format!("Refresh request failed: {e}")))?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(McpError::AuthFailed(format!(
            "Token refresh failed: {text}"
        )));
    }

    let tokens: OAuthTokens = response
        .json()
        .await
        .map_err(|e| McpError::AuthFailed(format!("Invalid refresh response: {e}")))?;

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_code_verifier() {
        let verifier = OAuthProvider::generate_code_verifier();
        assert!(!verifier.is_empty());
        // Base64url encoded 32 bytes = 43 characters
        assert!(verifier.len() >= 40);
    }

    #[test]
    fn test_generate_code_challenge() {
        let verifier = "test_verifier_12345678901234567890";
        let challenge = OAuthProvider::generate_code_challenge(verifier);
        assert!(!challenge.is_empty());
        // Should be base64url encoded SHA256 = 43 characters
        assert_eq!(challenge.len(), 43);
    }

    #[test]
    fn test_generate_state() {
        let state = OAuthProvider::generate_state();
        assert!(!state.is_empty());
    }

    #[test]
    fn test_build_auth_url() {
        let url = build_auth_url(
            "https://auth.example.com/authorize",
            "client123",
            "http://localhost:19876/callback",
            Some("read write"),
            "state123",
            "challenge123",
        );

        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client123"));
        assert!(url.contains("scope=read%20write"));
    }

    #[test]
    fn test_build_auth_url_no_scope() {
        let url = build_auth_url(
            "https://auth.example.com/authorize",
            "client123",
            "http://localhost:19876/callback",
            None,
            "state123",
            "challenge123",
        );

        assert!(url.contains("response_type=code"));
        assert!(!url.contains("scope="));
    }

    #[test]
    fn test_redirect_url() {
        let provider = OAuthProvider::new(
            "test".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        let url = provider.redirect_url();
        assert!(url.contains("127.0.0.1"));
        assert!(url.contains(&OAUTH_CALLBACK_PORT.to_string()));
    }

    #[test]
    fn test_oauth_config_default() {
        let config = OAuthConfig::default();
        assert!(config.client_id.is_none());
        assert!(config.client_secret.is_none());
        assert!(config.scope.is_none());
    }

    #[test]
    fn test_oauth_tokens_serialization() {
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("refresh456".to_string()),
            expires_in: Some(3600),
            scope: Some("read write".to_string()),
        };

        let json = serde_json::to_string(&tokens).unwrap();
        assert!(json.contains("access123"));
        assert!(json.contains("Bearer"));

        let parsed: OAuthTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.access_token, "access123");
        assert_eq!(parsed.refresh_token, Some("refresh456".to_string()));
    }

    #[test]
    fn test_oauth_tokens_minimal() {
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: None,
            scope: None,
        };

        let json = serde_json::to_string(&tokens).unwrap();
        assert!(!json.contains("refresh_token"));
        assert!(!json.contains("expires_in"));
        assert!(!json.contains("scope"));
    }

    #[test]
    fn test_client_info_serialization() {
        let info = ClientInfo {
            client_id: "client123".to_string(),
            client_secret: Some("secret456".to_string()),
            client_id_issued_at: Some(1234567890),
            client_secret_expires_at: Some(9876543210),
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: ClientInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.client_id, "client123");
        assert_eq!(parsed.client_secret, Some("secret456".to_string()));
    }

    #[test]
    fn test_oauth_state_default() {
        let state = OAuthState::default();
        assert!(state.server_url.is_none());
        assert!(state.client_info.is_none());
        assert!(state.tokens.is_none());
        assert!(state.code_verifier.is_none());
        assert!(state.oauth_state.is_none());
    }

    #[test]
    fn test_stored_tokens_serialization() {
        let tokens = StoredTokens {
            access_token: "access123".to_string(),
            refresh_token: Some("refresh456".to_string()),
            expires_at: Some(9999999999),
            scope: Some("read".to_string()),
        };

        let json = serde_json::to_string(&tokens).unwrap();
        let parsed: StoredTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.access_token, "access123");
        assert_eq!(parsed.expires_at, Some(9999999999));
    }

    #[tokio::test]
    async fn test_oauth_provider_new() {
        let config = OAuthConfig {
            client_id: Some("client123".to_string()),
            client_secret: Some("secret456".to_string()),
            scope: Some("read write".to_string()),
        };

        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            config,
        );

        // Check redirect URL
        let url = provider.redirect_url();
        assert!(url.contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn test_oauth_provider_client_info_from_config() {
        let config = OAuthConfig {
            client_id: Some("client123".to_string()),
            client_secret: Some("secret456".to_string()),
            scope: None,
        };

        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            config,
        );

        let info = provider.client_info().await;
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.client_id, "client123");
        assert_eq!(info.client_secret, Some("secret456".to_string()));
    }

    #[tokio::test]
    async fn test_oauth_provider_client_info_no_config() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // No client ID configured or stored
        let info = provider.client_info().await;
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_oauth_provider_save_client_info() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        let info = ClientInfo {
            client_id: "dynamic-client".to_string(),
            client_secret: None,
            client_id_issued_at: Some(1234567890),
            client_secret_expires_at: None,
        };

        provider.save_client_info(info.clone()).await;

        // Now client_info should return the saved info
        let retrieved = provider.client_info().await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().client_id, "dynamic-client");
    }

    #[tokio::test]
    async fn test_oauth_provider_tokens() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Initially no tokens
        let tokens = provider.tokens().await;
        assert!(tokens.is_none());

        // Save tokens
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("refresh456".to_string()),
            expires_in: Some(3600),
            scope: Some("read".to_string()),
        };
        provider.save_tokens(tokens).await;

        // Now tokens should be available
        let retrieved = provider.tokens().await;
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.access_token, "access123");
        assert_eq!(retrieved.token_type, "Bearer");
    }

    #[tokio::test]
    async fn test_oauth_provider_code_verifier() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Initially no code verifier
        let result = provider.code_verifier().await;
        assert!(result.is_err());

        // Save code verifier
        provider.save_code_verifier("verifier123".to_string()).await;

        // Now should be available
        let result = provider.code_verifier().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "verifier123");
    }

    #[tokio::test]
    async fn test_oauth_provider_oauth_state() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Initially no state
        let result = provider.oauth_state().await;
        assert!(result.is_err());

        // Save state
        provider.save_oauth_state("state123".to_string()).await;

        // Now should be available
        let result = provider.oauth_state().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "state123");
    }

    #[tokio::test]
    async fn test_oauth_provider_clear_temp_state() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Save temp state
        provider.save_code_verifier("verifier123".to_string()).await;
        provider.save_oauth_state("state123".to_string()).await;

        // Clear temp state
        provider.clear_temp_state().await;

        // Both should now be gone
        let verifier = provider.code_verifier().await;
        assert!(verifier.is_err());
        let state = provider.oauth_state().await;
        assert!(state.is_err());
    }

    #[tokio::test]
    async fn test_oauth_provider_has_valid_tokens() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Initially no tokens
        assert!(!provider.has_valid_tokens().await);

        // Save tokens with future expiry
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: Some(3600), // 1 hour
            scope: None,
        };
        provider.save_tokens(tokens).await;

        // Now has valid tokens
        assert!(provider.has_valid_tokens().await);
    }

    #[tokio::test]
    async fn test_oauth_provider_access_token() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Initially no access token
        let token = provider.access_token().await;
        assert!(token.is_none());

        // Save tokens
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: Some(3600),
            scope: None,
        };
        provider.save_tokens(tokens).await;

        // Now access token available
        let token = provider.access_token().await;
        assert!(token.is_some());
        assert_eq!(token.unwrap(), "access123");
    }

    #[test]
    fn test_client_metadata() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        let metadata = provider.client_metadata();
        assert!(metadata.contains_key("redirect_uris"));
        assert!(metadata.contains_key("client_name"));
        assert!(metadata.contains_key("grant_types"));
        assert!(metadata.contains_key("response_types"));
        assert!(metadata.contains_key("token_endpoint_auth_method"));

        // Without client_secret, auth method should be "none"
        let auth_method = metadata
            .get("token_endpoint_auth_method")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(auth_method, "none");
    }

    #[test]
    fn test_client_metadata_with_secret() {
        let config = OAuthConfig {
            client_id: Some("client123".to_string()),
            client_secret: Some("secret456".to_string()),
            scope: None,
        };

        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            config,
        );

        let metadata = provider.client_metadata();

        // With client_secret, auth method should be "client_secret_post"
        let auth_method = metadata
            .get("token_endpoint_auth_method")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(auth_method, "client_secret_post");
    }

    #[test]
    fn test_constants() {
        assert_eq!(OAUTH_CALLBACK_PORT, 19876);
        assert_eq!(OAUTH_CALLBACK_PATH, "/mcp/oauth/callback");
    }

    #[tokio::test]
    async fn test_oauth_provider_tokens_url_mismatch() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Save tokens
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: Some(3600),
            scope: None,
        };
        provider.save_tokens(tokens).await;

        // Create new provider with different URL
        let provider2 = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://different.com".to_string(),
            OAuthConfig::default(),
        );

        // Tokens should not be available for different URL
        let retrieved = provider2.tokens().await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_oauth_provider_client_info_url_mismatch() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Save client info
        let info = ClientInfo {
            client_id: "client123".to_string(),
            client_secret: None,
            client_id_issued_at: None,
            client_secret_expires_at: None,
        };
        provider.save_client_info(info).await;

        // Create new provider with different URL
        let provider2 = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://different.com".to_string(),
            OAuthConfig::default(),
        );

        // Client info should not be available for different URL
        let retrieved = provider2.client_info().await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_oauth_provider_expired_client_secret() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Save client info with expired secret
        let info = ClientInfo {
            client_id: "client123".to_string(),
            client_secret: Some("secret456".to_string()),
            client_id_issued_at: Some(1),
            client_secret_expires_at: Some(1), // Expired (timestamp = 1)
        };
        provider.save_client_info(info).await;

        // Client info should not be returned because secret is expired
        let retrieved = provider.client_info().await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_oauth_provider_no_expiry_tokens() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Save tokens without expiry
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: None, // No expiry
            scope: None,
        };
        provider.save_tokens(tokens).await;

        // Tokens with no expiry should be valid
        assert!(provider.has_valid_tokens().await);
        assert!(provider.access_token().await.is_some());
    }

    #[tokio::test]
    async fn test_oauth_provider_nearly_expired_tokens() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Manually create state with tokens about to expire
        {
            let mut state = provider.state.write().await;
            state.server_url = Some("https://example.com".to_string());
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            state.tokens = Some(StoredTokens {
                access_token: "access123".to_string(),
                refresh_token: None,
                expires_at: Some(now + 30), // Expires in 30 seconds (less than 60s buffer)
                scope: None,
            });
        }

        // Should not be considered valid due to 60s buffer
        assert!(!provider.has_valid_tokens().await);
    }

    #[tokio::test]
    async fn test_oauth_provider_tokens_with_expiry_calculation() {
        let provider = OAuthProvider::new(
            "test-mcp".to_string(),
            "https://example.com".to_string(),
            OAuthConfig::default(),
        );

        // Save tokens with expiry
        let tokens = OAuthTokens {
            access_token: "access123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("refresh456".to_string()),
            expires_in: Some(3600),
            scope: Some("read write".to_string()),
        };
        provider.save_tokens(tokens).await;

        // Retrieve and check
        let retrieved = provider.tokens().await.unwrap();
        assert_eq!(retrieved.access_token, "access123");
        assert_eq!(retrieved.token_type, "Bearer");
        assert_eq!(retrieved.refresh_token, Some("refresh456".to_string()));
        assert_eq!(retrieved.scope, Some("read write".to_string()));
        // expires_in should be roughly 3600 (slightly less due to time passing)
        assert!(retrieved.expires_in.unwrap() <= 3600);
        assert!(retrieved.expires_in.unwrap() >= 3590);
    }

    #[test]
    fn test_oauth_state_serialization() {
        let state = OAuthState {
            server_url: Some("https://example.com".to_string()),
            client_info: Some(ClientInfo {
                client_id: "client123".to_string(),
                client_secret: None,
                client_id_issued_at: None,
                client_secret_expires_at: None,
            }),
            tokens: Some(StoredTokens {
                access_token: "access".to_string(),
                refresh_token: None,
                expires_at: None,
                scope: None,
            }),
            code_verifier: Some("verifier".to_string()),
            oauth_state: Some("state".to_string()),
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: OAuthState = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.server_url, Some("https://example.com".to_string()));
        assert_eq!(parsed.client_info.unwrap().client_id, "client123");
    }

    #[test]
    fn test_client_info_minimal() {
        let info = ClientInfo {
            client_id: "client123".to_string(),
            client_secret: None,
            client_id_issued_at: None,
            client_secret_expires_at: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        // Optional fields should not appear when None
        assert!(!json.contains("client_secret"));
        assert!(!json.contains("client_id_issued_at"));
        assert!(!json.contains("client_secret_expires_at"));
    }

    #[test]
    fn test_stored_tokens_minimal() {
        let tokens = StoredTokens {
            access_token: "access123".to_string(),
            refresh_token: None,
            expires_at: None,
            scope: None,
        };

        let json = serde_json::to_string(&tokens).unwrap();
        let parsed: StoredTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.access_token, "access123");
        assert!(parsed.refresh_token.is_none());
    }

    #[test]
    fn test_build_auth_url_special_chars() {
        let url = build_auth_url(
            "https://auth.example.com/authorize",
            "client with spaces",
            "http://localhost:19876/callback?foo=bar",
            Some("read write admin"),
            "state=test&nonce=123",
            "challenge+123",
        );

        // Should be URL encoded
        assert!(url.contains("client%20with%20spaces"));
        assert!(url.contains("read%20write%20admin"));
    }

    #[test]
    fn test_generate_code_verifier_uniqueness() {
        let verifier1 = OAuthProvider::generate_code_verifier();
        let verifier2 = OAuthProvider::generate_code_verifier();
        // Each call should generate a unique verifier
        assert_ne!(verifier1, verifier2);
    }

    #[test]
    fn test_generate_state_uniqueness() {
        let state1 = OAuthProvider::generate_state();
        let state2 = OAuthProvider::generate_state();
        // Each call should generate a unique state
        assert_ne!(state1, state2);
    }

    #[test]
    fn test_generate_code_challenge_deterministic() {
        let verifier = "test_verifier_12345678901234567890";
        let challenge1 = OAuthProvider::generate_code_challenge(verifier);
        let challenge2 = OAuthProvider::generate_code_challenge(verifier);
        // Same verifier should produce same challenge
        assert_eq!(challenge1, challenge2);
    }
}
