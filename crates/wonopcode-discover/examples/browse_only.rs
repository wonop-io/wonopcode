//! Browse for services (for testing with advertise_only)

use std::time::Duration;
use wonopcode_discover::Browser;

fn main() {
    println!("=== Browse Only ===\n");
    
    // Create browser
    println!("Creating browser...");
    let browser = match Browser::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to create browser: {}", e);
            return;
        }
    };
    
    println!("Browsing for 10 seconds...");
    match browser.browse(Duration::from_secs(10)) {
        Ok(servers) => {
            println!("\n=== Results ===");
            if servers.is_empty() {
                println!("No servers found!");
            } else {
                for server in &servers {
                    println!("Found: {}", server);
                    println!("  Address: {}", server.address);
                    if let Some(ref hostname) = server.hostname {
                        println!("  Hostname: {}", hostname);
                    }
                    println!("  Version: {:?}", server.version);
                    println!("  Model: {:?}", server.model);
                    println!("  CWD: {:?}", server.cwd);
                    println!("  Auth required: {}", server.auth_required);
                }
            }
            println!("\nTotal servers found: {}", servers.len());
        }
        Err(e) => {
            eprintln!("Browse failed: {}", e);
        }
    }
    
    println!("Done!");
}
