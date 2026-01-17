//! Service advertisement via mDNS using native Bonjour/Avahi.

use std::any::Any;
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
}
