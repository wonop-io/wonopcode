//! Service advertisement via mDNS using native Bonjour/Avahi.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};
use zeroconf::prelude::*;
use zeroconf::{MdnsService, ServiceRegistration, ServiceType, TxtRecord};

use crate::error::DiscoverError;
use crate::service::AdvertiseConfig;

/// Advertises a wonopcode server on the local network via mDNS.
pub struct Advertiser {
    service: Option<MdnsService>,
    event_loop: Option<zeroconf::EventLoop>,
    fullname: Option<String>,
}

impl Advertiser {
    /// Create a new advertiser.
    pub fn new() -> Result<Self, DiscoverError> {
        Ok(Self {
            service: None,
            event_loop: None,
            fullname: None,
        })
    }

    /// Start advertising a server.
    ///
    /// # Arguments
    /// * `config` - Configuration for the advertisement
    ///
    /// # Returns
    /// The full service name that was registered.
    pub fn advertise(&mut self, config: AdvertiseConfig) -> Result<String, DiscoverError> {
        let service_type = ServiceType::new("wonopcode", "tcp")
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        let mut service = MdnsService::new(service_type, config.port);

        // Set the service name
        service.set_name(&config.name);

        // Set TXT record with properties
        let mut txt_record = TxtRecord::new();
        txt_record
            .insert("version", &config.version)
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;
        txt_record
            .insert("auth", &config.auth_required.to_string())
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        if let Some(ref model) = config.model {
            txt_record
                .insert("model", model)
                .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;
        }
        if let Some(ref project) = config.project {
            txt_record
                .insert("project", project)
                .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;
        }
        if let Some(ref cwd) = config.cwd {
            txt_record
                .insert("cwd", cwd)
                .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;
        }

        service.set_txt_record(txt_record);

        // Set callback for when registration completes
        service.set_registered_callback(Box::new(on_service_registered));

        debug!(
            name = %config.name,
            port = config.port,
            "Registering mDNS service"
        );

        // Register the service
        let event_loop = service
            .register()
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        // Poll once to start registration
        event_loop
            .poll(Duration::from_millis(100))
            .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;

        let fullname = format!("{}._wonopcode._tcp.local.", config.name);
        self.fullname = Some(fullname.clone());
        self.service = Some(service);
        self.event_loop = Some(event_loop);

        info!(
            name = %config.name,
            port = config.port,
            fullname = %fullname,
            "Advertising server via mDNS"
        );

        Ok(fullname)
    }

    /// Poll the event loop to keep the service alive.
    /// Call this periodically (e.g., in a background task).
    pub fn poll(&self) -> Result<(), DiscoverError> {
        if let Some(ref event_loop) = self.event_loop {
            event_loop
                .poll(Duration::from_millis(0))
                .map_err(|e| DiscoverError::ServiceInfo(e.to_string()))?;
        }
        Ok(())
    }

    /// Stop advertising the service.
    pub fn stop(&mut self) -> Result<(), DiscoverError> {
        if self.service.is_some() {
            debug!(fullname = ?self.fullname, "Stopping mDNS service");
            self.service = None;
            self.event_loop = None;
            self.fullname = None;
            info!("Stopped mDNS advertisement");
        }
        Ok(())
    }

    /// Check if currently advertising.
    pub fn is_advertising(&self) -> bool {
        self.service.is_some()
    }

    /// Get the full service name if advertising.
    pub fn service_fullname(&self) -> Option<&str> {
        self.fullname.as_deref()
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            error!(error = %e, "Failed to stop mDNS advertisement on drop");
        }
    }
}

fn on_service_registered(
    result: zeroconf::Result<ServiceRegistration>,
    _context: Option<Arc<dyn Any + Send + Sync>>,
) {
    match result {
        Ok(registration) => {
            info!(
                name = %registration.name(),
                service_type = ?registration.service_type(),
                domain = %registration.domain(),
                "Service registered successfully"
            );
        }
        Err(e) => {
            error!(error = %e, "Failed to register service");
        }
    }
}

// ============================================================================
// Helper functions for building TXT records (public for testing)
// ============================================================================

