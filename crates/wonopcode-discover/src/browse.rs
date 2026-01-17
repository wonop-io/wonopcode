//! Service discovery (browsing) via mDNS using native Bonjour/Avahi.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, info, trace, warn};
use zeroconf::prelude::*;
use zeroconf::{BrowserEvent, MdnsBrowser, ServiceType};

use crate::error::DiscoverError;
use crate::service::ServerInfo;

/// Browses for wonopcode servers on the local network via mDNS.
pub struct Browser {
    // Browser doesn't hold state - created fresh for each browse operation
}

impl Browser {
    /// Create a new browser.
    pub fn new() -> Result<Self, DiscoverError> {
        Ok(Self {})
    }

    /// Browse for servers with a timeout.
    ///
    /// Returns a list of discovered servers after the timeout expires.
    /// This is a blocking operation that waits for the full timeout duration.
    ///
    /// # Arguments
    /// * `timeout` - How long to wait for responses
    pub fn browse(&self, timeout: Duration) -> Result<Vec<ServerInfo>, DiscoverError> {
        info!(
            timeout_secs = timeout.as_secs_f32(),
            "Browsing for wonopcode servers"
        );

        let service_type = ServiceType::new("wonopcode", "tcp")
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        let mut browser = MdnsBrowser::new(service_type);

        // Shared state for collecting discovered services
        let servers: Arc<Mutex<HashMap<String, ServerInfo>>> = Arc::new(Mutex::new(HashMap::new()));
        let servers_clone = servers.clone();

        browser.set_service_callback(Box::new(move |result, _context| {
            on_service_event(result, servers_clone.clone());
        }));

        let event_loop = browser
            .browse_services()
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        // Poll until timeout
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let poll_time = remaining.min(Duration::from_millis(100));

            event_loop
                .poll(poll_time)
                .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;
        }

        // Extract results
        let result: Vec<ServerInfo> = servers.lock().unwrap().values().cloned().collect();

        info!(count = result.len(), "Browse completed");

        Ok(result)
    }

    /// Browse for a single server with a timeout.
    ///
    /// Returns as soon as a server is found, or None if timeout expires.
    pub fn browse_one(&self, timeout: Duration) -> Result<Option<ServerInfo>, DiscoverError> {
        debug!(
            timeout_secs = timeout.as_secs_f32(),
            "Browsing for first wonopcode server"
        );

        let service_type = ServiceType::new("wonopcode", "tcp")
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        let mut browser = MdnsBrowser::new(service_type);

        // Shared state for collecting discovered services
        let servers: Arc<Mutex<HashMap<String, ServerInfo>>> = Arc::new(Mutex::new(HashMap::new()));
        let servers_clone = servers.clone();

        browser.set_service_callback(Box::new(move |result, _context| {
            on_service_event(result, servers_clone.clone());
        }));

        let event_loop = browser
            .browse_services()
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        // Poll until timeout or we find a server
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let poll_time = remaining.min(Duration::from_millis(100));

            event_loop
                .poll(poll_time)
                .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

            // Check if we found a server
            let guard = servers.lock().unwrap();
            if !guard.is_empty() {
                let first = guard.values().next().cloned();
                return Ok(first);
            }
        }

        Ok(None)
    }
}

fn on_service_event(
    result: zeroconf::Result<BrowserEvent>,
    servers: Arc<Mutex<HashMap<String, ServerInfo>>>,
) {
    match result {
        Ok(BrowserEvent::Add(discovery)) => {
            trace!(
                name = %discovery.name(),
                service_type = ?discovery.service_type(),
                domain = %discovery.domain(),
                "Service discovered"
            );

            // Parse the discovery into a ServerInfo
            if let Some(server_info) = parse_discovery(&discovery) {
                debug!(
                    name = %server_info.name,
                    address = %server_info.address,
                    "Found server"
                );
                servers
                    .lock()
                    .unwrap()
                    .insert(discovery.name().to_string(), server_info);
            }
        }
        Ok(BrowserEvent::Remove(removal)) => {
            debug!(
                name = %removal.name(),
                "Service removed"
            );
            servers.lock().unwrap().remove(removal.name());
        }
        Err(e) => {
            warn!(error = %e, "Service discovery error");
        }
    }
}

