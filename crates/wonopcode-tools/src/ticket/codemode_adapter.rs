//! Adapter to expose TicketService functionality to the TypeScript execution engine.
//!
//! This module bridges the `wonopcode_tools::TicketService` with the
//! `wonopcode_codemode::TicketService` trait.

use async_trait::async_trait;
use std::sync::Arc;

use wonopcode_codemode::{
    NewTicket as CodemodeNewTicket, ServiceError, ServiceResult, Ticket as CodemodeTicket,
    TicketFilter as CodemodeTicketFilter, TicketService as CodemodeTicketService,
    TicketStatus as CodemodeTicketStatus, TrackerInfo as CodemodeTrackerInfo,
};

use super::service::{NewTicket, TicketDetails, TicketError, TicketFilter, TicketService, TicketStatus, TicketSummary};

/// Adapter that implements the codemode TicketService trait using the tools TicketService.
pub struct TicketServiceAdapter {
    service: Arc<dyn TicketService>,
}

impl TicketServiceAdapter {
    /// Create a new adapter wrapping the given ticket service.
    pub fn new(service: Arc<dyn TicketService>) -> Self {
        Self { service }
    }

    fn convert_status(status: &TicketStatus) -> CodemodeTicketStatus {
        match status {
            TicketStatus::Open => CodemodeTicketStatus::Open,
            TicketStatus::InProgress => CodemodeTicketStatus::InProgress,
            TicketStatus::Review => CodemodeTicketStatus::Review,
            TicketStatus::Closed => CodemodeTicketStatus::Closed,
            TicketStatus::Custom(_) => CodemodeTicketStatus::Open,
        }
    }

    fn convert_codemode_status(status: CodemodeTicketStatus) -> TicketStatus {
        match status {
            CodemodeTicketStatus::Open => TicketStatus::Open,
            CodemodeTicketStatus::InProgress => TicketStatus::InProgress,
            CodemodeTicketStatus::Review => TicketStatus::Review,
            CodemodeTicketStatus::Closed => TicketStatus::Closed,
        }
    }

    fn convert_filter(filter: CodemodeTicketFilter) -> TicketFilter {
        TicketFilter {
            status: filter.status.map(|statuses| {
                statuses.into_iter().map(Self::convert_codemode_status).collect()
            }),
            assignee: filter.assignee,
            labels: filter.labels.unwrap_or_default(),
            tracker_id: filter.tracker_id,
            limit: filter.limit.unwrap_or(100),
        }
    }

    fn summary_to_ticket(t: TicketSummary) -> CodemodeTicket {
        CodemodeTicket {
            id: t.id,
            title: t.title,
            description: None, // Summary doesn't have description
            status: Self::convert_status(&t.status),
            assignee: t.assignee.map(|u| u.username),
            labels: t.labels,
            tracker_id: Some(t.tracker_name), // Use tracker_name as tracker_id
        }
    }

    fn details_to_ticket(t: TicketDetails) -> CodemodeTicket {
        CodemodeTicket {
            id: t.id,
            title: t.title,
            description: t.description,
            status: Self::convert_status(&t.status),
            assignee: t.assignee.map(|u| u.username),
            labels: t.labels,
            tracker_id: Some(t.tracker_name),
        }
    }

    fn convert_error(e: TicketError) -> ServiceError {
        ServiceError::new("TICKET_ERROR", e.to_string())
    }
}

#[async_trait]
impl CodemodeTicketService for TicketServiceAdapter {
    async fn list_trackers(&self) -> ServiceResult<Vec<CodemodeTrackerInfo>> {
        self.service.list_trackers().await
            .map(|trackers| {
                trackers.into_iter().map(|t| CodemodeTrackerInfo {
                    id: t.id,
                    name: t.name,
                    tracker_type: t.tracker_type,
                    enabled: t.enabled,
                }).collect()
            })
            .map_err(Self::convert_error)
    }

    async fn list_tickets(&self, filter: CodemodeTicketFilter) -> ServiceResult<Vec<CodemodeTicket>> {
        let tools_filter = Self::convert_filter(filter);
        self.service.list_tickets(tools_filter).await
            .map(|tickets| tickets.into_iter().map(Self::summary_to_ticket).collect())
            .map_err(Self::convert_error)
    }

    async fn read_ticket(&self, ticket_id: &str) -> ServiceResult<CodemodeTicket> {
        self.service.get_ticket(ticket_id, false, false).await
            .map(Self::details_to_ticket)
            .map_err(Self::convert_error)
    }

    async fn create_ticket(&self, ticket: CodemodeNewTicket) -> ServiceResult<CodemodeTicket> {
        let labels = ticket.labels.clone().unwrap_or_default();
        let tools_ticket = NewTicket {
            title: ticket.title.clone(),
            description: ticket.description.clone(),
            labels,
            tracker_id: ticket.tracker_id.clone(),
            assignee: ticket.assignee.clone(),
            status: None,
        };
        self.service.create_ticket(tools_ticket).await
            .map(|t| CodemodeTicket {
                id: t.id,
                title: t.title,
                description: ticket.description,
                status: CodemodeTicketStatus::Open,
                assignee: ticket.assignee,
                labels: ticket.labels.unwrap_or_default(),
                tracker_id: Some(t.tracker_name),
            })
            .map_err(Self::convert_error)
    }

    async fn search_tickets(&self, query: &str, limit: Option<usize>) -> ServiceResult<Vec<CodemodeTicket>> {
        self.service.search_tickets(query, limit.unwrap_or(20), None).await
            .map(|tickets| tickets.into_iter().map(Self::summary_to_ticket).collect())
            .map_err(Self::convert_error)
    }

    async fn add_labels(&self, ticket_id: &str, labels: Vec<String>) -> ServiceResult<CodemodeTicket> {
        self.service.add_labels(ticket_id, labels).await
            .map(Self::details_to_ticket)
            .map_err(Self::convert_error)
    }

    async fn remove_labels(&self, ticket_id: &str, labels: Vec<String>) -> ServiceResult<CodemodeTicket> {
        self.service.remove_labels(ticket_id, labels).await
            .map(Self::details_to_ticket)
            .map_err(Self::convert_error)
    }
}
