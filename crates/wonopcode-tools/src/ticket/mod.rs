//! Ticket management tools for interacting with issue trackers.
//!
//! This module provides tools that allow agents to list, search, read, and create
//! tickets in configured issue trackers (GitHub, Linear, etc.).

mod create;
mod list;
mod read;
mod search;
mod service;
mod trackers;

pub use create::TicketCreateTool;
pub use list::TicketListTool;
pub use read::TicketReadTool;
pub use search::TicketSearchTool;
pub use service::{
    CreatedTicket, NewTicket, TicketAttachment, TicketComment, TicketDetails, TicketError,
    TicketFilter, TicketService, TicketStatus, TicketSummary, TicketUser, TrackerInfo,
};
pub use trackers::TicketListTrackersTool;

#[cfg(test)]
pub(crate) use service::mock;
