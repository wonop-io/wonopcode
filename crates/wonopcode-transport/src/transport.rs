//! Core transport implementation using Apache Iggy.

use std::time::Duration;

use iggy::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use wonopcode_message::{ClientMessage, ServerMessage, WorkstreamId};

use crate::config::TransportConfig;
use crate::error::TransportError;
use crate::{CLIENT_TOPIC, SERVER_TOPIC};

/// Transport client for sending/receiving messages via Iggy.
///
/// This is the main entry point for communication. It handles:
/// - Connection management and authentication
/// - Stream/topic creation
/// - Message publishing and subscription
pub struct Transport {
    /// The Iggy client instance.
    client: IggyClient,
    /// Configuration.
    config: TransportConfig,
    /// Whether we're connected.
    connected: bool,
}

impl Transport {
    /// Create and connect a new transport.
    ///
    /// This will:
    /// 1. Connect to the Iggy server
    /// 2. Authenticate with the provided credentials
    /// 3. Optionally create stream and topics if they don't exist
    pub async fn connect(config: TransportConfig) -> Result<Self, TransportError> {
        info!("Connecting to Iggy at {}", config.server_address);

        // Use connection string for the high-level SDK
        let conn_str = config.connection_string();
        debug!("Using connection string: {}", conn_str);

        let client = IggyClient::from_connection_string(&conn_str)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        // Connect and authenticate (handled by connection string)
        client
            .connect()
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        info!("Connected to Iggy server");

        let transport = Self {
            client,
            config,
            connected: true,
        };

        // Create infrastructure if needed
        if transport.config.auto_create {
            transport.ensure_infrastructure().await?;
        }

        Ok(transport)
    }

    /// Ensure stream and topics exist.
    async fn ensure_infrastructure(&self) -> Result<(), TransportError> {
        let stream_name = &self.config.stream_name;

        // Try to create stream (ignore if exists)
        info!("Ensuring stream '{}' exists", stream_name);
        match self.client.create_stream(stream_name).await {
            Ok(_) => info!("Created stream '{}'", stream_name),
            Err(e) => {
                // Check if it's a "stream exists" error (code 1000)
                let err_str = e.to_string();
                if err_str.contains("already exists") || err_str.contains("1000") {
                    debug!("Stream '{}' already exists", stream_name);
                } else {
                    return Err(TransportError::InfrastructureSetup(e.to_string()));
                }
            }
        }

        let stream_id = Identifier::named(stream_name)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        // Create client-messages topic
        Self::ensure_topic(&self.client, &stream_id, CLIENT_TOPIC).await?;

        // Create server-messages topic
        Self::ensure_topic(&self.client, &stream_id, SERVER_TOPIC).await?;

        info!("Infrastructure ready");
        Ok(())
    }

    /// Ensure a topic exists in the stream.
    async fn ensure_topic(
        client: &IggyClient,
        stream_id: &Identifier,
        topic_name: &str,
    ) -> Result<(), TransportError> {
        info!("Ensuring topic '{}' exists", topic_name);

        match client
            .create_topic(
                stream_id,
                topic_name,
                1, // partitions
                CompressionAlgorithm::default(),
                None, // replication factor
                IggyExpiry::NeverExpire,
                MaxTopicSize::ServerDefault,
            )
            .await
        {
            Ok(_) => {
                info!("Created topic '{}'", topic_name);
                Ok(())
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("already exists") || err_str.contains("1001") {
                    debug!("Topic '{}' already exists", topic_name);
                    Ok(())
                } else {
                    Err(TransportError::InfrastructureSetup(e.to_string()))
                }
            }
        }
    }

    /// Check if the transport is connected.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Get the stream name.
    pub fn stream_name(&self) -> &str {
        &self.config.stream_name
    }

    /// Send a client message to the server.
    pub async fn send_client_message(&self, message: ClientMessage) -> Result<(), TransportError> {
        if !self.connected {
            return Err(TransportError::NotConnected);
        }

        let payload = serde_json::to_vec(&message)?;
        let iggy_message = IggyMessage::from_bytes(payload.into())
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;

        let stream_id = Identifier::named(&self.config.stream_name)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;
        let topic_id = Identifier::named(CLIENT_TOPIC)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        // Use balanced partitioning (single partition for now)
        let partitioning = Partitioning::balanced();

        self.client
            .send_messages(&stream_id, &topic_id, &partitioning, &mut [iggy_message])
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;

        debug!("Sent client message: {}", message.id);
        Ok(())
    }

