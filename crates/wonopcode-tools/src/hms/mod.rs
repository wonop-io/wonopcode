//! Hierarchical Memory System (HMS) for directory-scoped agent context.
//!
//! HMS provides a file-based memory system where:
//! - `memory.yaml` files store key-value entries with visibility rules
//! - Memory propagates up/down the directory tree based on visibility
//! - `AGENTS.TEMPLATE.md` files are rendered with Tera to produce `AGENTS.md`
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │   HmsService    │  ← Main entry point
//! └────────┬────────┘
//!          │
//!    ┌─────┴─────┐
//!    ▼           ▼
//! ┌──────────┐ ┌──────────────┐
//! │ Resolver │ │ Renderer     │
//! └──────────┘ └──────────────┘
//!    │              │
//!    ▼              ▼
//! memory.yaml   AGENTS.TEMPLATE.md
//! ```
//!
//! # Storage Classes
//!
//! - `tracked`: `.wonopcode/memory.yaml` - committed to git
//! - `local`: `.wonopcode-local/memory.yaml` - gitignored
//! - `user`: `~/.config/wonopcode/memory.yaml` - global user preferences

mod error;
mod renderer;
mod resolver;
mod service;
mod tools;
mod types;

pub use error::HmsError;
pub use renderer::{AgentsGenerator, TemplateRenderer};
pub use resolver::{CachedResolver, MemoryResolver};
pub use service::HmsService;
pub use tools::{HmsDeleteTool, HmsGetTool, HmsListTool, HmsRenderTool, HmsSetTool};
pub use types::{
    MemoryEntry, MemoryEntryData, MemoryFile, MemoryValue, ResolvedMemory, StorageClass, Visibility,
};

/// Shared HMS service type for use in ToolContext.
/// Uses tokio::RwLock for interior mutability since `generate` and `write_agents_md` require
/// mutable access and need to be held across await points.
pub type SharedHmsService = std::sync::Arc<tokio::sync::RwLock<HmsService>>;
