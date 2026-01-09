//! mDNS service discovery for wonopcode.
//!
//! This crate provides functionality for advertising and discovering wonopcode
//! servers on the local network using mDNS (Multicast DNS) and DNS-SD
//! (DNS-based Service Discovery).
//!
//! # Service Type
//!
//! Wonopcode servers are advertised using the service type `_wonopcode._tcp.local.`
//!
//! # Example: Advertising a Server
//!
//! ```no_run
//! use wonopcode_discover::{Advertiser, AdvertiseConfig};
//!
//! let mut advertiser = Advertiser::new().expect("Failed to create advertiser");
//!
//! let config = AdvertiseConfig::new("my-server", 3000, "0.1.0")
//!     .with_model("claude-opus-4-5-20251101")
//!     .with_project("my-project")
//!     .with_auth(true);
//!
//! advertiser.advertise(config).expect("Failed to advertise");
//!
//! // Server will be advertised until advertiser is dropped
//! ```
//!
//! # Example: Discovering Servers
//!
//! ```no_run
//! use wonopcode_discover::Browser;
//! use std::time::Duration;
//!
//! let browser = Browser::new().expect("Failed to create browser");
//! let servers = browser.browse(Duration::from_secs(3)).expect("Failed to browse");
//!
//! for server in servers {
//!     println!("Found: {} at {}", server.name, server.address);
//! }
//! ```

mod advertise;
mod browse;
mod error;
mod service;

pub use advertise::Advertiser;
pub use browse::Browser;
pub use error::DiscoverError;
pub use service::{AdvertiseConfig, ServerInfo, SERVICE_TYPE};
