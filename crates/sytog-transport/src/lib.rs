//! Versioned SYTOG messages carried over a replaceable WebSocket adapter.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sytog_domain::{CommandRequest, MessageId, ParticipantId, SessionEvent, SessionId};
use sytog_protocol::{
    ACTIVE_SERVER_PROTOCOL_VERSION, Envelope, PROTOCOL_FAMILY, PROTOCOL_VERSION_V1,
    PROTOCOL_VERSION_V2, SnapshotV0,
};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{self, Message},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum NetworkMessage {
    Hello {
        last_sequence: u64,
    },
    JoinSession {
        display_name: String,
    },
    SubmitCommand {
        request: CommandRequest,
    },
    CommandRejected {
        message_id: MessageId,
        code: String,
        detail: String,
        current_revision: u64,
    },
    EventBatch {
        from_sequence: u64,
        events: Vec<SessionEvent>,
    },
    StateSnapshot {
        snapshot: SnapshotV0,
    },
    CatchUpRequest {
        after_sequence: u64,
    },
}

impl NetworkMessage {
    #[must_use]
    pub const fn message_type(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::JoinSession { .. } => "join_session",
            Self::SubmitCommand { .. } => "submit_command",
            Self::CommandRejected { .. } => "command_rejected",
            Self::EventBatch { .. } => "event_batch",
            Self::StateSnapshot { .. } => "state_snapshot",
            Self::CatchUpRequest { .. } => "catch_up_request",
        }
    }
}

/// Stable close code for an established connection that must resynchronize.
pub const RESYNC_REQUIRED_CLOSE_CODE: u16 = 4001;
/// Stable close reason paired with [`RESYNC_REQUIRED_CLOSE_CODE`].
pub const RESYNC_REQUIRED_CLOSE_REASON: &str = "resync_required";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResyncReason {
    OutboundQueueFull,
    OutboundByteBudgetExceeded,
    WriteTimeout,
    CatchUpLimitExceeded,
    CursorBeforeRetention,
}

impl ResyncReason {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OutboundQueueFull => "outbound_queue_full",
            Self::OutboundByteBudgetExceeded => "outbound_byte_budget_exceeded",
            Self::WriteTimeout => "write_timeout",
            Self::CatchUpLimitExceeded => "catch_up_limit_exceeded",
            Self::CursorBeforeRetention => "cursor_before_retention",
        }
    }

    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::OutboundQueueFull => 4101,
            Self::OutboundByteBudgetExceeded => 4102,
            Self::WriteTimeout => 4103,
            Self::CatchUpLimitExceeded => 4104,
            Self::CursorBeforeRetention => 4105,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionRejectionReason {
    ServerOverloaded,
}

