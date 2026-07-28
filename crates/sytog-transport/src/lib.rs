//! Versioned SYTOG messages carried over a replaceable WebSocket adapter.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sytog_domain::{CommandRequest, MessageId, ParticipantId, SessionEvent, SessionId};
use sytog_protocol::{Envelope, PROTOCOL_FAMILY, PROTOCOL_VERSION, SnapshotV0};
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

/// Wraps a typed message in the stable SYTOG protocol envelope.
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
        version: PROTOCOL_VERSION,
        message_id,
        session_id,
        sender_id,
        message_type: message.message_type().to_owned(),
        revision,
        payload: serde_json::to_value(message)?,
    })
}

/// Decodes and validates an envelope and its typed network payload.
///
/// # Errors
///
/// Returns an error for an invalid envelope, payload, or mismatched type label.
pub fn decode(envelope: &Envelope) -> Result<NetworkMessage, TransportError> {
    envelope.validate()?;
    let message: NetworkMessage = serde_json::from_value(envelope.payload.clone())?;
    if envelope.message_type != message.message_type() {
        return Err(TransportError::MessageTypeMismatch {
            envelope: envelope.message_type.clone(),
            payload: message.message_type().to_owned(),
        });
    }
    Ok(message)
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
    #[error("received envelope for session {actual}, expected {expected}")]
    SessionMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(decode(&wrapped).expect("valid envelope"), message);
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
}
