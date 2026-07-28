//! Versioned JSON boundary types. Internal domain decisions remain JSON-independent.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sytog_domain::{MessageId, ParticipantId, SessionEvent, SessionId, SessionState};
use thiserror::Error;

pub const PROTOCOL_FAMILY: &str = "sytog";
pub const PROTOCOL_VERSION_V1: u16 = 1;
pub const PROTOCOL_VERSION_V2: u16 = 2;
pub const LATEST_PROTOCOL_VERSION: u16 = PROTOCOL_VERSION_V2;
/// Wire version emitted by the current host and client implementation.
pub const ACTIVE_SERVER_PROTOCOL_VERSION: u16 = PROTOCOL_VERSION_V1;
/// Compatibility version used by the existing journal, snapshots, and V1
/// transport path. New wire code must select V1 or V2 explicitly.
pub const PROTOCOL_VERSION: u16 = ACTIVE_SERVER_PROTOCOL_VERSION;
pub const EVENT_LOG_SCHEMA_VERSION: u16 = 1;
pub const SNAPSHOT_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub family: String,
    pub version: u16,
    pub message_id: MessageId,
    pub session_id: SessionId,
    pub sender_id: ParticipantId,
    pub message_type: String,
    pub revision: Option<u64>,
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventLogV0 {
    pub family: String,
    pub protocol_version: u16,
    pub schema_version: u16,
    pub session_id: SessionId,
    pub base_revision: u64,
    pub events: Vec<SessionEvent>,
}

impl EventLogV0 {
    /// Validates log identity, versions, contiguous sequence, and event ids.
    ///
    /// # Errors
    ///
    /// Returns the first incompatible version, sequence gap, or duplicate.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_header(&self.family, self.protocol_version)?;
        if self.schema_version != EVENT_LOG_SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedEventLogSchema(
                self.schema_version,
            ));
        }
        validate_identifier(&self.session_id.0, "session_id")?;
        let mut event_ids = BTreeSet::new();
        for (index, event) in self.events.iter().enumerate() {
            validate_identifier(&event.event_id.0, "event_id")?;
            validate_identifier(&event.causation_id.0, "causation_id")?;
            validate_identifier(&event.actor.0, "actor")?;
            let offset = u64::try_from(index).map_err(|_| ProtocolError::SequenceOverflow)?;
            let expected = self
                .base_revision
                .checked_add(offset)
                .and_then(|revision| revision.checked_add(1))
                .ok_or(ProtocolError::SequenceOverflow)?;
            if event.sequence != expected {
                return Err(ProtocolError::UnexpectedEventSequence {
                    expected,
                    actual: event.sequence,
                });
            }
            if !event_ids.insert(event.event_id.clone()) {
                return Err(ProtocolError::DuplicateEventId(event.event_id.0.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotV0 {
    pub family: String,
    pub protocol_version: u16,
    pub schema_version: u16,
    pub session_id: SessionId,
    pub revision: u64,
    pub state: SessionState,
}

impl SnapshotV0 {
    /// Validates snapshot versions and state identity.
    ///
    /// # Errors
    ///
    /// Returns an error for incompatible versions or inconsistent state.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_header(&self.family, self.protocol_version)?;
        if self.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSnapshotSchema(
                self.schema_version,
            ));
        }
        validate_identifier(&self.session_id.0, "session_id")?;
        if self.state.session_id != self.session_id || self.state.revision != self.revision {
            return Err(ProtocolError::SnapshotIdentityMismatch);
        }
        Ok(())
    }
}

fn validate_header(family: &str, version: u16) -> Result<(), ProtocolError> {
    if family != PROTOCOL_FAMILY {
        return Err(ProtocolError::UnknownFamily(family.to_owned()));
    }
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(version));
    }
    Ok(())
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        Err(ProtocolError::EmptyIdentifier(field))
    } else {
        Ok(())
    }
}

