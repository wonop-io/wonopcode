//! Language Server Protocol (LSP) client for wonopcode.
//!
//! This crate provides LSP integration for code intelligence features:
//! - Lazy activation on file access
//! - Broken server tracking (avoids repeated spawn failures)
//! - Spawning deduplication (concurrent requests share spawn)
//! - Diagnostics collection with optional waiting
//! - All major LSP operations
//!
//! # Features
//!
//! - Go to definition
//! - Find references
//! - Go to implementation
//! - Workspace symbol search
//! - Document symbols
//! - Hover information
//! - Call hierarchy (incoming/outgoing calls)
//! - Diagnostics collection
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌────────────┐     ┌──────────────┐
//! │  wonopcode  │────▶│ LSP Client │────▶│ Lang Server  │
//! │    (AI)     │◀────│            │◀────│ (rust-analyzer)
//! └─────────────┘     └────────────┘     └──────────────┘
//! ```
//!
//! # Supported Language Servers
//!
//! | Language   | Server                    | Command                        |
//! |------------|---------------------------|--------------------------------|
//! | Rust       | rust-analyzer             | `rust-analyzer`                |
//! | TypeScript | typescript-language-server| `typescript-language-server --stdio` |
//! | Python     | pyright                   | `pyright-langserver --stdio`   |
//! | Go         | gopls                     | `gopls`                        |
//! | And 30+ more...                                                        |
//!
//! # Example
//!
//! ```no_run
//! use wonopcode_lsp::{LspClient, LspConfig};
//! use std::path::PathBuf;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create client with defaults
//! let client = LspClient::with_defaults();
//!
//! // Touch a file to activate LSP and collect diagnostics
//! client.touch_file(&PathBuf::from("src/main.rs"), true).await?;
//!
//! // Go to definition
//! let locations = client.goto_definition(
//!     &PathBuf::from("src/main.rs"),
//!     10,  // line (0-based)
//!     5,   // column (0-based)
//! ).await?;
//!
//! // Get diagnostics from all servers
//! let diagnostics = client.diagnostics().await;
//! for (path, diags) in diagnostics {
//!     for d in diags {
//!         println!("{}: {}", path, d.pretty());
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod client;
mod config;
mod error;
mod transport;

pub use client::{
    DiagnosticInfo, DiagnosticSeverityLevel, DocumentSymbolInfo, LspClient, LspServerStatus,
    LspStatus,
};
pub use config::LspConfig;
pub use error::{LspError, LspResult};

// Re-export useful lsp-types
pub use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Location, Position,
    Range, SymbolInformation, SymbolKind, Uri,
};
