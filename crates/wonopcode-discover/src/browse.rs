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

// ============================================================================
// Helper functions for building ServerInfo (public for testing)
// ============================================================================

/// Build a ServerInfo from raw discovery data.
/// This is a testable helper function.
pub fn build_server_info(
    name: String,
    address: SocketAddr,
    hostname: Option<String>,
    txt_records: Option<HashMap<String, String>>,
) -> ServerInfo {
    let version = txt_records
        .as_ref()
        .and_then(|t| t.get("version"))
        .cloned();
    let model = txt_records.as_ref().and_then(|t| t.get("model")).cloned();
    let project = txt_records
        .as_ref()
        .and_then(|t| t.get("project"))
        .cloned();
    let cwd = txt_records.as_ref().and_then(|t| t.get("cwd")).cloned();
    let auth_required = txt_records
        .as_ref()
        .and_then(|t| t.get("auth"))
        .map(|s| s == "true")
        .unwrap_or(false);

    ServerInfo {
        name,
        address,
        hostname,
        version,
        model,
        project,
        cwd,
        auth_required,
    }
}

/// Normalize an IP address - converts 0.0.0.0 to 127.0.0.1.
pub fn normalize_ip(ip: IpAddr) -> IpAddr {
    if ip.is_unspecified() {
        "127.0.0.1".parse().unwrap()
    } else {
        ip
    }
}

/// Parse a hostname, trimming trailing dots and returning None for empty strings.
pub fn parse_hostname(hostname: &str) -> Option<String> {
    let h = hostname.trim_end_matches('.');
    if h.is_empty() {
        None
    } else {
        Some(h.to_string())
    }
}

/// Handle a service add event by inserting into the servers map.
pub fn handle_service_add(
    servers: &Mutex<HashMap<String, ServerInfo>>,
    name: String,
    server_info: ServerInfo,
) {
    servers.lock().unwrap().insert(name, server_info);
}

/// Handle a service remove event by removing from the servers map.
pub fn handle_service_remove(servers: &Mutex<HashMap<String, ServerInfo>>, name: &str) {
    servers.lock().unwrap().remove(name);
}

/// Parse an IP address string and normalize it.
/// Returns None if the address is invalid.
pub fn parse_and_normalize_address(address_str: &str, port: u16) -> Option<SocketAddr> {
    let ip: IpAddr = address_str.parse().ok()?;
    let normalized = normalize_ip(ip);
    Some(SocketAddr::new(normalized, port))
}

/// Extract version from TXT records.
pub fn extract_version(txt: &Option<HashMap<String, String>>) -> Option<String> {
    txt.as_ref().and_then(|t| t.get("version")).cloned()
}

/// Extract model from TXT records.
pub fn extract_model(txt: &Option<HashMap<String, String>>) -> Option<String> {
    txt.as_ref().and_then(|t| t.get("model")).cloned()
}

/// Extract project from TXT records.
pub fn extract_project(txt: &Option<HashMap<String, String>>) -> Option<String> {
    txt.as_ref().and_then(|t| t.get("project")).cloned()
}

/// Extract cwd from TXT records.
pub fn extract_cwd(txt: &Option<HashMap<String, String>>) -> Option<String> {
    txt.as_ref().and_then(|t| t.get("cwd")).cloned()
}

/// Extract auth_required from TXT records.
pub fn extract_auth_required(txt: &Option<HashMap<String, String>>) -> bool {
    txt.as_ref()
        .and_then(|t| t.get("auth"))
        .map(|s| s == "true")
        .unwrap_or(false)
}