#[allow(dead_code)]
/// Build TXT record entries from an AdvertiseConfig.
/// Returns a HashMap of key-value pairs that would go into the TXT record.
pub fn build_txt_entries(config: &AdvertiseConfig) -> HashMap<String, String> {
    let mut entries = HashMap::new();

    entries.insert("version".to_string(), config.version.clone());
    entries.insert("auth".to_string(), config.auth_required.to_string());

    if let Some(ref model) = config.model {
        entries.insert("model".to_string(), model.clone());
    }
    if let Some(ref project) = config.project {
        entries.insert("project".to_string(), project.clone());
    }
    if let Some(ref cwd) = config.cwd {
        entries.insert("cwd".to_string(), cwd.clone());
    }

    entries
}

#[allow(dead_code)]
/// Build the full service name from a service name.
pub fn build_fullname(name: &str) -> String {
    format!("{name}._wonopcode._tcp.local.")
}

#[allow(dead_code)]
/// Validate a service name.
/// Returns true if the name is valid for mDNS registration.
pub fn validate_service_name(name: &str) -> bool {
    // Name should not be empty
    if name.is_empty() {
        return false;
    }
    // Name should not be too long (DNS labels have a 63-byte limit)
    if name.len() > 63 {
        return false;
    }
    // Name should not contain certain special characters
    if name.contains('.') || name.contains('/') {
        return false;
    }
    true
}

#[allow(dead_code)]
/// Validate a port number.
/// Returns true if the port is valid.
pub fn validate_port(port: u16) -> bool {
    // Port 0 is typically not valid for services
    // Ports below 1024 are privileged on most systems
    port > 0
}

#[allow(dead_code)]
/// Format the auth_required field as a string for TXT record.
pub fn format_auth_required(auth_required: bool) -> String {
    auth_required.to_string()
}

#[allow(dead_code)]
/// Count the number of entries that would be in a TXT record.
pub fn count_txt_entries(config: &AdvertiseConfig) -> usize {
    let mut count = 2; // version and auth are always present
    if config.model.is_some() {
        count += 1;
    }
    if config.project.is_some() {
        count += 1;
    }
    if config.cwd.is_some() {
        count += 1;
    }
    count
}

#[allow(dead_code)]
/// Validate an AdvertiseConfig for common issues.
pub fn validate_config(config: &AdvertiseConfig) -> Result<(), String> {
    if !validate_service_name(&config.name) {
        return Err(format!("Invalid service name: {}", config.name));
    }
    if !validate_port(config.port) {
        return Err(format!("Invalid port: {}", config.port));
    }
    if config.version.is_empty() {
        return Err("Version cannot be empty".to_string());
    }
    Ok(())
}

#[allow(dead_code)]
/// Create a minimal AdvertiseConfig for testing.
pub fn create_minimal_config(name: &str, port: u16) -> AdvertiseConfig {
    AdvertiseConfig::new(name, port, "0.0.0")
}

#[allow(dead_code)]
/// Parse auth string from TXT record format.
pub fn parse_auth_string(s: &str) -> bool {
    s == "true"
}

#[allow(dead_code)]
/// Build a complete TXT entry for the version field.
pub fn build_version_entry(version: &str) -> (String, String) {
    ("version".to_string(), version.to_string())
}

#[allow(dead_code)]
/// Build a complete TXT entry for the auth field.
pub fn build_auth_entry(auth_required: bool) -> (String, String) {
    ("auth".to_string(), auth_required.to_string())
}

#[allow(dead_code)]
/// Build optional TXT entry for model field.
pub fn build_model_entry(model: &Option<String>) -> Option<(String, String)> {
    model.as_ref().map(|m| ("model".to_string(), m.clone()))
}

#[allow(dead_code)]
/// Build optional TXT entry for project field.
pub fn build_project_entry(project: &Option<String>) -> Option<(String, String)> {
    project.as_ref().map(|p| ("project".to_string(), p.clone()))
}

#[allow(dead_code)]
/// Build optional TXT entry for cwd field.
pub fn build_cwd_entry(cwd: &Option<String>) -> Option<(String, String)> {
    cwd.as_ref().map(|c| ("cwd".to_string(), c.clone()))
}

