//! Service information types.

use std::net::SocketAddr;

/// The mDNS service type for wonopcode servers.
pub const SERVICE_TYPE: &str = "_wonopcode._tcp.local.";

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_service_type_constant() {
        assert_eq!(SERVICE_TYPE, "_wonopcode._tcp.local.");
    }

    #[test]
    fn test_server_info_display_basic() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8080);
        let info = ServerInfo {
            name: "TestServer".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };
        let display = format!("{}", info);
        assert_eq!(display, "TestServer (192.168.1.100:8080)");
    }

    #[test]
    fn test_server_info_display_with_project() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8080);
        let info = ServerInfo {
            name: "TestServer".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: Some("my-project".to_string()),
            cwd: None,
            auth_required: false,
        };
        let display = format!("{}", info);
        assert_eq!(display, "TestServer (192.168.1.100:8080) [my-project]");
    }

    #[test]
    fn test_server_info_display_with_auth() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8080);
        let info = ServerInfo {
            name: "TestServer".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: true,
        };
        let display = format!("{}", info);
        assert_eq!(display, "TestServer (192.168.1.100:8080) 🔒");
    }

    #[test]
    fn test_server_info_display_with_project_and_auth() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8080);
        let info = ServerInfo {
            name: "TestServer".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: Some("my-project".to_string()),
            cwd: None,
            auth_required: true,
        };
        let display = format!("{}", info);
        assert_eq!(display, "TestServer (192.168.1.100:8080) [my-project] 🔒");
    }

    #[test]
    fn test_server_info_clone() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3000);
        let info = ServerInfo {
            name: "Original".to_string(),
            address: addr,
            hostname: Some("host.local".to_string()),
            version: Some("1.0.0".to_string()),
            model: Some("claude-3".to_string()),
            project: Some("project".to_string()),
            cwd: Some("/home/user".to_string()),
            auth_required: true,
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, info.name);
        assert_eq!(cloned.address, info.address);
        assert_eq!(cloned.hostname, info.hostname);
        assert_eq!(cloned.version, info.version);
        assert_eq!(cloned.model, info.model);
        assert_eq!(cloned.project, info.project);
        assert_eq!(cloned.cwd, info.cwd);
        assert_eq!(cloned.auth_required, info.auth_required);
    }

    #[test]
    fn test_server_info_debug() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3000);
        let info = ServerInfo {
            name: "Test".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("ServerInfo"));
        assert!(debug_str.contains("Test"));
    }

    #[test]
    fn test_advertise_config_new() {
        let config = AdvertiseConfig::new("MyServer", 8080, "1.0.0");
        assert_eq!(config.name, "MyServer");
        assert_eq!(config.port, 8080);
        assert_eq!(config.version, "1.0.0");
        assert!(config.model.is_none());
        assert!(config.project.is_none());
        assert!(config.cwd.is_none());
        assert!(!config.auth_required);
    }

    #[test]
    fn test_advertise_config_with_model() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0").with_model("claude-3-opus");
        assert_eq!(config.model, Some("claude-3-opus".to_string()));
    }

    #[test]
    fn test_advertise_config_with_project() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0").with_project("my-project");
        assert_eq!(config.project, Some("my-project".to_string()));
    }

    #[test]
    fn test_advertise_config_with_cwd() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0").with_cwd("/home/user/code");
        assert_eq!(config.cwd, Some("/home/user/code".to_string()));
    }

    #[test]
    fn test_advertise_config_with_auth() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0").with_auth(true);
        assert!(config.auth_required);

        let config2 = AdvertiseConfig::new("Server", 8080, "1.0").with_auth(false);
        assert!(!config2.auth_required);
    }

    #[test]
    fn test_advertise_config_builder_chain() {
        let config = AdvertiseConfig::new("FullServer", 9000, "2.0.0")
            .with_model("gpt-4")
            .with_project("awesome-project")
            .with_cwd("/workspace")
            .with_auth(true);

        assert_eq!(config.name, "FullServer");
        assert_eq!(config.port, 9000);
        assert_eq!(config.version, "2.0.0");
        assert_eq!(config.model, Some("gpt-4".to_string()));
        assert_eq!(config.project, Some("awesome-project".to_string()));
        assert_eq!(config.cwd, Some("/workspace".to_string()));
        assert!(config.auth_required);
    }

    #[test]
    fn test_advertise_config_clone() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0")
            .with_model("model")
            .with_project("proj")
            .with_cwd("/cwd")
            .with_auth(true);
        let cloned = config.clone();
        assert_eq!(cloned.name, config.name);
        assert_eq!(cloned.port, config.port);
        assert_eq!(cloned.version, config.version);
        assert_eq!(cloned.model, config.model);
        assert_eq!(cloned.project, config.project);
        assert_eq!(cloned.cwd, config.cwd);
        assert_eq!(cloned.auth_required, config.auth_required);
    }

    #[test]
    fn test_advertise_config_debug() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0");
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("AdvertiseConfig"));
        assert!(debug_str.contains("Server"));
    }

    #[test]
    fn test_advertise_config_with_string_types() {
        // Test that Into<String> works for various string types
        let config = AdvertiseConfig::new(String::from("Server"), 8080, String::from("1.0"))
            .with_model(String::from("model"))
            .with_project(String::from("project"))
            .with_cwd(String::from("/cwd"));
        assert_eq!(config.name, "Server");
        assert_eq!(config.version, "1.0");
        assert_eq!(config.model, Some("model".to_string()));
    }
}

/// Information about a discovered wonopcode server.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    /// Display name of the server instance.
    pub name: String,
    /// Socket address (IP:port) of the server.
    pub address: SocketAddr,
    /// Hostname of the server (e.g., "machine.local").
    pub hostname: Option<String>,
    /// Wonopcode version.
    pub version: Option<String>,
    /// Current AI model.
    pub model: Option<String>,
    /// Project name.
    pub project: Option<String>,
    /// Working directory.
    pub cwd: Option<String>,
    /// Whether authentication is required.
    pub auth_required: bool,
}

impl std::fmt::Display for ServerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.address)?;
        if let Some(ref project) = self.project {
            write!(f, " [{project}]")?;
        }
        if self.auth_required {
            write!(f, " 🔒")?;
        }
        Ok(())
    }
}

/// Configuration for advertising a server.
#[derive(Debug, Clone)]
pub struct AdvertiseConfig {
    /// Display name for the server.
    pub name: String,
    /// Port the server is listening on.
    pub port: u16,
    /// Wonopcode version.
    pub version: String,
    /// Current AI model.
    pub model: Option<String>,
    /// Project name.
    pub project: Option<String>,
    /// Working directory.
    pub cwd: Option<String>,
    /// Whether authentication is required.
    pub auth_required: bool,
}

impl AdvertiseConfig {
    /// Create a new advertise config with required fields.
    pub fn new(name: impl Into<String>, port: u16, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            port,
            version: version.into(),
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        }
    }

    /// Set the model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the project name.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Set the working directory.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set whether authentication is required.
    pub fn with_auth(mut self, required: bool) -> Self {
        self.auth_required = required;
        self
    }
}
