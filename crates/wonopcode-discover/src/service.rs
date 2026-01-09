//! Service information types.

use std::net::SocketAddr;

/// The mDNS service type for wonopcode servers.
pub const SERVICE_TYPE: &str = "_wonopcode._tcp.local.";

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
            write!(f, " [{}]", project)?;
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
