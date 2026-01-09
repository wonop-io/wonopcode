//! Advertise a service (for testing with browse_only)

use std::time::Duration;
use wonopcode_discover::{AdvertiseConfig, Advertiser};

fn main() {
    println!("=== Advertise Only ===\n");
    
    // Create advertiser
    println!("Creating advertiser...");
    let mut advertiser = match Advertiser::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to create advertiser: {}", e);
            return;
        }
    };
    
    // Advertise a test service
    println!("Advertising service...");
    let config = AdvertiseConfig::new("test-server", 3000, "0.1.0")
        .with_model("test-model")
        .with_cwd("/tmp/test")
        .with_auth(true);
    
    match advertiser.advertise(config) {
        Ok(fullname) => println!("Registered: {}", fullname),
        Err(e) => {
            eprintln!("Failed to advertise: {}", e);
            return;
        }
    }
    
    println!("\nService is now advertised. Keeping alive for 30 seconds...");
    println!("Run browse_only in another terminal to find it.\n");
    
    // Poll the event loop to keep the service alive
    for _ in 0..300 {
        if let Err(e) = advertiser.poll() {
            eprintln!("Poll error: {}", e);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    
    println!("Stopping advertiser...");
    drop(advertiser);
    
    println!("Done!");
}
