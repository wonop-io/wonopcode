//! Ticket management tools for interacting with issue trackers.
//!
//! This module provides tools that allow agents to list, search, read, and create
//! tickets in configured issue trackers (GitHub, Linear, etc.).

mod add_labels;
mod create;
mod list;
mod list_labels;
mod read;
mod remove_labels;
mod search;
mod service;
mod trackers;

pub use add_labels::TicketAddLabelsTool;
pub use create::TicketCreateTool;
pub use list::TicketListTool;
pub use list_labels::TicketListLabelsTool;
pub use read::TicketReadTool;
pub use remove_labels::TicketRemoveLabelsTool;
pub use search::TicketSearchTool;
pub use service::{
    CreatedTicket, LabelInfo, NewTicket, TicketAttachment, TicketComment, TicketDetails,
    TicketError, TicketFilter, TicketService, TicketStatus, TicketSummary, TicketUser, TrackerInfo,
};
pub use trackers::TicketListTrackersTool;

#[cfg(test)]
pub(crate) use service::mock;
