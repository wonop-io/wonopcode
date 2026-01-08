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
        format!(
            "http://127.0.0.1:{}{}",
            OAUTH_CALLBACK_PORT, OAUTH_CALLBACK_PATH
        )
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
        .map_err(|e| McpError::AuthFailed(format!("Token request failed: {}", e)))?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(McpError::AuthFailed(format!(
            "Token exchange failed: {}",
            text
        )));
    }

    let tokens: OAuthTokens = response
        .json()
        .await
        .map_err(|e| McpError::AuthFailed(format!("Invalid token response: {}", e)))?;

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
        .map_err(|e| McpError::AuthFailed(format!("Refresh request failed: {}", e)))?;

    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(McpError::AuthFailed(format!(
            "Token refresh failed: {}",
            text
        )));
    }

    let tokens: OAuthTokens = response
        .json()
        .await
        .map_err(|e| McpError::AuthFailed(format!("Invalid refresh response: {}", e)))?;

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
}