    /// Send a server message to clients.
    pub async fn send_server_message(&self, message: ServerMessage) -> Result<(), TransportError> {
        if !self.connected {
            return Err(TransportError::NotConnected);
        }

        let payload = serde_json::to_vec(&message)?;
        let iggy_message = IggyMessage::from_bytes(payload.into())
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;

        let stream_id = Identifier::named(&self.config.stream_name)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;
        let topic_id = Identifier::named(SERVER_TOPIC)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        // Partition by workstream_id for ordering within a workstream
        let partitioning = Partitioning::messages_key(message.workstream_id.0.as_bytes())
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        self.client
            .send_messages(&stream_id, &topic_id, &partitioning, &mut [iggy_message])
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;

        debug!("Sent server message: {}", message.id);
        Ok(())
    }

    /// Subscribe to client messages (for server).
    ///
    /// Returns a channel that receives client messages.
    /// The consumer group ensures messages are distributed across server instances.
    pub async fn subscribe_client_messages(
        &self,
        consumer_group: &str,
    ) -> Result<mpsc::UnboundedReceiver<ClientMessage>, TransportError> {
        if !self.connected {
            return Err(TransportError::NotConnected);
        }

        let (tx, rx) = mpsc::unbounded_channel();

        let stream_name = self.config.stream_name.clone();
        let conn_str = self.config.connection_string();

        // Create a separate client for the consumer (Iggy recommendation)
        let consumer_client = IggyClient::from_connection_string(&conn_str)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        consumer_client
            .connect()
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        let consumer_group = consumer_group.to_string();

        // Spawn consumer task
        tokio::spawn(async move {
            if let Err(e) =
                Self::consume_client_messages(consumer_client, stream_name, consumer_group, tx)
                    .await
            {
                error!("Client message consumer error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Internal consumer loop for client messages.
    async fn consume_client_messages(
        client: IggyClient,
        stream_name: String,
        consumer_group: String,
        tx: mpsc::UnboundedSender<ClientMessage>,
    ) -> Result<(), TransportError> {
        // Create/join consumer group
        let stream_id = Identifier::named(&stream_name)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;
        let topic_id = Identifier::named(CLIENT_TOPIC)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        let consumer_group_id = Identifier::named(&consumer_group)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        // Try to create consumer group (ignore if exists)
        let _ = client
            .create_consumer_group(&stream_id, &topic_id, &consumer_group)
            .await;

        // Join the consumer group
        client
            .join_consumer_group(&stream_id, &topic_id, &consumer_group_id)
            .await
            .map_err(|e| TransportError::ReceiveFailed(e.to_string()))?;

        info!(
            "Joined consumer group '{}' for client messages",
            consumer_group
        );

        let consumer = Consumer::group(
            Identifier::named(&consumer_group)
                .map_err(|e| TransportError::InvalidConfig(e.to_string()))?,
        );

        loop {
            let polled = client
                .poll_messages(
                    &stream_id,
                    &topic_id,
                    None, // partition_id (None = auto-assigned by consumer group)
                    &consumer,
                    &PollingStrategy::next(),
                    100,  // batch size
                    true, // auto-commit
                )
                .await;

            match polled {
                Ok(messages) => {
                    for msg in messages.messages {
                        match serde_json::from_slice::<ClientMessage>(&msg.payload) {
                            Ok(client_msg) => {
                                if tx.send(client_msg).is_err() {
                                    info!("Client message channel closed, stopping consumer");
                                    return Ok(());
                                }
                            }
                            Err(e) => {
                                warn!("Failed to deserialize client message: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    // Ignore "no messages" type errors
                    if !err_str.contains("no messages") {
                        warn!("Poll error: {}", e);
                    }
                }
            }

            // Small delay to prevent busy-looping
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Subscribe to server messages (for client).
    ///
    /// Returns a channel that receives server messages.
    /// Optionally filter by workstream_id.
    /// If from_offset is provided, replay messages from that offset.
    pub async fn subscribe_server_messages(
        &self,
        consumer_id: &str,
        workstream_filter: Option<WorkstreamId>,
        from_offset: Option<u64>,
    ) -> Result<mpsc::UnboundedReceiver<ServerMessage>, TransportError> {
        if !self.connected {
            return Err(TransportError::NotConnected);
        }

        let (tx, rx) = mpsc::unbounded_channel();

        let stream_name = self.config.stream_name.clone();
        let conn_str = self.config.connection_string();
        let consumer_id = consumer_id.to_string();

        // Create a separate client for the consumer
        let consumer_client = IggyClient::from_connection_string(&conn_str)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        consumer_client
            .connect()
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        // Spawn consumer task
        tokio::spawn(async move {
            if let Err(e) = Self::consume_server_messages(
                consumer_client,
                stream_name,
                consumer_id,
                workstream_filter,
                from_offset,
                tx,
            )
            .await
            {
                error!("Server message consumer error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Internal consumer loop for server messages.
    async fn consume_server_messages(
        client: IggyClient,
        stream_name: String,
        consumer_id: String,
        workstream_filter: Option<WorkstreamId>,
        from_offset: Option<u64>,
        tx: mpsc::UnboundedSender<ServerMessage>,
    ) -> Result<(), TransportError> {
        let stream_id = Identifier::named(&stream_name)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;
        let topic_id = Identifier::named(SERVER_TOPIC)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        let consumer = Consumer::new(
            Identifier::named(&consumer_id)
                .map_err(|e| TransportError::InvalidConfig(e.to_string()))?,
        );

        // Start from specified offset or from next available message
        let mut current_offset = from_offset.unwrap_or(0);
        let polling_strategy = if from_offset.is_some() {
            PollingStrategy::offset(current_offset)
        } else {
            PollingStrategy::next()
        };

        info!(
            "Starting server message consumer '{}' from offset {:?}",
            consumer_id, from_offset
        );

        let mut strategy = polling_strategy;

        loop {
            let polled = client
                .poll_messages(
                    &stream_id,
                    &topic_id,
                    Some(0), // partition_id 0
                    &consumer,
                    &strategy,
                    100,   // batch size
                    false, // no auto-commit (clients manage their own offsets)
                )
                .await;

            match polled {
                Ok(messages) => {
                    for msg in messages.messages {
                        match serde_json::from_slice::<ServerMessage>(&msg.payload) {
                            Ok(mut server_msg) => {
                                // Update offset in the message
                                server_msg.offset = msg.header.offset;
                                current_offset = msg.header.offset + 1;

                                // Apply workstream filter
                                if let Some(ref filter) = workstream_filter {
                                    if &server_msg.workstream_id != filter {
                                        continue;
                                    }
                                }

                                if tx.send(server_msg).is_err() {
                                    info!("Server message channel closed, stopping consumer");
                                    return Ok(());
                                }
                            }
                            Err(e) => {
                                warn!("Failed to deserialize server message: {}", e);
                            }
                        }
                    }

                    // Update strategy to continue from current offset
                    strategy = PollingStrategy::offset(current_offset);
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if !err_str.contains("no messages") {
                        warn!("Poll error: {}", e);
                    }
                }
            }

            // Small delay to prevent busy-looping
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Replay server messages from a specific offset.
    ///
    /// This is useful for reconnecting clients to catch up on missed messages.
    pub async fn replay_server_messages(
        &self,
        from_offset: u64,
        max_messages: u32,
        workstream_filter: Option<WorkstreamId>,
    ) -> Result<Vec<ServerMessage>, TransportError> {
        if !self.connected {
            return Err(TransportError::NotConnected);
        }

        let stream_id = Identifier::named(&self.config.stream_name)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;
        let topic_id = Identifier::named(SERVER_TOPIC)
            .map_err(|e| TransportError::InvalidConfig(e.to_string()))?;

        let consumer = Consumer::default();

        let polled = self
            .client
            .poll_messages(
                &stream_id,
                &topic_id,
                Some(0), // partition_id 0
                &consumer,
                &PollingStrategy::offset(from_offset),
                max_messages,
                false,
            )
            .await
            .map_err(|e| TransportError::ReceiveFailed(e.to_string()))?;

        let messages: Vec<ServerMessage> = polled
            .messages
            .into_iter()
            .filter_map(|msg| {
                serde_json::from_slice::<ServerMessage>(&msg.payload)
                    .map(|mut m| {
                        m.offset = msg.header.offset;
                        m
                    })
                    .ok()
            })
            .filter(|m| {
                workstream_filter
                    .as_ref()
                    .map(|f| &m.workstream_id == f)
                    .unwrap_or(true)
            })
            .collect();

        Ok(messages)
    }

    /// Close the transport connection.
    pub async fn close(mut self) -> Result<(), TransportError> {
        self.connected = false;
        // IggyClient handles cleanup on drop
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.connected = false;
    }
}
