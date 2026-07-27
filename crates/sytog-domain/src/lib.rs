//! Deterministic generic session domain.
//!
//! Session rules remain strongly typed. Activity payloads cross an explicit,
//! versioned envelope and are interpreted by an activity implementation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(SessionId);
string_id!(ParticipantId);
string_id!(MessageId);
string_id!(ActivityId);
string_id!(EventId);

impl EventId {
    #[must_use]
    pub fn from_causation(causation_id: &MessageId, ordinal: u32) -> Self {
        Self(format!("{}:{ordinal}", causation_id.0))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Uninitialized,
    Open,
    Active,
    Completed,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    pub id: ParticipantId,
    pub display_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityDescriptor {
    pub activity_id: ActivityId,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActiveActivity {
    pub descriptor: ActivityDescriptor,
    pub revision: u64,
    pub state: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivityCommandEnvelope {
    pub descriptor: ActivityDescriptor,
    pub command_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivityEventEnvelope {
    pub descriptor: ActivityDescriptor,
    pub activity_sequence: u64,
    pub event_type: String,
    pub payload: Value,
    pub resulting_state: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: SessionId,
    pub lifecycle: Lifecycle,
    pub participants: BTreeMap<ParticipantId, Participant>,
    pub authority: Option<ParticipantId>,
    pub activity: Option<ActiveActivity>,
    pub revision: u64,
}

impl SessionState {
    #[must_use]
    pub fn uninitialized(session_id: SessionId) -> Self {
        Self {
            session_id,
            lifecycle: Lifecycle::Uninitialized,
            participants: BTreeMap::new(),
            authority: None,
            activity: None,
            revision: 0,
        }
    }

    /// Applies exactly the next event to this state.
    ///
    /// # Errors
    ///
    /// Returns an error for an out-of-order or structurally invalid event.
    pub fn apply(&mut self, event: &SessionEvent) -> Result<(), ApplyError> {
        let expected = self
            .revision
            .checked_add(1)
            .ok_or(ApplyError::RevisionOverflow)?;
        if event.sequence != expected {
            return Err(ApplyError::UnexpectedSequence {
                expected,
                actual: event.sequence,
            });
        }

        match &event.kind {
            EventKind::Session(kind) => self.apply_session_event(&event.actor, kind)?,
            EventKind::Activity(activity_event) => {
                self.apply_activity_event(&event.actor, activity_event)?;
            }
        }
        self.revision = event.sequence;
        Ok(())
    }

    fn apply_session_event(
        &mut self,
        actor: &ParticipantId,
        event: &SessionEventKind,
    ) -> Result<(), ApplyError> {
        match event {
            SessionEventKind::SessionCreated {
                creator,
                display_name,
            } => {
                if self.lifecycle != Lifecycle::Uninitialized {
                    return Err(ApplyError::InvalidEvent("session already initialized"));
                }
                if actor != creator {
                    return Err(ApplyError::InvalidEvent(
                        "session creator must be the event actor",
                    ));
                }
                self.participants.insert(
                    creator.clone(),
                    Participant {
                        id: creator.clone(),
                        display_name: display_name.clone(),
                    },
                );
                self.authority = Some(creator.clone());
                self.lifecycle = Lifecycle::Open;
            }
            SessionEventKind::ParticipantJoined {
                participant,
                display_name,
            } => {
                if self.lifecycle != Lifecycle::Open {
                    return Err(ApplyError::InvalidEvent(
                        "participants may join only an open session",
                    ));
                }
                if self.participants.contains_key(participant) {
                    return Err(ApplyError::InvalidEvent("participant already exists"));
                }
                self.participants.insert(
                    participant.clone(),
                    Participant {
                        id: participant.clone(),
                        display_name: display_name.clone(),
                    },
                );
            }
            SessionEventKind::ActivityStarted {
                descriptor,
                initial_state,
            } => {
                self.require_authority_actor(actor)?;
                if self.lifecycle != Lifecycle::Open {
                    return Err(ApplyError::InvalidEvent(
                        "activity may start only in an open session",
                    ));
                }
                self.activity = Some(ActiveActivity {
                    descriptor: descriptor.clone(),
                    revision: 0,
                    state: initial_state.clone(),
                });
                self.lifecycle = Lifecycle::Active;
            }
            SessionEventKind::ActivityStopped { descriptor } => {
                self.require_authority_actor(actor)?;
                let active = self
                    .activity
                    .as_ref()
                    .ok_or(ApplyError::InvalidEvent("activity is missing"))?;
                if &active.descriptor != descriptor {
                    return Err(ApplyError::InvalidEvent("activity descriptor mismatch"));
                }
                self.activity = None;
                self.lifecycle = Lifecycle::Open;
            }
            SessionEventKind::AuthorityTransferred { from, to } => {
                if self.authority.as_ref() != Some(from) || actor != from {
                    return Err(ApplyError::InvalidEvent(
                        "authority transfer actor must hold authority",
                    ));
                }
                if from == to || !self.participants.contains_key(to) {
                    return Err(ApplyError::InvalidEvent(
                        "authority target must be another existing participant",
                    ));
                }
                self.authority = Some(to.clone());
            }
        }
        Ok(())
    }

    fn apply_activity_event(
        &mut self,
        actor: &ParticipantId,
        event: &ActivityEventEnvelope,
    ) -> Result<(), ApplyError> {
        if !self.participants.contains_key(actor) {
            return Err(ApplyError::InvalidEvent(
                "activity actor must be an existing participant",
            ));
        }
        let active = self
            .activity
            .as_mut()
            .ok_or(ApplyError::InvalidEvent("activity is missing"))?;
        if active.descriptor != event.descriptor {
            return Err(ApplyError::InvalidEvent("activity descriptor mismatch"));
        }
        let expected = active
            .revision
            .checked_add(1)
            .ok_or(ApplyError::ActivityRevisionOverflow)?;
        if event.activity_sequence != expected {
            return Err(ApplyError::UnexpectedActivitySequence {
                expected,
                actual: event.activity_sequence,
            });
        }
        active.revision = event.activity_sequence;
        active.state = event.resulting_state.clone();
        Ok(())
    }

    fn require_authority_actor(&self, actor: &ParticipantId) -> Result<(), ApplyError> {
        if self.authority.as_ref() == Some(actor) {
            Ok(())
        } else {
            Err(ApplyError::InvalidEvent("event actor must hold authority"))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", content = "command", rename_all = "snake_case")]
pub enum Command {
    Session(SessionCommand),
    Activity(ActivityCommandEnvelope),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionCommand {
    CreateSession { display_name: String },
    Join { display_name: String },
    StartActivity { descriptor: ActivityDescriptor },
    StopActivity,
    TransferAuthority { to: ParticipantId },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub message_id: MessageId,
    pub actor: ParticipantId,
    pub expected_revision: u64,
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub event_id: EventId,
    pub sequence: u64,
    pub causation_id: MessageId,
    pub actor: ParticipantId,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", content = "event", rename_all = "snake_case")]
pub enum EventKind {
    Session(SessionEventKind),
    Activity(ActivityEventEnvelope),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEventKind {
    SessionCreated {
        creator: ParticipantId,
        display_name: String,
    },
    ParticipantJoined {
        participant: ParticipantId,
        display_name: String,
    },
    ActivityStarted {
        descriptor: ActivityDescriptor,
        initial_state: Value,
    },
    ActivityStopped {
        descriptor: ActivityDescriptor,
    },
    AuthorityTransferred {
        from: ParticipantId,
        to: ParticipantId,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ApplyError {
    #[error("expected event sequence {expected}, received {actual}")]
    UnexpectedSequence { expected: u64, actual: u64 },
    #[error("expected activity sequence {expected}, received {actual}")]
    UnexpectedActivitySequence { expected: u64, actual: u64 },
    #[error("invalid event: {0}")]
    InvalidEvent(&'static str),
    #[error("session revision overflow")]
    RevisionOverflow,
    #[error("activity revision overflow")]
    ActivityRevisionOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reducer_rejects_forged_creator_actor() {
        let mut state = SessionState::uninitialized(SessionId::from("test"));
        let event = SessionEvent {
            event_id: EventId::from("event-1"),
            sequence: 1,
            causation_id: MessageId::from("m1"),
            actor: ParticipantId::from("mallory"),
            kind: EventKind::Session(SessionEventKind::SessionCreated {
                creator: ParticipantId::from("alice"),
                display_name: "Alice".to_owned(),
            }),
        };

        assert!(matches!(
            state.apply(&event),
            Err(ApplyError::InvalidEvent(_))
        ));
        assert_eq!(state.revision, 0);
    }
}