/// Build a complete ServerInfo from all parts.
/// This is the full builder function that combines all extraction helpers.
pub fn build_complete_server_info(
    name: String,
    address_str: &str,
    port: u16,
    hostname_raw: &str,
    txt: Option<HashMap<String, String>>,
) -> Option<ServerInfo> {
    let address = parse_and_normalize_address(address_str, port)?;
    let hostname = parse_hostname(hostname_raw);
    let version = extract_version(&txt);
    let model = extract_model(&txt);
    let project = extract_project(&txt);
    let cwd = extract_cwd(&txt);
    let auth_required = extract_auth_required(&txt);

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

/// Check if a hostname is valid (non-empty after trimming dots).
pub fn is_valid_hostname(hostname: &str) -> bool {
    !hostname.trim_end_matches('.').is_empty()
}

/// Parse an auth value from a TXT record string.
pub fn parse_auth_value(value: &str) -> bool {
    value == "true"
}

/// Extract all known fields from TXT records into a struct-like format.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TxtFields {
    pub version: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub cwd: Option<String>,
    pub auth_required: bool,
}

/// Extract all fields from TXT records at once.
pub fn extract_all_txt_fields(txt: &Option<HashMap<String, String>>) -> TxtFields {
    TxtFields {
        version: extract_version(txt),
        model: extract_model(txt),
        project: extract_project(txt),
        cwd: extract_cwd(txt),
        auth_required: extract_auth_required(txt),
    }
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

    // ========================================================================
    // Tests for new helper functions
    // ========================================================================

    #[test]
    fn test_build_server_info_with_all_txt_records() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8080);
        let mut txt = HashMap::new();
        txt.insert("version".to_string(), "1.0.0".to_string());
        txt.insert("model".to_string(), "claude-3".to_string());
        txt.insert("project".to_string(), "my-project".to_string());
        txt.insert("cwd".to_string(), "/home/user".to_string());
        txt.insert("auth".to_string(), "true".to_string());

        let info = build_server_info(
            "TestServer".to_string(),
            addr,
            Some("test.local".to_string()),
            Some(txt),
        );

        assert_eq!(info.name, "TestServer");
        assert_eq!(info.address, addr);
        assert_eq!(info.hostname, Some("test.local".to_string()));
        assert_eq!(info.version, Some("1.0.0".to_string()));
        assert_eq!(info.model, Some("claude-3".to_string()));
        assert_eq!(info.project, Some("my-project".to_string()));
        assert_eq!(info.cwd, Some("/home/user".to_string()));
        assert!(info.auth_required);
    }

    #[test]
    fn test_build_server_info_with_no_txt_records() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 8080);

        let info = build_server_info("TestServer".to_string(), addr, None, None);

        assert_eq!(info.name, "TestServer");
        assert!(info.version.is_none());
        assert!(info.model.is_none());
        assert!(info.project.is_none());
        assert!(info.cwd.is_none());
        assert!(!info.auth_required);
    }

    #[test]
    fn test_build_server_info_with_partial_txt_records() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 3000);
        let mut txt = HashMap::new();
        txt.insert("version".to_string(), "2.0".to_string());
        txt.insert("auth".to_string(), "false".to_string());

        let info = build_server_info("PartialServer".to_string(), addr, None, Some(txt));

        assert_eq!(info.version, Some("2.0".to_string()));
        assert!(info.model.is_none());
        assert!(!info.auth_required);
    }

    #[test]
    fn test_build_server_info_auth_true() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let mut txt = HashMap::new();
        txt.insert("auth".to_string(), "true".to_string());

        let info = build_server_info("AuthServer".to_string(), addr, None, Some(txt));
        assert!(info.auth_required);
    }

    #[test]
    fn test_build_server_info_auth_false() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let mut txt = HashMap::new();
        txt.insert("auth".to_string(), "false".to_string());

        let info = build_server_info("NoAuthServer".to_string(), addr, None, Some(txt));
        assert!(!info.auth_required);
    }

    #[test]
    fn test_build_server_info_auth_invalid() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let mut txt = HashMap::new();
        txt.insert("auth".to_string(), "invalid".to_string());

        let info = build_server_info("InvalidAuthServer".to_string(), addr, None, Some(txt));
        assert!(!info.auth_required); // anything that's not "true" should be false
    }

    #[test]
    fn test_normalize_ip_unspecified_v4() {
        let ip: IpAddr = "0.0.0.0".parse().unwrap();
        let normalized = normalize_ip(ip);
        assert_eq!(normalized, "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_normalize_ip_unspecified_v6() {
        let ip: IpAddr = "::".parse().unwrap();
        let normalized = normalize_ip(ip);
        assert_eq!(normalized, "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_normalize_ip_regular_v4() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let normalized = normalize_ip(ip);
        assert_eq!(normalized, ip);
    }

    #[test]
    fn test_normalize_ip_regular_v6() {
        let ip: IpAddr = "fe80::1".parse().unwrap();
        let normalized = normalize_ip(ip);
        assert_eq!(normalized, ip);
    }

    #[test]
    fn test_normalize_ip_localhost_v4() {
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let normalized = normalize_ip(ip);
        assert_eq!(normalized, ip);
    }

    #[test]
    fn test_parse_hostname_with_trailing_dot() {
        let result = parse_hostname("example.local.");
        assert_eq!(result, Some("example.local".to_string()));
    }

    #[test]
    fn test_parse_hostname_without_trailing_dot() {
        let result = parse_hostname("example.local");
        assert_eq!(result, Some("example.local".to_string()));
    }

    #[test]
    fn test_parse_hostname_empty_string() {
        let result = parse_hostname("");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_hostname_only_dots() {
        let result = parse_hostname("...");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_hostname_single_dot() {
        let result = parse_hostname(".");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_hostname_multiple_trailing_dots() {
        let result = parse_hostname("example.local...");
        assert_eq!(result, Some("example.local".to_string()));
    }

    #[test]
    fn test_handle_service_add() {
        let servers: Mutex<HashMap<String, ServerInfo>> = Mutex::new(HashMap::new());
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8080);
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

        handle_service_add(&servers, "TestServer".to_string(), info);

        let guard = servers.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert!(guard.contains_key("TestServer"));
    }

    #[test]
    fn test_handle_service_add_replaces_existing() {
        let servers: Mutex<HashMap<String, ServerInfo>> = Mutex::new(HashMap::new());
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8080);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 9090);

        let info1 = ServerInfo {
            name: "TestServer".to_string(),
            address: addr1,
            hostname: None,
            version: Some("1.0".to_string()),
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };

        let info2 = ServerInfo {
            name: "TestServer".to_string(),
            address: addr2,
            hostname: None,
            version: Some("2.0".to_string()),
            model: None,
            project: None,
            cwd: None,
            auth_required: true,
        };

        handle_service_add(&servers, "TestServer".to_string(), info1);
        handle_service_add(&servers, "TestServer".to_string(), info2);

        let guard = servers.lock().unwrap();
        assert_eq!(guard.len(), 1);
        let server = guard.get("TestServer").unwrap();
        assert_eq!(server.version, Some("2.0".to_string()));
        assert!(server.auth_required);
    }

    #[test]
    fn test_handle_service_remove() {
        let servers: Mutex<HashMap<String, ServerInfo>> = Mutex::new(HashMap::new());
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

        {
            servers.lock().unwrap().insert("ToRemove".to_string(), info);
        }

        assert_eq!(servers.lock().unwrap().len(), 1);

        handle_service_remove(&servers, "ToRemove");

        assert_eq!(servers.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_handle_service_remove_nonexistent() {
        let servers: Mutex<HashMap<String, ServerInfo>> = Mutex::new(HashMap::new());

        // Should not panic when removing non-existent key
        handle_service_remove(&servers, "DoesNotExist");

        assert_eq!(servers.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_handle_service_add_multiple() {
        let servers: Mutex<HashMap<String, ServerInfo>> = Mutex::new(HashMap::new());

        for i in 0..5 {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, i as u8)), 8080);
            let info = ServerInfo {
                name: format!("Server{}", i),
                address: addr,
                hostname: None,
                version: None,
                model: None,
                project: None,
                cwd: None,
                auth_required: false,
            };
            handle_service_add(&servers, format!("Server{}", i), info);
        }

        let guard = servers.lock().unwrap();
        assert_eq!(guard.len(), 5);
        for i in 0..5 {
            assert!(guard.contains_key(&format!("Server{}", i)));
        }
    }

    // ========================================================================
    // Tests for parse_and_normalize_address
    // ========================================================================

    #[test]
    fn test_parse_and_normalize_address_valid_ipv4() {
        let result = parse_and_normalize_address("192.168.1.100", 8080);
        assert!(result.is_some());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 8080);
        assert_eq!(addr.ip().to_string(), "192.168.1.100");
    }

    #[test]
    fn test_parse_and_normalize_address_unspecified() {
        let result = parse_and_normalize_address("0.0.0.0", 3000);
        assert!(result.is_some());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 3000);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn test_parse_and_normalize_address_invalid() {
        let result = parse_and_normalize_address("not-an-ip", 8080);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_and_normalize_address_empty() {
        let result = parse_and_normalize_address("", 8080);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_and_normalize_address_ipv6() {
        let result = parse_and_normalize_address("::1", 9000);
        assert!(result.is_some());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 9000);
    }

    #[test]
    fn test_parse_and_normalize_address_ipv6_unspecified() {
        let result = parse_and_normalize_address("::", 9000);
        assert!(result.is_some());
        let addr = result.unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    // ========================================================================
    // Tests for extract_* functions
    // ========================================================================

    #[test]
    fn test_extract_version_present() {
        let mut txt = HashMap::new();
        txt.insert("version".to_string(), "1.0.0".to_string());
        let result = extract_version(&Some(txt));
        assert_eq!(result, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_extract_version_absent() {
        let txt: HashMap<String, String> = HashMap::new();
        let result = extract_version(&Some(txt));
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_version_none_txt() {
        let result = extract_version(&None);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_model_present() {
        let mut txt = HashMap::new();
        txt.insert("model".to_string(), "claude-3".to_string());
        let result = extract_model(&Some(txt));
        assert_eq!(result, Some("claude-3".to_string()));
    }

    #[test]
    fn test_extract_model_absent() {
        let result = extract_model(&None);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_project_present() {
        let mut txt = HashMap::new();
        txt.insert("project".to_string(), "my-project".to_string());
        let result = extract_project(&Some(txt));
        assert_eq!(result, Some("my-project".to_string()));
    }

    #[test]
    fn test_extract_project_absent() {
        let result = extract_project(&None);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_cwd_present() {
        let mut txt = HashMap::new();
        txt.insert("cwd".to_string(), "/home/user".to_string());
        let result = extract_cwd(&Some(txt));
        assert_eq!(result, Some("/home/user".to_string()));
    }

    #[test]
    fn test_extract_cwd_absent() {
        let result = extract_cwd(&None);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_auth_required_true() {
        let mut txt = HashMap::new();
        txt.insert("auth".to_string(), "true".to_string());
        let result = extract_auth_required(&Some(txt));
        assert!(result);
    }

    #[test]
    fn test_extract_auth_required_false() {
        let mut txt = HashMap::new();
        txt.insert("auth".to_string(), "false".to_string());
        let result = extract_auth_required(&Some(txt));
        assert!(!result);
    }

    #[test]
    fn test_extract_auth_required_absent() {
        let result = extract_auth_required(&None);
        assert!(!result);
    }

    #[test]
    fn test_extract_auth_required_invalid_value() {
        let mut txt = HashMap::new();
        txt.insert("auth".to_string(), "yes".to_string());
        let result = extract_auth_required(&Some(txt));
        assert!(!result); // Only "true" should return true
    }

    #[test]
    fn test_extract_auth_required_empty_map() {
        let txt: HashMap<String, String> = HashMap::new();
        let result = extract_auth_required(&Some(txt));
        assert!(!result);
    }

    // ========================================================================
    // Additional ServerInfo tests
    // ========================================================================

    #[test]
    fn test_server_info_with_all_optional_none() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 8080);
        let info = ServerInfo {
            name: "Minimal".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };
        assert!(info.hostname.is_none());
        assert!(info.version.is_none());
        assert!(info.model.is_none());
        assert!(info.project.is_none());
        assert!(info.cwd.is_none());
    }

    #[test]
    fn test_server_info_port_extraction() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 12345);
        let info = ServerInfo {
            name: "PortTest".to_string(),
            address: addr,
            hostname: None,
            version: None,
            model: None,
            project: None,
            cwd: None,
            auth_required: false,
        };
        assert_eq!(info.address.port(), 12345);
    }

    #[test]
    fn test_normalize_ip_loopback_unchanged() {
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let normalized = normalize_ip(ip);
        assert_eq!(normalized.to_string(), "127.0.0.1");
    }

    #[test]
    fn test_normalize_ip_private_unchanged() {
        let ip: IpAddr = "192.168.0.1".parse().unwrap();
        let normalized = normalize_ip(ip);
        assert_eq!(normalized.to_string(), "192.168.0.1");
    }

    #[test]
    fn test_build_server_info_with_empty_hostname() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let info = build_server_info("Test".to_string(), addr, Some("".to_string()), None);
        // Empty hostname is preserved
        assert_eq!(info.hostname, Some("".to_string()));
    }

    #[test]
    fn test_parse_hostname_leading_trailing_dots() {
        let result = parse_hostname("...test...");
        // trim_end_matches only trims trailing dots
        assert_eq!(result, Some("...test".to_string()));
    }

    #[test]
    fn test_mutex_concurrent_access() {
        use std::thread;
        let servers: Arc<Mutex<HashMap<String, ServerInfo>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let servers = servers.clone();
                thread::spawn(move || {
                    let addr =
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, i as u8)), 8080);
                    let info = ServerInfo {
                        name: format!("Server{}", i),
                        address: addr,
                        hostname: None,
                        version: None,
                        model: None,
                        project: None,
                        cwd: None,
                        auth_required: false,
                    };
                    handle_service_add(&servers, format!("Server{}", i), info);
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(servers.lock().unwrap().len(), 10);
    }

    // ========================================================================
    // Tests for build_complete_server_info
    // ========================================================================

    #[test]
    fn test_build_complete_server_info_valid() {
        let mut txt = HashMap::new();
        txt.insert("version".to_string(), "1.0.0".to_string());
        txt.insert("model".to_string(), "claude-3".to_string());
        txt.insert("project".to_string(), "my-project".to_string());
        txt.insert("cwd".to_string(), "/home/user".to_string());
        txt.insert("auth".to_string(), "true".to_string());

        let result = build_complete_server_info(
            "TestServer".to_string(),
            "192.168.1.100",
            8080,
            "test.local.",
            Some(txt),
        );

        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.name, "TestServer");
        assert_eq!(info.address.port(), 8080);
        assert_eq!(info.hostname, Some("test.local".to_string()));
        assert_eq!(info.version, Some("1.0.0".to_string()));
        assert_eq!(info.model, Some("claude-3".to_string()));
        assert_eq!(info.project, Some("my-project".to_string()));
        assert_eq!(info.cwd, Some("/home/user".to_string()));
        assert!(info.auth_required);
    }

    #[test]
    fn test_build_complete_server_info_invalid_address() {
        let result = build_complete_server_info(
            "Test".to_string(),
            "not-an-ip",
            8080,
            "test.local",
            None,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_build_complete_server_info_minimal() {
        let result = build_complete_server_info(
            "Minimal".to_string(),
            "127.0.0.1",
            3000,
            "",
            None,
        );

        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.name, "Minimal");
        assert!(info.hostname.is_none());
        assert!(info.version.is_none());
        assert!(!info.auth_required);
    }

    #[test]
    fn test_build_complete_server_info_unspecified_address() {
        let result = build_complete_server_info(
            "Unspecified".to_string(),
            "0.0.0.0",
            8080,
            "host.local",
            None,
        );

        assert!(result.is_some());
        let info = result.unwrap();
        // 0.0.0.0 should be normalized to 127.0.0.1
        assert_eq!(info.address.ip().to_string(), "127.0.0.1");
    }

    // ========================================================================
    // Tests for is_valid_hostname
    // ========================================================================

    #[test]
    fn test_is_valid_hostname_valid() {
        assert!(is_valid_hostname("test.local"));
        assert!(is_valid_hostname("test.local."));
        assert!(is_valid_hostname("a"));
        assert!(is_valid_hostname("test..."));
    }

    #[test]
    fn test_is_valid_hostname_invalid() {
        assert!(!is_valid_hostname(""));
        assert!(!is_valid_hostname("."));
        assert!(!is_valid_hostname("..."));
    }

    // ========================================================================
    // Tests for parse_auth_value
    // ========================================================================

    #[test]
    fn test_parse_auth_value_true() {
        assert!(parse_auth_value("true"));
    }

    #[test]
    fn test_parse_auth_value_false() {
        assert!(!parse_auth_value("false"));
        assert!(!parse_auth_value("True"));
        assert!(!parse_auth_value("TRUE"));
        assert!(!parse_auth_value("1"));
        assert!(!parse_auth_value("yes"));
        assert!(!parse_auth_value(""));
    }

    // ========================================================================
    // Tests for TxtFields
    // ========================================================================

    #[test]
    fn test_txt_fields_default() {
        let fields = TxtFields::default();
        assert!(fields.version.is_none());
        assert!(fields.model.is_none());
        assert!(fields.project.is_none());
        assert!(fields.cwd.is_none());
        assert!(!fields.auth_required);
    }

    #[test]
    fn test_txt_fields_debug() {
        let fields = TxtFields {
            version: Some("1.0".to_string()),
            model: None,
            project: None,
            cwd: None,
            auth_required: true,
        };
        let debug = format!("{:?}", fields);
        assert!(debug.contains("TxtFields"));
        assert!(debug.contains("1.0"));
    }

    #[test]
    fn test_txt_fields_clone() {
        let fields = TxtFields {
            version: Some("2.0".to_string()),
            model: Some("gpt-4".to_string()),
            project: None,
            cwd: None,
            auth_required: false,
        };
        let cloned = fields.clone();
        assert_eq!(fields, cloned);
    }

    #[test]
    fn test_txt_fields_equality() {
        let fields1 = TxtFields {
            version: Some("1.0".to_string()),
            model: None,
            project: None,
            cwd: None,
            auth_required: true,
        };
        let fields2 = TxtFields {
            version: Some("1.0".to_string()),
            model: None,
            project: None,
            cwd: None,
            auth_required: true,
        };
        assert_eq!(fields1, fields2);

        let fields3 = TxtFields::default();
        assert_ne!(fields1, fields3);
    }

    // ========================================================================
    // Tests for extract_all_txt_fields
    // ========================================================================

    #[test]
    fn test_extract_all_txt_fields_full() {
        let mut txt = HashMap::new();
        txt.insert("version".to_string(), "1.0.0".to_string());
        txt.insert("model".to_string(), "claude-3".to_string());
        txt.insert("project".to_string(), "my-project".to_string());
        txt.insert("cwd".to_string(), "/home/user".to_string());
        txt.insert("auth".to_string(), "true".to_string());

        let fields = extract_all_txt_fields(&Some(txt));

        assert_eq!(fields.version, Some("1.0.0".to_string()));
        assert_eq!(fields.model, Some("claude-3".to_string()));
        assert_eq!(fields.project, Some("my-project".to_string()));
        assert_eq!(fields.cwd, Some("/home/user".to_string()));
        assert!(fields.auth_required);
    }

    #[test]
    fn test_extract_all_txt_fields_none() {
        let fields = extract_all_txt_fields(&None);
        assert_eq!(fields, TxtFields::default());
    }

    #[test]
    fn test_extract_all_txt_fields_empty() {
        let txt: HashMap<String, String> = HashMap::new();
        let fields = extract_all_txt_fields(&Some(txt));
        assert!(fields.version.is_none());
        assert!(fields.model.is_none());
        assert!(!fields.auth_required);
    }

    #[test]
    fn test_extract_all_txt_fields_partial() {
        let mut txt = HashMap::new();
        txt.insert("version".to_string(), "2.0".to_string());
        txt.insert("auth".to_string(), "false".to_string());

        let fields = extract_all_txt_fields(&Some(txt));

        assert_eq!(fields.version, Some("2.0".to_string()));
        assert!(fields.model.is_none());
        assert!(fields.project.is_none());
        assert!(!fields.auth_required);
    }
}