fn parse_discovery(discovery: &zeroconf::ServiceDiscovery) -> Option<ServerInfo> {
    // Get address
    let address_str = discovery.address();
    let port = *discovery.port();

    // Parse the address - if it's 0.0.0.0, we'll try to use the hostname
    let ip: IpAddr = address_str.parse().ok()?;

    // For 0.0.0.0, use 127.0.0.1 for local connections
    // (The service is advertising on all interfaces, so localhost will work)
    let ip = if ip.is_unspecified() {
        "127.0.0.1".parse().unwrap()
    } else {
        ip
    };

    let address = SocketAddr::new(ip, port);

    let name = discovery.name().to_string();

    // Get the hostname for potential use in connections
    let hostname = {
        let h = discovery.host_name().trim_end_matches('.');
        if h.is_empty() {
            None
        } else {
            Some(h.to_string())
        }
    };

    // Parse TXT record for properties
    let txt = discovery.txt();
    let version = txt
        .as_ref()
        .and_then(|t| t.get("version"))
        .map(|s| s.to_string());
    let model = txt
        .as_ref()
        .and_then(|t| t.get("model"))
        .map(|s| s.to_string());
    let project = txt
        .as_ref()
        .and_then(|t| t.get("project"))
        .map(|s| s.to_string());
    let cwd = txt
        .as_ref()
        .and_then(|t| t.get("cwd"))
        .map(|s| s.to_string());
    let auth_required = txt
        .as_ref()
        .and_then(|t| t.get("auth"))
        .map(|s| s == "true")
        .unwrap_or(false);

    Some(ServerInfo {
        name,
        address,
        hostname,
        version,
        model,
        project,
        cwd,
        auth_required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_browser_creation() {
        let result = Browser::new();
        assert!(result.is_ok());
    }

    #[test]
    fn test_server_info_fields() {
        // Test that ServerInfo can be created with all fields
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8080);
        let info = ServerInfo {
            name: "TestServer".to_string(),
            address: addr,
            hostname: Some("test.local".to_string()),
            version: Some("1.0.0".to_string()),
            model: Some("claude-3".to_string()),
            project: Some("test-project".to_string()),
            cwd: Some("/home/user/project".to_string()),
            auth_required: true,
        };

        assert_eq!(info.name, "TestServer");
        assert_eq!(info.address.port(), 8080);
        assert_eq!(info.hostname, Some("test.local".to_string()));
        assert_eq!(info.version, Some("1.0.0".to_string()));
        assert_eq!(info.model, Some("claude-3".to_string()));
        assert_eq!(info.project, Some("test-project".to_string()));
        assert_eq!(info.cwd, Some("/home/user/project".to_string()));
        assert!(info.auth_required);
    }

    #[test]
    fn test_server_info_minimal() {
        // Test ServerInfo with minimal fields
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3000);
        let info = ServerInfo {
            name: "MinimalServer".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };

        assert_eq!(info.name, "MinimalServer");
        assert!(!info.auth_required);
        assert!(info.hostname.is_none());
        assert!(info.version.is_none());
    }

    #[test]
    fn test_server_info_clone_preserves_all_fields() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 9999);
        let original = ServerInfo {
            name: "CloneTest".to_string(),
            address: addr,
            hostname: Some("clone.local".to_string()),
            version: Some("2.0.0".to_string()),
            model: Some("gpt-4".to_string()),
            project: Some("clone-project".to_string()),
            cwd: Some("/var/clone".to_string()),
            auth_required: true,
        };

        let cloned = original.clone();

        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.address, original.address);
        assert_eq!(cloned.hostname, original.hostname);
        assert_eq!(cloned.version, original.version);
        assert_eq!(cloned.model, original.model);
        assert_eq!(cloned.project, original.project);
        assert_eq!(cloned.cwd, original.cwd);
        assert_eq!(cloned.auth_required, original.auth_required);
    }

    #[test]
    fn test_server_info_hashmap_key() {
        // Test that ServerInfo can be stored in a HashMap by name (like the browse function does)
        let mut servers: HashMap<String, ServerInfo> = HashMap::new();

        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8080);
        let info1 = ServerInfo {
            name: "Server1".to_string(),
            address: addr1,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };

        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 8081);
        let info2 = ServerInfo {
            name: "Server2".to_string(),
            address: addr2,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: true,
        };

        servers.insert(info1.name.clone(), info1);
        servers.insert(info2.name.clone(), info2);

        assert_eq!(servers.len(), 2);
        assert!(servers.contains_key("Server1"));
        assert!(servers.contains_key("Server2"));
    }

    #[test]
    fn test_server_info_remove_from_hashmap() {
        // Test the removal pattern used in on_service_event
        let mut servers: HashMap<String, ServerInfo> = HashMap::new();

        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8080);
        let info = ServerInfo {
            name: "ToRemove".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };

        servers.insert("ToRemove".to_string(), info);
        assert_eq!(servers.len(), 1);

        servers.remove("ToRemove");
        assert_eq!(servers.len(), 0);
    }

    #[test]
    fn test_ipv6_address() {
        // Test that ServerInfo works with IPv6 addresses
        let addr = SocketAddr::new(
            IpAddr::V6(std::net::Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
            8080,
        );
        let info = ServerInfo {
            name: "IPv6Server".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };

        assert!(info.address.is_ipv6());
        assert_eq!(info.address.port(), 8080);
    }
}
