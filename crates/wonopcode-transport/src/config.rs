//! Transport configuration.

use std::time::Duration;

/// Transport protocol to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportProtocol {
    /// TCP (default, most compatible).
    #[default]
    Tcp,
    /// QUIC (lower latency, better for mobile).
    Quic,
}

/// Configuration for the Iggy transport.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Iggy server address (host:port).
    pub server_address: String,

    /// Transport protocol to use.
    pub protocol: TransportProtocol,

    /// Stream name (default: "wonopcode").
    pub stream_name: String,

    /// Whether to automatically create stream/topics if they don't exist.
    pub auto_create: bool,

    /// Authentication username.
    pub username: String,

    /// Authentication password.
    pub password: String,

    /// Connection timeout.
    pub connect_timeout: Duration,

    /// Whether to use TLS.
    pub tls_enabled: bool,

    /// TLS domain (for certificate validation).
    pub tls_domain: Option<String>,

    /// Maximum reconnection retries (None = unlimited).
    pub reconnection_retries: Option<u32>,

    /// Interval between reconnection attempts.
    pub reconnection_interval: Duration,

    /// Heartbeat interval.
    pub heartbeat_interval: Duration,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            server_address: "127.0.0.1:8090".to_string(),
            protocol: TransportProtocol::Tcp,
            stream_name: crate::STREAM_NAME.to_string(),
            auto_create: true,
            username: "iggy".to_string(),
            password: "iggy".to_string(),
            connect_timeout: Duration::from_secs(10),
            tls_enabled: false,
            tls_domain: None,
            reconnection_retries: None, // unlimited
            reconnection_interval: Duration::from_secs(1),
            heartbeat_interval: Duration::from_secs(5),
        }
    }
}

impl TransportConfig {
    /// Create a new configuration with the given server address.
    pub fn new(server_address: impl Into<String>) -> Self {
        Self {
            server_address: server_address.into(),
            ..Default::default()
        }
    }

    /// Set the transport protocol.
    pub fn with_protocol(mut self, protocol: TransportProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Set the stream name.
    pub fn with_stream_name(mut self, name: impl Into<String>) -> Self {
        self.stream_name = name.into();
        self
    }

    /// Set auto-create behavior.
    pub fn with_auto_create(mut self, auto_create: bool) -> Self {
        self.auto_create = auto_create;
        self
    }

    /// Set authentication credentials.
    pub fn with_credentials(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.username = username.into();
        self.password = password.into();
        self
    }

    /// Enable TLS with optional domain.
    pub fn with_tls(mut self, domain: Option<String>) -> Self {
        self.tls_enabled = true;
        self.tls_domain = domain;
        self
    }

    /// Set reconnection parameters.
    pub fn with_reconnection(mut self, retries: Option<u32>, interval: Duration) -> Self {
        self.reconnection_retries = retries;
        self.reconnection_interval = interval;
        self
    }

    /// Build a connection string for Iggy's high-level SDK.
    pub fn connection_string(&self) -> String {
        let protocol = match self.protocol {
            TransportProtocol::Tcp => "tcp",
            TransportProtocol::Quic => "quic",
        };

        let tls_param = if self.tls_enabled {
            let domain = self.tls_domain.as_deref().unwrap_or("localhost");
            format!("&tls=true&tls_domain={}", domain)
        } else {
            String::new()
        };

        let reconnection_param = match self.reconnection_retries {
            Some(retries) => format!("&reconnection_retries={}", retries),
            None => "&reconnection_retries=unlimited".to_string(),
        };

        format!(
            "iggy+{}://{}:{}@{}?reconnection_interval={}s&heartbeat_interval={}s{}{}",
            protocol,
            self.username,
            self.password,
            self.server_address,
            self.reconnection_interval.as_secs(),
            self.heartbeat_interval.as_secs(),
            tls_param,
            reconnection_param,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = TransportConfig::default();
        assert_eq!(config.server_address, "127.0.0.1:8090");
        assert_eq!(config.protocol, TransportProtocol::Tcp);
        assert_eq!(config.stream_name, "wonopcode");
        assert!(config.auto_create);
    }

    #[test]
    fn builder_pattern() {
        let config = TransportConfig::new("localhost:9090")
            .with_protocol(TransportProtocol::Quic)
            .with_stream_name("custom-stream")
            .with_credentials("user", "pass")
            .with_auto_create(false);

        assert_eq!(config.server_address, "localhost:9090");
        assert_eq!(config.protocol, TransportProtocol::Quic);
        assert_eq!(config.stream_name, "custom-stream");
        assert_eq!(config.username, "user");
        assert_eq!(config.password, "pass");
        assert!(!config.auto_create);
    }

    #[test]
    fn connection_string_tcp() {
        let config = TransportConfig::default();
        let conn_str = config.connection_string();
        assert!(conn_str.starts_with("iggy+tcp://"));
        assert!(conn_str.contains("iggy:iggy@"));
        assert!(conn_str.contains("127.0.0.1:8090"));
    }

    #[test]
    fn connection_string_quic() {
        let config = TransportConfig::default().with_protocol(TransportProtocol::Quic);
        let conn_str = config.connection_string();
        assert!(conn_str.starts_with("iggy+quic://"));
    }

    #[test]
    fn connection_string_with_tls() {
        let config = TransportConfig::default().with_tls(Some("example.com".to_string()));
        let conn_str = config.connection_string();
        assert!(conn_str.contains("tls=true"));
        assert!(conn_str.contains("tls_domain=example.com"));
    }
}