impl Envelope {
    /// Validates a supported V1 or V2 envelope header.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown family, unsupported version, or empty
    /// message type.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.family != PROTOCOL_FAMILY {
            return Err(ProtocolError::UnknownFamily(self.family.clone()));
        }
        if !matches!(self.version, PROTOCOL_VERSION_V1 | PROTOCOL_VERSION_V2) {
            return Err(ProtocolError::UnsupportedVersion(self.version));
        }
        if self.message_type.trim().is_empty() {
            return Err(ProtocolError::EmptyMessageType);
        }
        validate_identifier(&self.message_id.0, "message_id")?;
        validate_identifier(&self.session_id.0, "session_id")?;
        validate_identifier(&self.sender_id.0, "sender_id")?;
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("unknown protocol family: {0}")]
    UnknownFamily(String),
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(u16),
    #[error("message_type must not be empty")]
    EmptyMessageType,
    #[error("{0} must not be empty")]
    EmptyIdentifier(&'static str),
    #[error("unsupported event log schema version: {0}")]
    UnsupportedEventLogSchema(u16),
    #[error("unsupported snapshot schema version: {0}")]
    UnsupportedSnapshotSchema(u16),
    #[error("expected event sequence {expected}, received {actual}")]
    UnexpectedEventSequence { expected: u64, actual: u64 },
    #[error("duplicate event id: {0}")]
    DuplicateEventId(String),
    #[error("event sequence overflow")]
    SequenceOverflow,
    #[error("snapshot identity or revision does not match its state")]
    SnapshotIdentityMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sytog_domain::{EventId, EventKind, SessionEventKind};

    fn event(sequence: u64, causation: &str) -> SessionEvent {
        SessionEvent {
            event_id: EventId::from_causation(&MessageId::from(causation), 0),
            sequence,
            causation_id: MessageId::from(causation),
            actor: ParticipantId::from("alice"),
            kind: EventKind::Session(SessionEventKind::SessionCreated {
                creator: ParticipantId::from("alice"),
                display_name: "Alice".to_owned(),
            }),
        }
    }

    #[test]
    fn unknown_versions_are_rejected_explicitly() {
        let envelope = Envelope {
            family: PROTOCOL_FAMILY.to_owned(),
            version: 99,
            message_id: MessageId::from("m1"),
            session_id: SessionId::from("s1"),
            sender_id: ParticipantId::from("p1"),
            message_type: "command".to_owned(),
            revision: Some(0),
            payload: Value::Null,
        };
        assert_eq!(
            envelope.validate(),
            Err(ProtocolError::UnsupportedVersion(99))
        );
    }

    #[test]
    fn v1_fixture_remains_a_supported_envelope() {
        let envelope: Envelope =
            serde_json::from_str(include_str!("../../../fixtures/protocol/envelope-v1.json"))
                .expect("V1 fixture deserializes");
        assert_eq!(envelope.version, PROTOCOL_VERSION_V1);
        envelope.validate().expect("V1 envelope remains supported");
    }

    #[test]
    fn v2_envelope_header_is_supported_without_reinterpreting_its_payload() {
        assert_eq!(ACTIVE_SERVER_PROTOCOL_VERSION, PROTOCOL_VERSION_V1);
        assert_eq!(PROTOCOL_VERSION, ACTIVE_SERVER_PROTOCOL_VERSION);
        assert_eq!(LATEST_PROTOCOL_VERSION, PROTOCOL_VERSION_V2);
        let envelope = Envelope {
            family: PROTOCOL_FAMILY.to_owned(),
            version: PROTOCOL_VERSION_V2,
            message_id: MessageId::from("m2"),
            session_id: SessionId::from("s1"),
            sender_id: ParticipantId::from("p1"),
            message_type: "resync_required".to_owned(),
            revision: Some(12),
            payload: Value::Null,
        };
        envelope.validate().expect("V2 header is supported");
    }

    #[test]
    fn log_rejects_sequence_gaps() {
        let log = EventLogV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: EVENT_LOG_SCHEMA_VERSION,
            session_id: SessionId::from("s1"),
            base_revision: 0,
            events: vec![event(2, "m1")],
        };
        assert!(matches!(
            log.validate(),
            Err(ProtocolError::UnexpectedEventSequence { .. })
        ));
    }

    #[test]
    fn log_allows_shared_causation_with_unique_event_ids() {
        let mut second = event(2, "m1");
        second.event_id = EventId::from_causation(&MessageId::from("m1"), 1);
        let log = EventLogV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: EVENT_LOG_SCHEMA_VERSION,
            session_id: SessionId::from("s1"),
            base_revision: 0,
            events: vec![event(1, "m1"), second],
        };
        assert_eq!(log.validate(), Ok(()));
    }

    #[test]
    fn log_rejects_duplicate_event_ids() {
        let log = EventLogV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: EVENT_LOG_SCHEMA_VERSION,
            session_id: SessionId::from("s1"),
            base_revision: 0,
            events: vec![event(1, "m1"), event(2, "m1")],
        };
        assert_eq!(
            log.validate(),
            Err(ProtocolError::DuplicateEventId("m1:0".to_owned()))
        );
    }
}