impl AdmissionRejectionReason {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ServerOverloaded => "server_overloaded",
        }
    }

    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::ServerOverloaded => 4201,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatchUpPageMetadata {
    pub earliest_available_sequence: u64,
    pub from_sequence: u64,
    pub through_sequence: u64,
    pub current_sequence: u64,
    pub snapshot_revision: Option<u64>,
    pub terminal: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatchUpPage {
    pub metadata: CatchUpPageMetadata,
    pub events: Vec<SessionEvent>,
}

impl CatchUpPage {
    /// Validates page bounds and its exact contiguous event range.
    ///
    /// # Errors
    ///
    /// Returns the first empty, inverted, non-contiguous, or contradictory
    /// bound.
    pub fn validate(&self) -> Result<(), CatchUpPageError> {
        let metadata = &self.metadata;
        if self.events.is_empty() {
            return Err(CatchUpPageError::Empty);
        }
        if metadata.from_sequence == 0 {
            return Err(CatchUpPageError::ZeroSequence);
        }
        if metadata.from_sequence > metadata.through_sequence {
            return Err(CatchUpPageError::Inverted {
                from: metadata.from_sequence,
                through: metadata.through_sequence,
            });
        }
        if metadata.earliest_available_sequence > metadata.from_sequence {
            return Err(CatchUpPageError::EarliestAfterStart {
                earliest: metadata.earliest_available_sequence,
                from: metadata.from_sequence,
            });
        }
        if metadata.through_sequence > metadata.current_sequence {
            return Err(CatchUpPageError::BeyondCurrent {
                through: metadata.through_sequence,
                current: metadata.current_sequence,
            });
        }
        let expected_terminal = metadata.through_sequence == metadata.current_sequence;
        if metadata.terminal != expected_terminal {
            return Err(CatchUpPageError::TerminalMismatch {
                terminal: metadata.terminal,
                through: metadata.through_sequence,
                current: metadata.current_sequence,
            });
        }
        if let Some(snapshot_revision) = metadata.snapshot_revision {
            let expected_from = snapshot_revision
                .checked_add(1)
                .ok_or(CatchUpPageError::SequenceOverflow)?;
            if metadata.from_sequence != expected_from {
                return Err(CatchUpPageError::SnapshotBoundary {
                    snapshot_revision,
                    from: metadata.from_sequence,
                });
            }
        }
        validate_event_range(
            metadata.from_sequence,
            metadata.through_sequence,
            &self.events,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CatchUpPageError {
    #[error("catch-up page must contain at least one event")]
    Empty,
    #[error("event sequences start at 1")]
    ZeroSequence,
    #[error("catch-up page range is inverted: from={from}, through={through}")]
    Inverted { from: u64, through: u64 },
    #[error("earliest available sequence {earliest} is after page start {from}")]
    EarliestAfterStart { earliest: u64, from: u64 },
    #[error("catch-up page ends at {through}, after current sequence {current}")]
    BeyondCurrent { through: u64, current: u64 },
    #[error("terminal={terminal} contradicts page end {through} and current sequence {current}")]
    TerminalMismatch {
        terminal: bool,
        through: u64,
        current: u64,
    },
    #[error("snapshot revision {snapshot_revision} is not immediately before page start {from}")]
    SnapshotBoundary { snapshot_revision: u64, from: u64 },
    #[error("event range must not be empty")]
    EmptyEventRange,
    #[error("event sequence {actual} is not the expected contiguous sequence {expected}")]
    NonContiguous { expected: u64, actual: u64 },
    #[error("event range ends at {actual}, expected {expected}")]
    RangeEndMismatch { expected: u64, actual: u64 },
    #[error("event sequence overflow")]
    SequenceOverflow,
}

fn validate_event_range(
    from_sequence: u64,
    through_sequence: u64,
    events: &[SessionEvent],
) -> Result<(), CatchUpPageError> {
    if events.is_empty() {
        return Err(CatchUpPageError::EmptyEventRange);
    }
    for (offset, event) in events.iter().enumerate() {
        let offset = u64::try_from(offset).map_err(|_| CatchUpPageError::SequenceOverflow)?;
        let expected = from_sequence
            .checked_add(offset)
            .ok_or(CatchUpPageError::SequenceOverflow)?;
        if event.sequence != expected {
            return Err(CatchUpPageError::NonContiguous {
                expected,
                actual: event.sequence,
            });
        }
    }
    let actual = events
        .last()
        .map(|event| event.sequence)
        .ok_or(CatchUpPageError::EmptyEventRange)?;
    if actual != through_sequence {
        return Err(CatchUpPageError::RangeEndMismatch {
            expected: through_sequence,
            actual,
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum NetworkMessageV2 {
    Hello {
        last_applied_sequence: u64,
    },
    JoinSession {
        display_name: String,
    },
    SubmitCommand {
        request: CommandRequest,
    },
    CommandRejected {
        message_id: MessageId,
        code: String,
        detail: String,
        current_sequence: u64,
    },
    AdmissionRejected {
        message_id: MessageId,
        reason: AdmissionRejectionReason,
        current_sequence: u64,
    },
    EventBatch {
        from_sequence: u64,
        through_sequence: u64,
        events: Vec<SessionEvent>,
    },
    StateSnapshot {
        snapshot: SnapshotV0,
    },
    CatchUpRequest {
        after_sequence: u64,
    },
    CatchUpPage {
        page: CatchUpPage,
    },
    ResyncRequired {
        reason: ResyncReason,
        earliest_available_sequence: u64,
        current_sequence: u64,
        snapshot_revision: Option<u64>,
    },
}

impl NetworkMessageV2 {
    #[must_use]
    pub const fn message_type(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::JoinSession { .. } => "join_session",
            Self::SubmitCommand { .. } => "submit_command",
            Self::CommandRejected { .. } => "command_rejected",
            Self::AdmissionRejected { .. } => "admission_rejected",
            Self::EventBatch { .. } => "event_batch",
            Self::StateSnapshot { .. } => "state_snapshot",
            Self::CatchUpRequest { .. } => "catch_up_request",
            Self::CatchUpPage { .. } => "catch_up_page",
            Self::ResyncRequired { .. } => "resync_required",
        }
    }

    /// Validates V2 message-level sequence and recovery bounds.
    ///
    /// # Errors
    ///
    /// Returns a page or message-bound contradiction.
    pub fn validate(&self) -> Result<(), TransportError> {
        match self {
            Self::EventBatch {
                from_sequence,
                through_sequence,
                events,
            } => validate_event_range(*from_sequence, *through_sequence, events)
                .map_err(TransportError::InvalidCatchUpPage),
            Self::CatchUpPage { page } => {
                page.validate().map_err(TransportError::InvalidCatchUpPage)
            }
            Self::ResyncRequired {
                earliest_available_sequence,
                current_sequence,
                snapshot_revision,
                ..
            } => {
                let latest_valid_floor = current_sequence
                    .checked_add(1)
                    .ok_or(TransportError::SequenceOverflow)?;
                if *earliest_available_sequence == 0
                    || *earliest_available_sequence > latest_valid_floor
                {
                    return Err(TransportError::InvalidRecoveryBounds {
                        earliest: *earliest_available_sequence,
                        current: *current_sequence,
                    });
                }
                if let Some(snapshot) = snapshot_revision {
                    if *snapshot > *current_sequence {
                        return Err(TransportError::SnapshotAfterCurrent {
                            snapshot: *snapshot,
                            current: *current_sequence,
                        });
                    }
                    let suffix_start = snapshot
                        .checked_add(1)
                        .ok_or(TransportError::SequenceOverflow)?;
                    if suffix_start < *earliest_available_sequence {
                        return Err(TransportError::SnapshotBeforeAvailable {
                            snapshot: *snapshot,
                            earliest: *earliest_available_sequence,
                        });
                    }
                }
                Ok(())
            }
            Self::Hello { .. }
            | Self::JoinSession { .. }
            | Self::SubmitCommand { .. }
            | Self::CommandRejected { .. }
            | Self::AdmissionRejected { .. }
            | Self::CatchUpRequest { .. } => Ok(()),
            Self::StateSnapshot { snapshot } => {
                snapshot.validate().map_err(TransportError::Protocol)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum VersionedNetworkMessage {
    V1(NetworkMessage),
    V2(NetworkMessageV2),
}

/// Wraps a V1 message in the stable V1 SYTOG envelope.
///
/// # Errors
///
/// Returns an error if the typed payload cannot be serialized.
pub fn envelope(
    session_id: SessionId,
    sender_id: ParticipantId,
    message_id: MessageId,
    revision: Option<u64>,
    message: &NetworkMessage,
) -> Result<Envelope, TransportError> {
    Ok(Envelope {
        family: PROTOCOL_FAMILY.to_owned(),
        version: ACTIVE_SERVER_PROTOCOL_VERSION,
        message_id,
        session_id,
        sender_id,
        message_type: message.message_type().to_owned(),
        revision,
        payload: serde_json::to_value(message)?,
    })
}

/// Wraps a V2 message in a V2 SYTOG envelope.
///
/// # Errors
///
/// Returns an error if the message is invalid or cannot be serialized.
pub fn envelope_v2(
    session_id: SessionId,
    sender_id: ParticipantId,
    message_id: MessageId,
    revision: Option<u64>,
    message: &NetworkMessageV2,
) -> Result<Envelope, TransportError> {
    message.validate()?;
    Ok(Envelope {
        family: PROTOCOL_FAMILY.to_owned(),
        version: PROTOCOL_VERSION_V2,
        message_id,
        session_id,
        sender_id,
        message_type: message.message_type().to_owned(),
        revision,
        payload: serde_json::to_value(message)?,
    })
}

/// Decodes and validates a V1 envelope and its typed network payload.
///
/// # Errors
///
/// Returns an error for an invalid or non-V1 envelope, payload, or mismatched
/// type label.
pub fn decode(envelope: &Envelope) -> Result<NetworkMessage, TransportError> {
    envelope.validate()?;
    if envelope.version != PROTOCOL_VERSION_V1 {
        return Err(TransportError::ProtocolVersionMismatch {
            expected: PROTOCOL_VERSION_V1,
            actual: envelope.version,
        });
    }
    let message: NetworkMessage = serde_json::from_value(envelope.payload.clone())?;
    if envelope.message_type != message.message_type() {
        return Err(TransportError::MessageTypeMismatch {
            envelope: envelope.message_type.clone(),
            payload: message.message_type().to_owned(),
        });
    }
    Ok(message)
}

/// Decodes and validates a V2 envelope and its typed network payload.
///
/// # Errors
///
/// Returns an error for an invalid or non-V2 envelope, payload, message bounds,
/// or mismatched type label.
pub fn decode_v2(envelope: &Envelope) -> Result<NetworkMessageV2, TransportError> {
    envelope.validate()?;
    if envelope.version != PROTOCOL_VERSION_V2 {
        return Err(TransportError::ProtocolVersionMismatch {
            expected: PROTOCOL_VERSION_V2,
            actual: envelope.version,
        });
    }
    let message: NetworkMessageV2 = serde_json::from_value(envelope.payload.clone())?;
    if envelope.message_type != message.message_type() {
        return Err(TransportError::MessageTypeMismatch {
            envelope: envelope.message_type.clone(),
            payload: message.message_type().to_owned(),
        });
    }
    message.validate()?;
    Ok(message)
}

/// Decodes a supported envelope without reinterpreting V1 as V2 or V2 as V1.
///
/// # Errors
///
/// Returns an error for an unknown version or an invalid version-specific
/// payload.
pub fn decode_versioned(envelope: &Envelope) -> Result<VersionedNetworkMessage, TransportError> {
    envelope.validate()?;
    match envelope.version {
        PROTOCOL_VERSION_V1 => decode(envelope).map(VersionedNetworkMessage::V1),
        PROTOCOL_VERSION_V2 => decode_v2(envelope).map(VersionedNetworkMessage::V2),
        version => Err(TransportError::Protocol(
            sytog_protocol::ProtocolError::UnsupportedVersion(version),
        )),
    }
}

pub type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub type ServerSocket = WebSocketStream<TcpStream>;

/// Opens a WebSocket client connection.
///
/// # Errors
///
/// Returns the WebSocket handshake or connection error.
pub async fn connect(url: &str) -> Result<ClientSocket, TransportError> {
    let (socket, _) = connect_async(url).await?;
    Ok(socket)
}

/// Sends a validated SYTOG envelope as WebSocket text.
///
/// # Errors
///
/// Returns a JSON serialization or WebSocket error.
pub async fn send<S>(sink: &mut S, envelope: &Envelope) -> Result<(), TransportError>
where
    S: futures_util::Sink<Message, Error = tungstenite::Error> + Unpin,
{
    let text = serde_json::to_string(envelope)?;
    sink.send(Message::Text(text.into())).await?;
    Ok(())
}

/// Reads the next SYTOG envelope, handling ping/close frames.
///
/// # Errors
///
/// Returns an invalid-frame, JSON, or WebSocket error.
pub async fn receive<S>(stream: &mut S) -> Result<Option<Envelope>, TransportError>
where
    S: futures_util::Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    while let Some(frame) = stream.next().await {
        match frame? {
            Message::Text(text) => return Ok(Some(serde_json::from_str(&text)?)),
            Message::Binary(bytes) => return Ok(Some(serde_json::from_slice(&bytes)?)),
            Message::Close(_) => return Ok(None),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
    Ok(None)
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error(transparent)]
    WebSocket(#[from] tungstenite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Protocol(#[from] sytog_protocol::ProtocolError),
    #[error("message type mismatch: envelope={envelope}, payload={payload}")]
    MessageTypeMismatch { envelope: String, payload: String },
    #[error("protocol version mismatch: expected={expected}, actual={actual}")]
    ProtocolVersionMismatch { expected: u16, actual: u16 },
    #[error("invalid catch-up or event page: {0}")]
    InvalidCatchUpPage(CatchUpPageError),
    #[error(
        "earliest available sequence {earliest} is inconsistent with current sequence {current}"
    )]
    InvalidRecoveryBounds { earliest: u64, current: u64 },
    #[error("snapshot revision {snapshot} is after current sequence {current}")]
    SnapshotAfterCurrent { snapshot: u64, current: u64 },
    #[error("snapshot revision {snapshot} cannot bridge to earliest available sequence {earliest}")]
    SnapshotBeforeAvailable { snapshot: u64, earliest: u64 },
    #[error("sequence overflow")]
    SequenceOverflow,
    #[error("received envelope for session {actual}, expected {expected}")]
    SessionMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use sytog_domain::{EventId, EventKind, SessionEventKind};

    fn event(sequence: u64) -> SessionEvent {
        SessionEvent {
            event_id: EventId(format!("event-{sequence}")),
            sequence,
            causation_id: MessageId(format!("command-{sequence}")),
            actor: ParticipantId::from("alice"),
            kind: EventKind::Session(SessionEventKind::SessionCreated {
                creator: ParticipantId::from("alice"),
                display_name: "Alice".to_owned(),
            }),
        }
    }

    #[test]
    fn envelope_round_trip_preserves_catch_up_request() {
        let message = NetworkMessage::CatchUpRequest { after_sequence: 41 };
        let wrapped = envelope(
            SessionId::from("session"),
            ParticipantId::from("alice"),
            MessageId::from("m1"),
            Some(41),
            &message,
        )
        .expect("message serializes");
        assert_eq!(wrapped.version, PROTOCOL_VERSION_V1);
        assert_eq!(decode(&wrapped).expect("valid envelope"), message);
        assert_eq!(
            decode_versioned(&wrapped).expect("V1 dispatch remains supported"),
            VersionedNetworkMessage::V1(message)
        );
    }

    #[test]
    fn envelope_type_mismatch_is_rejected() {
        let message = NetworkMessage::Hello { last_sequence: 0 };
        let mut wrapped = envelope(
            SessionId::from("session"),
            ParticipantId::from("alice"),
            MessageId::from("m1"),
            Some(0),
            &message,
        )
        .expect("message serializes");
        wrapped.message_type = "event_batch".to_owned();
        assert!(matches!(
            decode(&wrapped),
            Err(TransportError::MessageTypeMismatch { .. })
        ));
    }

    #[test]
    fn v2_resync_required_round_trips_without_v1_reinterpretation() {
        let message = NetworkMessageV2::ResyncRequired {
            reason: ResyncReason::OutboundQueueFull,
            earliest_available_sequence: 1,
            current_sequence: 42,
            snapshot_revision: None,
        };
        let wrapped = envelope_v2(
            SessionId::from("session"),
            ParticipantId::from("host"),
            MessageId::from("resync-1"),
            Some(42),
            &message,
        )
        .expect("V2 message serializes");
        let encoded = serde_json::to_string(&wrapped).expect("envelope serializes");
        let decoded: Envelope = serde_json::from_str(&encoded).expect("envelope deserializes");

        assert_eq!(decoded.version, PROTOCOL_VERSION_V2);
        assert_eq!(
            decode_versioned(&decoded).expect("V2 dispatch succeeds"),
            VersionedNetworkMessage::V2(message)
        );
        assert!(matches!(
            decode(&decoded),
            Err(TransportError::ProtocolVersionMismatch {
                expected: PROTOCOL_VERSION_V1,
                actual: PROTOCOL_VERSION_V2,
            })
        ));
    }

    #[test]
    fn unknown_wire_version_is_rejected_explicitly() {
        let mut wrapped = envelope(
            SessionId::from("session"),
            ParticipantId::from("alice"),
            MessageId::from("m1"),
            Some(0),
            &NetworkMessage::Hello { last_sequence: 0 },
        )
        .expect("V1 message serializes");
        wrapped.version = 99;

        assert!(matches!(
            decode_versioned(&wrapped),
            Err(TransportError::Protocol(
                sytog_protocol::ProtocolError::UnsupportedVersion(99)
            ))
        ));
    }

    #[test]
    fn paged_catch_up_round_trips_with_coherent_non_terminal_bounds() {
        let page = CatchUpPage {
            metadata: CatchUpPageMetadata {
                earliest_available_sequence: 1,
                from_sequence: 11,
                through_sequence: 12,
                current_sequence: 20,
                snapshot_revision: None,
                terminal: false,
            },
            events: vec![event(11), event(12)],
        };
        page.validate().expect("page bounds are coherent");
        let message = NetworkMessageV2::CatchUpPage { page };
        let wrapped = envelope_v2(
            SessionId::from("session"),
            ParticipantId::from("host"),
            MessageId::from("page-1"),
            Some(12),
            &message,
        )
        .expect("page serializes");

        assert_eq!(
            decode_v2(&wrapped).expect("page deserializes and validates"),
            message
        );
    }

    #[test]
    fn terminal_page_must_end_at_current_sequence() {
        let page = CatchUpPage {
            metadata: CatchUpPageMetadata {
                earliest_available_sequence: 1,
                from_sequence: 19,
                through_sequence: 20,
                current_sequence: 20,
                snapshot_revision: None,
                terminal: true,
            },
            events: vec![event(19), event(20)],
        };
        page.validate()
            .expect("terminal page reaches current sequence");

        let mut contradictory = page;
        contradictory.metadata.terminal = false;
        assert!(matches!(
            contradictory.validate(),
            Err(CatchUpPageError::TerminalMismatch { .. })
        ));
    }

    #[test]
    fn empty_inverted_and_non_contiguous_pages_are_rejected() {
        let empty = CatchUpPage {
            metadata: CatchUpPageMetadata {
                earliest_available_sequence: 1,
                from_sequence: 1,
                through_sequence: 1,
                current_sequence: 1,
                snapshot_revision: None,
                terminal: true,
            },
            events: Vec::new(),
        };
        assert_eq!(empty.validate(), Err(CatchUpPageError::Empty));

        let inverted = CatchUpPage {
            metadata: CatchUpPageMetadata {
                earliest_available_sequence: 1,
                from_sequence: 3,
                through_sequence: 2,
                current_sequence: 3,
                snapshot_revision: None,
                terminal: false,
            },
            events: vec![event(3)],
        };
        assert!(matches!(
            inverted.validate(),
            Err(CatchUpPageError::Inverted {
                from: 3,
                through: 2
            })
        ));

        let non_contiguous = CatchUpPage {
            metadata: CatchUpPageMetadata {
                earliest_available_sequence: 1,
                from_sequence: 2,
                through_sequence: 4,
                current_sequence: 5,
                snapshot_revision: None,
                terminal: false,
            },
            events: vec![event(2), event(4)],
        };
        assert!(matches!(
            non_contiguous.validate(),
            Err(CatchUpPageError::NonContiguous {
                expected: 3,
                actual: 4
            })
        ));

        let zero = CatchUpPage {
            metadata: CatchUpPageMetadata {
                earliest_available_sequence: 0,
                from_sequence: 0,
                through_sequence: 0,
                current_sequence: 0,
                snapshot_revision: None,
                terminal: true,
            },
            events: vec![event(0)],
        };
        assert_eq!(zero.validate(), Err(CatchUpPageError::ZeroSequence));
    }

    #[test]
    fn snapshot_revision_must_be_immediately_before_its_suffix() {
        let page = CatchUpPage {
            metadata: CatchUpPageMetadata {
                earliest_available_sequence: 1,
                from_sequence: 12,
                through_sequence: 12,
                current_sequence: 12,
                snapshot_revision: Some(10),
                terminal: true,
            },
            events: vec![event(12)],
        };
        assert!(matches!(
            page.validate(),
            Err(CatchUpPageError::SnapshotBoundary {
                snapshot_revision: 10,
                from: 12
            })
        ));
    }

    #[test]
    fn resync_required_can_offer_a_snapshot_that_bridges_retention() {
        let message = NetworkMessageV2::ResyncRequired {
            reason: ResyncReason::CursorBeforeRetention,
            earliest_available_sequence: 51,
            current_sequence: 100,
            snapshot_revision: Some(50),
        };
        let wrapped = envelope_v2(
            SessionId::from("session"),
            ParticipantId::from("host"),
            MessageId::from("resync-snapshot"),
            Some(100),
            &message,
        )
        .expect("snapshot recovery bounds are valid");
        assert_eq!(decode_v2(&wrapped).expect("message round-trips"), message);
    }

    #[test]
    fn v2_resync_fixture_has_stable_wire_names() {
        let envelope: Envelope = serde_json::from_str(include_str!(
            "../../../fixtures/protocol/envelope-v2-resync-required.json"
        ))
        .expect("V2 fixture deserializes");
        let message = decode_v2(&envelope).expect("V2 fixture validates");
        assert_eq!(
            message,
            NetworkMessageV2::ResyncRequired {
                reason: ResyncReason::CursorBeforeRetention,
                earliest_available_sequence: 51,
                current_sequence: 100,
                snapshot_revision: Some(50),
            }
        );
    }

    #[test]
    fn overload_reason_names_and_codes_are_stable() {
        let resync_reasons = [
            (ResyncReason::OutboundQueueFull, "outbound_queue_full", 4101),
            (
                ResyncReason::OutboundByteBudgetExceeded,
                "outbound_byte_budget_exceeded",
                4102,
            ),
            (ResyncReason::WriteTimeout, "write_timeout", 4103),
            (
                ResyncReason::CatchUpLimitExceeded,
                "catch_up_limit_exceeded",
                4104,
            ),
            (
                ResyncReason::CursorBeforeRetention,
                "cursor_before_retention",
                4105,
            ),
        ];
        for (reason, name, code) in resync_reasons {
            assert_eq!(reason.name(), name);
            assert_eq!(reason.code(), code);
            assert_eq!(
                serde_json::to_value(reason).expect("reason serializes"),
                serde_json::Value::String(name.to_owned())
            );
        }
        assert_eq!(
            AdmissionRejectionReason::ServerOverloaded.name(),
            "server_overloaded"
        );
        assert_eq!(AdmissionRejectionReason::ServerOverloaded.code(), 4201);
        assert_eq!(RESYNC_REQUIRED_CLOSE_CODE, 4001);
        assert_eq!(RESYNC_REQUIRED_CLOSE_REASON, "resync_required");
    }

    #[test]
    fn pre_admission_overload_is_distinct_from_established_connection_resync() {
        let admission = NetworkMessageV2::AdmissionRejected {
            message_id: MessageId::from("command-1"),
            reason: AdmissionRejectionReason::ServerOverloaded,
            current_sequence: 7,
        };
        let resync = NetworkMessageV2::ResyncRequired {
            reason: ResyncReason::CatchUpLimitExceeded,
            earliest_available_sequence: 1,
            current_sequence: 7,
            snapshot_revision: None,
        };

        assert_eq!(admission.message_type(), "admission_rejected");
        assert_eq!(resync.message_type(), "resync_required");
        assert_ne!(
            serde_json::to_value(admission).expect("admission serializes"),
            serde_json::to_value(resync).expect("resync serializes")
        );
    }
}