#[allow(dead_code)]
/// Check if a config has optional model field.
pub fn has_model(config: &AdvertiseConfig) -> bool {
    config.model.is_some()
}

#[allow(dead_code)]
/// Check if a config has optional project field.
pub fn has_project(config: &AdvertiseConfig) -> bool {
    config.project.is_some()
}

#[allow(dead_code)]
/// Check if a config has optional cwd field.
pub fn has_cwd(config: &AdvertiseConfig) -> bool {
    config.cwd.is_some()
}

#[allow(dead_code)]
/// Get all optional fields that are set in a config.
pub fn get_optional_fields(config: &AdvertiseConfig) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if config.model.is_some() {
        fields.push("model");
    }
    if config.project.is_some() {
        fields.push("project");
    }
    if config.cwd.is_some() {
        fields.push("cwd");
    }
    fields
}

#[allow(dead_code)]
/// Describe a config for logging/debugging.
pub fn describe_config(config: &AdvertiseConfig) -> String {
    let optional_count = count_txt_entries(config) - 2;
    format!(
        "AdvertiseConfig {{ name: '{}', port: {}, version: '{}', auth: {}, optional_fields: {} }}",
        config.name, config.port, config.version, config.auth_required, optional_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advertiser_creation() {
        let result = Advertiser::new();
        assert!(result.is_ok());
    }

    #[test]
    fn test_advertiser_initial_state() {
        let advertiser = Advertiser::new().unwrap();
        assert!(!advertiser.is_advertising());
        assert!(advertiser.service_fullname().is_none());
    }

    #[test]
    fn test_advertiser_stop_when_not_advertising() {
        let mut advertiser = Advertiser::new().unwrap();
        // Should not error when stopping while not advertising
        let result = advertiser.stop();
        assert!(result.is_ok());
        assert!(!advertiser.is_advertising());
    }

    #[test]
    fn test_advertiser_poll_when_not_advertising() {
        let advertiser = Advertiser::new().unwrap();
        // Should not error when polling while not advertising
        let result = advertiser.poll();
        assert!(result.is_ok());
    }

    #[test]
    fn test_advertiser_drop_when_not_advertising() {
        // Just verify drop doesn't panic when not advertising
        let advertiser = Advertiser::new().unwrap();
        drop(advertiser);
    }

    #[test]
    fn test_advertise_config_for_advertiser() {
        // Test that AdvertiseConfig can be created for use with Advertiser
        let config = AdvertiseConfig::new("TestService", 8080, "1.0.0")
            .with_model("claude")
            .with_project("test-project")
            .with_cwd("/home/user")
            .with_auth(true);

        assert_eq!(config.name, "TestService");
        assert_eq!(config.port, 8080);
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.model, Some("claude".to_string()));
        assert_eq!(config.project, Some("test-project".to_string()));
        assert_eq!(config.cwd, Some("/home/user".to_string()));
        assert!(config.auth_required);
    }

    // ========================================================================
    // Tests for helper functions
    // ========================================================================

    #[test]
    fn test_build_txt_entries_minimal() {
        let config = AdvertiseConfig::new("TestServer", 8080, "1.0.0");
        let entries = build_txt_entries(&config);

        assert_eq!(entries.get("version"), Some(&"1.0.0".to_string()));
        assert_eq!(entries.get("auth"), Some(&"false".to_string()));
        assert!(!entries.contains_key("model"));
        assert!(!entries.contains_key("project"));
        assert!(!entries.contains_key("cwd"));
    }

    #[test]
    fn test_build_txt_entries_full() {
        let config = AdvertiseConfig::new("TestServer", 8080, "2.0.0")
            .with_model("claude-3")
            .with_project("my-project")
            .with_cwd("/home/user/code")
            .with_auth(true);
        let entries = build_txt_entries(&config);

        assert_eq!(entries.get("version"), Some(&"2.0.0".to_string()));
        assert_eq!(entries.get("auth"), Some(&"true".to_string()));
        assert_eq!(entries.get("model"), Some(&"claude-3".to_string()));
        assert_eq!(entries.get("project"), Some(&"my-project".to_string()));
        assert_eq!(entries.get("cwd"), Some(&"/home/user/code".to_string()));
    }

    #[test]
    fn test_build_txt_entries_partial() {
        let config = AdvertiseConfig::new("Server", 3000, "1.5.0").with_model("gpt-4");
        let entries = build_txt_entries(&config);

        assert_eq!(entries.get("version"), Some(&"1.5.0".to_string()));
        assert_eq!(entries.get("model"), Some(&"gpt-4".to_string()));
        assert!(!entries.contains_key("project"));
        assert!(!entries.contains_key("cwd"));
    }

    #[test]
    fn test_build_txt_entries_auth_true() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0").with_auth(true);
        let entries = build_txt_entries(&config);
        assert_eq!(entries.get("auth"), Some(&"true".to_string()));
    }

    #[test]
    fn test_build_txt_entries_auth_false() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0").with_auth(false);
        let entries = build_txt_entries(&config);
        assert_eq!(entries.get("auth"), Some(&"false".to_string()));
    }

    #[test]
    fn test_build_fullname_simple() {
        let fullname = build_fullname("MyServer");
        assert_eq!(fullname, "MyServer._wonopcode._tcp.local.");
    }

    #[test]
    fn test_build_fullname_with_spaces() {
        let fullname = build_fullname("My Server");
        assert_eq!(fullname, "My Server._wonopcode._tcp.local.");
    }

    #[test]
    fn test_build_fullname_with_numbers() {
        let fullname = build_fullname("server123");
        assert_eq!(fullname, "server123._wonopcode._tcp.local.");
    }

    #[test]
    fn test_build_fullname_with_dashes() {
        let fullname = build_fullname("my-server-name");
        assert_eq!(fullname, "my-server-name._wonopcode._tcp.local.");
    }

    #[test]
    fn test_validate_service_name_valid() {
        assert!(validate_service_name("MyServer"));
        assert!(validate_service_name("server123"));
        assert!(validate_service_name("my-server"));
        assert!(validate_service_name("a"));
    }

    #[test]
    fn test_validate_service_name_empty() {
        assert!(!validate_service_name(""));
    }

    #[test]
    fn test_validate_service_name_too_long() {
        let long_name = "a".repeat(64);
        assert!(!validate_service_name(&long_name));
    }

    #[test]
    fn test_validate_service_name_max_length() {
        let max_name = "a".repeat(63);
        assert!(validate_service_name(&max_name));
    }

    #[test]
    fn test_validate_service_name_with_dot() {
        assert!(!validate_service_name("my.server"));
    }

    #[test]
    fn test_validate_service_name_with_slash() {
        assert!(!validate_service_name("my/server"));
    }

    #[test]
    fn test_validate_port_valid() {
        assert!(validate_port(80));
        assert!(validate_port(8080));
        assert!(validate_port(443));
        assert!(validate_port(65535));
        assert!(validate_port(1));
    }

    #[test]
    fn test_validate_port_zero() {
        assert!(!validate_port(0));
    }

    #[test]
    fn test_format_auth_required_true() {
        assert_eq!(format_auth_required(true), "true");
    }

    #[test]
    fn test_format_auth_required_false() {
        assert_eq!(format_auth_required(false), "false");
    }

    #[test]
    fn test_build_txt_entries_count() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0");
        let entries = build_txt_entries(&config);
        // Should have version and auth
        assert_eq!(entries.len(), 2);

        let config_full = AdvertiseConfig::new("Server", 8080, "1.0.0")
            .with_model("model")
            .with_project("proj")
            .with_cwd("/cwd");
        let entries_full = build_txt_entries(&config_full);
        // Should have version, auth, model, project, cwd
        assert_eq!(entries_full.len(), 5);
    }

    #[test]
    fn test_advertiser_multiple_stop_calls() {
        let mut advertiser = Advertiser::new().unwrap();
        // Multiple stop calls should not error
        assert!(advertiser.stop().is_ok());
        assert!(advertiser.stop().is_ok());
        assert!(advertiser.stop().is_ok());
    }

    #[test]
    fn test_advertiser_poll_multiple_calls() {
        let advertiser = Advertiser::new().unwrap();
        // Multiple poll calls should not error
        assert!(advertiser.poll().is_ok());
        assert!(advertiser.poll().is_ok());
        assert!(advertiser.poll().is_ok());
    }

    #[test]
    fn test_advertiser_is_not_advertising_after_creation() {
        let advertiser = Advertiser::new().unwrap();
        assert!(!advertiser.is_advertising());
    }

    #[test]
    fn test_advertiser_fullname_is_none_after_creation() {
        let advertiser = Advertiser::new().unwrap();
        assert!(advertiser.service_fullname().is_none());
    }

    #[test]
    fn test_build_fullname_empty() {
        let fullname = build_fullname("");
        assert_eq!(fullname, "._wonopcode._tcp.local.");
    }

    #[test]
    fn test_build_txt_entries_with_empty_strings() {
        let config = AdvertiseConfig {
            name: "Server".to_string(),
            port: 8080,
            version: "".to_string(),
            model: Some("".to_string()),
            project: Some("".to_string()),
            cwd: Some("".to_string()),
            auth_required: false,
        };
        let entries = build_txt_entries(&config);
        assert_eq!(entries.get("version"), Some(&"".to_string()));
        assert_eq!(entries.get("model"), Some(&"".to_string()));
    }

    // ========================================================================
    // Tests for count_txt_entries
    // ========================================================================

    #[test]
    fn test_count_txt_entries_minimal() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0");
        assert_eq!(count_txt_entries(&config), 2);
    }

    #[test]
    fn test_count_txt_entries_with_model() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0").with_model("claude");
        assert_eq!(count_txt_entries(&config), 3);
    }

    #[test]
    fn test_count_txt_entries_with_all() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0")
            .with_model("claude")
            .with_project("proj")
            .with_cwd("/home");
        assert_eq!(count_txt_entries(&config), 5);
    }

    #[test]
    fn test_count_txt_entries_with_project_only() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0").with_project("proj");
        assert_eq!(count_txt_entries(&config), 3);
    }

    #[test]
    fn test_count_txt_entries_with_cwd_only() {
        let config = AdvertiseConfig::new("Server", 8080, "1.0.0").with_cwd("/home");
        assert_eq!(count_txt_entries(&config), 3);
    }

    // ========================================================================
    // Tests for validate_config
    // ========================================================================

    #[test]
    fn test_validate_config_valid() {
        let config = AdvertiseConfig::new("ValidName", 8080, "1.0.0");
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_empty_name() {
        let config = AdvertiseConfig::new("", 8080, "1.0.0");
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid service name"));
    }

    #[test]
    fn test_validate_config_invalid_port() {
        let config = AdvertiseConfig::new("Valid", 0, "1.0.0");
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid port"));
    }

    #[test]
    fn test_validate_config_empty_version() {
        let config = AdvertiseConfig::new("Valid", 8080, "");
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Version cannot be empty"));
    }

    #[test]
    fn test_validate_config_name_with_dot() {
        let config = AdvertiseConfig::new("my.server", 8080, "1.0.0");
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_config_name_with_slash() {
        let config = AdvertiseConfig::new("my/server", 8080, "1.0.0");
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    // ========================================================================
    // Tests for create_minimal_config
    // ========================================================================

    #[test]
    fn test_create_minimal_config() {
        let config = create_minimal_config("Test", 3000);
        assert_eq!(config.name, "Test");
        assert_eq!(config.port, 3000);
        assert_eq!(config.version, "0.0.0");
        assert!(config.model.is_none());
        assert!(config.project.is_none());
        assert!(config.cwd.is_none());
        assert!(!config.auth_required);
    }

    #[test]
    fn test_create_minimal_config_valid() {
        let config = create_minimal_config("TestServer", 8080);
        // 0.0.0 is still a valid version string
        assert!(validate_config(&config).is_ok());
    }

    // ========================================================================
    // Tests for parse_auth_string
    // ========================================================================

    #[test]
    fn test_parse_auth_string_true() {
        assert!(parse_auth_string("true"));
    }

    #[test]
    fn test_parse_auth_string_false() {
        assert!(!parse_auth_string("false"));
    }

    #[test]
    fn test_parse_auth_string_yes() {
        assert!(!parse_auth_string("yes"));
    }

    #[test]
    fn test_parse_auth_string_one() {
        assert!(!parse_auth_string("1"));
    }

    #[test]
    fn test_parse_auth_string_empty() {
        assert!(!parse_auth_string(""));
    }

    #[test]
    fn test_parse_auth_string_case_sensitive() {
        assert!(!parse_auth_string("True"));
        assert!(!parse_auth_string("TRUE"));
    }

    // ========================================================================
    // Additional validation tests
    // ========================================================================

    #[test]
    fn test_validate_service_name_with_unicode() {
        // Unicode characters should be allowed
        assert!(validate_service_name("café"));
        assert!(validate_service_name("日本語"));
    }

    #[test]
    fn test_validate_service_name_with_hyphen() {
        assert!(validate_service_name("my-server"));
        assert!(validate_service_name("server-123-test"));
    }

    #[test]
    fn test_validate_service_name_with_underscore() {
        assert!(validate_service_name("my_server"));
    }

    #[test]
    fn test_validate_port_max() {
        assert!(validate_port(65535));
    }

    #[test]
    fn test_validate_port_min_valid() {
        assert!(validate_port(1));
    }

    #[test]
    fn test_validate_port_privileged() {
        // Privileged ports are still valid
        assert!(validate_port(80));
        assert!(validate_port(443));
        assert!(validate_port(22));
    }

    // ========================================================================
    // Additional build_fullname tests
    // ========================================================================

    #[test]
    fn test_build_fullname_with_unicode() {
        let fullname = build_fullname("日本語");
        assert_eq!(fullname, "日本語._wonopcode._tcp.local.");
    }

    #[test]
    fn test_build_fullname_with_special_chars() {
        let fullname = build_fullname("test-server_123");
        assert_eq!(fullname, "test-server_123._wonopcode._tcp.local.");
    }

    // ========================================================================
    // Advertiser state tests
    // ========================================================================

    #[test]
    fn test_advertiser_state_after_stop() {
        let mut advertiser = Advertiser::new().unwrap();
        advertiser.stop().unwrap();

        assert!(!advertiser.is_advertising());
        assert!(advertiser.service_fullname().is_none());
    }

    #[test]
    fn test_advertiser_poll_after_stop() {
        let mut advertiser = Advertiser::new().unwrap();
        advertiser.stop().unwrap();

        // Polling after stop should still work
        assert!(advertiser.poll().is_ok());
    }

    // ========================================================================
    // Tests for build_version_entry
    // ========================================================================

    #[test]
    fn test_build_version_entry() {
        let (key, value) = build_version_entry("1.0.0");
        assert_eq!(key, "version");
        assert_eq!(value, "1.0.0");
    }

    #[test]
    fn test_build_version_entry_empty() {
        let (key, value) = build_version_entry("");
        assert_eq!(key, "version");
        assert_eq!(value, "");
    }

    // ========================================================================
    // Tests for build_auth_entry
    // ========================================================================

    #[test]
    fn test_build_auth_entry_true() {
        let (key, value) = build_auth_entry(true);
        assert_eq!(key, "auth");
        assert_eq!(value, "true");
    }

    #[test]
    fn test_build_auth_entry_false() {
        let (key, value) = build_auth_entry(false);
        assert_eq!(key, "auth");
        assert_eq!(value, "false");
    }

    // ========================================================================
    // Tests for build_model_entry
    // ========================================================================

    #[test]
    fn test_build_model_entry_some() {
        let result = build_model_entry(&Some("claude-3".to_string()));
        assert!(result.is_some());
        let (key, value) = result.unwrap();
        assert_eq!(key, "model");
        assert_eq!(value, "claude-3");
    }

    #[test]
    fn test_build_model_entry_none() {
        let result = build_model_entry(&None);
        assert!(result.is_none());
    }

    // ========================================================================
    // Tests for build_project_entry
    // ========================================================================

    #[test]
    fn test_build_project_entry_some() {
        let result = build_project_entry(&Some("my-project".to_string()));
        assert!(result.is_some());
        let (key, value) = result.unwrap();
        assert_eq!(key, "project");
        assert_eq!(value, "my-project");
    }

    #[test]
    fn test_build_project_entry_none() {
        let result = build_project_entry(&None);
        assert!(result.is_none());
    }

    // ========================================================================
    // Tests for build_cwd_entry
    // ========================================================================

    #[test]
    fn test_build_cwd_entry_some() {
        let result = build_cwd_entry(&Some("/home/user/project".to_string()));
        assert!(result.is_some());
        let (key, value) = result.unwrap();
        assert_eq!(key, "cwd");
        assert_eq!(value, "/home/user/project");
    }

    #[test]
    fn test_build_cwd_entry_none() {
        let result = build_cwd_entry(&None);
        assert!(result.is_none());
    }

    // ========================================================================
    // Tests for has_model, has_project, has_cwd
    // ========================================================================

    #[test]
    fn test_has_model_true() {
        let config = AdvertiseConfig::new("test", 8080, "1.0").with_model("claude-3");
        assert!(has_model(&config));
    }

    #[test]
    fn test_has_model_false() {
        let config = AdvertiseConfig::new("test", 8080, "1.0");
        assert!(!has_model(&config));
    }

    #[test]
    fn test_has_project_true() {
        let config = AdvertiseConfig::new("test", 8080, "1.0").with_project("my-project");
        assert!(has_project(&config));
    }

    #[test]
    fn test_has_project_false() {
        let config = AdvertiseConfig::new("test", 8080, "1.0");
        assert!(!has_project(&config));
    }

    #[test]
    fn test_has_cwd_true() {
        let config = AdvertiseConfig::new("test", 8080, "1.0").with_cwd("/home/user");
        assert!(has_cwd(&config));
    }

    #[test]
    fn test_has_cwd_false() {
        let config = AdvertiseConfig::new("test", 8080, "1.0");
        assert!(!has_cwd(&config));
    }

    // ========================================================================
    // Tests for get_optional_fields
    // ========================================================================

    #[test]
    fn test_get_optional_fields_none() {
        let config = AdvertiseConfig::new("test", 8080, "1.0");
        let fields = get_optional_fields(&config);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_get_optional_fields_model_only() {
        let config = AdvertiseConfig::new("test", 8080, "1.0").with_model("claude-3");
        let fields = get_optional_fields(&config);
        assert_eq!(fields, vec!["model"]);
    }

    #[test]
    fn test_get_optional_fields_all() {
        let config = AdvertiseConfig::new("test", 8080, "1.0")
            .with_model("claude-3")
            .with_project("my-project")
            .with_cwd("/home/user");
        let fields = get_optional_fields(&config);
        assert_eq!(fields, vec!["model", "project", "cwd"]);
    }

    #[test]
    fn test_get_optional_fields_project_and_cwd() {
        let config = AdvertiseConfig::new("test", 8080, "1.0")
            .with_project("my-project")
            .with_cwd("/home/user");
        let fields = get_optional_fields(&config);
        assert_eq!(fields, vec!["project", "cwd"]);
    }

    // ========================================================================
    // Tests for describe_config
    // ========================================================================

    #[test]
    fn test_describe_config_minimal() {
        let config = AdvertiseConfig::new("test-server", 8080, "1.0.0");
        let desc = describe_config(&config);
        assert!(desc.contains("test-server"));
        assert!(desc.contains("8080"));
        assert!(desc.contains("1.0.0"));
        assert!(desc.contains("optional_fields: 0"));
    }

    #[test]
    fn test_describe_config_with_optional() {
        let config = AdvertiseConfig::new("test-server", 8080, "1.0.0")
            .with_model("claude-3")
            .with_project("my-project")
            .with_auth(true);
        let desc = describe_config(&config);
        assert!(desc.contains("test-server"));
        assert!(desc.contains("auth: true"));
        assert!(desc.contains("optional_fields: 2"));
    }

    #[test]
    fn test_describe_config_all_fields() {
        let config = AdvertiseConfig::new("server", 3000, "2.0")
            .with_model("gpt-4")
            .with_project("project")
            .with_cwd("/path")
            .with_auth(false);
        let desc = describe_config(&config);
        assert!(desc.contains("server"));
        assert!(desc.contains("3000"));
        assert!(desc.contains("2.0"));
        assert!(desc.contains("auth: false"));
        assert!(desc.contains("optional_fields: 3"));
    }
}
