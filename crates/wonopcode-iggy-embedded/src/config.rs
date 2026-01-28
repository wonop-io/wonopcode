//! Configuration for embedded Iggy server.

use std::path::PathBuf;

/// Configuration for the embedded Iggy server.
#[derive(Debug, Clone)]
pub struct EmbeddedIggyConfig {
    /// Data directory for persistence (None = temporary/in-memory).
    pub data_dir: Option<PathBuf>,

    /// TCP port to bind to (0 = random available port).
    pub tcp_port: u16,

    /// Whether to enable QUIC transport.
    pub enable_quic: bool,

    /// QUIC port (0 = random available port).
    pub quic_port: u16,

    /// Whether to enable HTTP transport (for health checks and Web UI).
    pub enable_http: bool,

    /// HTTP port (0 = random available port).
    pub http_port: u16,

    /// Whether to enable the Web UI (requires HTTP).
    pub enable_web_ui: bool,

    /// Authentication username.
    pub username: String,

    /// Authentication password.
    pub password: String,

    /// Path to iggy-server binary (None = search in PATH).
    pub binary_path: Option<PathBuf>,

    /// Timeout for server startup in seconds.
    pub startup_timeout_secs: u64,
}

impl Default for EmbeddedIggyConfig {
    fn default() -> Self {
        Self {
            data_dir: None, // In-memory/temporary for standalone
            tcp_port: 0,    // Random available port
            enable_quic: false,
            quic_port: 0,
            enable_http: true, // For health checks
            http_port: 0,
            enable_web_ui: false,
            username: "iggy".to_string(),
            password: "iggy".to_string(),
            binary_path: None,
            startup_timeout_secs: 30,
        }
    }
}

impl EmbeddedIggyConfig {
    /// Create a new configuration with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a configuration for in-memory operation.
    ///
    /// Data is stored in a temporary directory and will be lost on restart.
    pub fn in_memory() -> Self {
        Self {
            data_dir: None,
            ..Default::default()
        }
    }

    /// Create a configuration with persistent storage.
    ///
    /// Data will be stored in the specified directory.
    pub fn persistent(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
            ..Default::default()
        }
    }

    /// Set the data directory.
    pub fn with_data_dir(mut self, dir: PathBuf) -> Self {
        self.data_dir = Some(dir);
        self
    }

    /// Set the TCP port (0 for random).
    pub fn with_tcp_port(mut self, port: u16) -> Self {
        self.tcp_port = port;
        self
    }

    /// Enable QUIC transport.
    pub fn with_quic(mut self, port: u16) -> Self {
        self.enable_quic = true;
        self.quic_port = port;
        self
    }

    /// Disable HTTP transport.
    pub fn without_http(mut self) -> Self {
        self.enable_http = false;
        self
    }

    /// Set authentication credentials.
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.username = username.into();
        self.password = password.into();
        self
    }

    /// Set path to iggy-server binary.
    pub fn with_binary_path(mut self, path: PathBuf) -> Self {
        self.binary_path = Some(path);
        self
    }

    /// Set startup timeout.
    pub fn with_startup_timeout(mut self, secs: u64) -> Self {
        self.startup_timeout_secs = secs;
        self
    }

    /// Generate TOML configuration for iggy-server.
    pub fn to_toml(
        &self,
        tcp_port: u16,
        http_port: u16,
        quic_port: u16,
        data_path: &str,
    ) -> String {
        let http_enabled = if self.enable_http { "true" } else { "false" };
        let quic_enabled = if self.enable_quic { "true" } else { "false" };
        let web_ui = if self.enable_web_ui { "true" } else { "false" };

        format!(
            r#"# Auto-generated Iggy configuration for Wonopcode
# This is a minimal configuration for embedded operation

[http]
enabled = {http_enabled}
address = "127.0.0.1:{http_port}"
web_ui = {web_ui}

[tcp]
enabled = true
address = "127.0.0.1:{tcp_port}"

[quic]
enabled = {quic_enabled}
address = "127.0.0.1:{quic_port}"

[system]
path = "{data_path}"

[system.logging]
level = "warn"
file_enabled = false

[system.encryption]
enabled = false

[system.message_deduplication]
enabled = false

[message_saver]
enabled = true
interval = "10 s"

[heartbeat]
enabled = false
"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = EmbeddedIggyConfig::default();
        assert!(config.data_dir.is_none());
        assert_eq!(config.tcp_port, 0);
        assert!(config.enable_http);
        assert!(!config.enable_quic);
    }

    #[test]
    fn in_memory_config() {
        let config = EmbeddedIggyConfig::in_memory();
        assert!(config.data_dir.is_none());
    }

    #[test]
    fn persistent_config() {
        let config = EmbeddedIggyConfig::persistent("/tmp/iggy".into());
        assert_eq!(config.data_dir, Some(PathBuf::from("/tmp/iggy")));
    }

    #[test]
    fn builder_pattern() {
        let config = EmbeddedIggyConfig::new()
            .with_tcp_port(9090)
            .with_quic(9091)
            .with_credentials("user", "pass");

        assert_eq!(config.tcp_port, 9090);
        assert!(config.enable_quic);
        assert_eq!(config.quic_port, 9091);
        assert_eq!(config.username, "user");
        assert_eq!(config.password, "pass");
    }

    #[test]
    fn toml_generation() {
        let config = EmbeddedIggyConfig::default();
        let toml = config.to_toml(8090, 3000, 8080, "/tmp/iggy");

        assert!(toml.contains("address = \"127.0.0.1:8090\""));
        assert!(toml.contains("[tcp]"));
        assert!(toml.contains("enabled = true"));
    }
}
