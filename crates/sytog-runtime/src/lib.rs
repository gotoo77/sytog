//! Pure session decision, activity routing, reduction, snapshot, and replay.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sytog_domain::{
    ActiveActivity, ActivityCommandEnvelope, ActivityDescriptor, ActivityEventEnvelope, ApplyError,
    Command, CommandRequest, EventId, EventKind, Lifecycle, ParticipantId, SessionCommand,
    SessionEvent, SessionEventKind, SessionState,
};
use sytog_protocol::EventLogV0;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub events: Vec<SessionEvent>,
    pub effects: Vec<RequestedEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestedEffect {
    BroadcastEvent { sequence: u64 },
    PersistEvent { sequence: u64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivityTransition {
    pub event_type: String,
    pub payload: Value,
    pub resulting_state: Value,
}

/// Minimal typed seam implemented by an activity adapter.
pub trait ActivityEngine {
    fn descriptor(&self) -> ActivityDescriptor;
    fn initial_state(&self) -> Value;

    /// Validates an activity command and returns its accepted transition.
    ///
    /// # Errors
    ///
    /// Returns a stable activity-specific rejection.
    fn decide(
        &self,
        actor: &ParticipantId,
        current: &ActiveActivity,
        command: &ActivityCommandEnvelope,
    ) -> Result<ActivityTransition, ActivityRejection>;
}

/// Validates a command and describes accepted facts and external effects.
///
/// # Errors
///
/// Returns a structured rejection when revision, permission, lifecycle,
/// activity routing, or payload invariants do not hold.
pub fn decide(
    state: &SessionState,
    request: &CommandRequest,
    activity: Option<&dyn ActivityEngine>,
) -> Result<Decision, Rejection> {
    if request.expected_revision != state.revision {
        return Err(Rejection::RevisionConflict {
            expected: state.revision,
            actual: request.expected_revision,
        });
    }

    let kind = match &request.command {
        Command::Session(command) => decide_session(state, request, command, activity)?,
        Command::Activity(command) => decide_activity(state, request, command, activity)?,
    };

    let sequence = state
        .revision
        .checked_add(1)
        .ok_or(Rejection::RevisionOverflow)?;
    Ok(Decision {
        events: vec![SessionEvent {
            event_id: EventId::from_causation(&request.message_id, 0),
            sequence,
            causation_id: request.message_id.clone(),
            actor: request.actor.clone(),
            kind,
        }],
        effects: vec![
            RequestedEffect::PersistEvent { sequence },
            RequestedEffect::BroadcastEvent { sequence },
        ],
    })
}

fn decide_session(
    state: &SessionState,
    request: &CommandRequest,
    command: &SessionCommand,
    activity: Option<&dyn ActivityEngine>,
) -> Result<EventKind, Rejection> {
    let event = match command {
        SessionCommand::CreateSession { display_name } => {
            if state.lifecycle != Lifecycle::Uninitialized {
                return Err(Rejection::AlreadyInitialized);
            }
            SessionEventKind::SessionCreated {
                creator: request.actor.clone(),
                display_name: required_text(display_name, "display_name")?,
            }
        }
        SessionCommand::Join { display_name } => {
            require_initialized(state)?;
            if state.lifecycle != Lifecycle::Open {
                return Err(Rejection::SessionNotOpen);
            }
            if state.participants.contains_key(&request.actor) {
                return Err(Rejection::ParticipantAlreadyJoined);
            }
            SessionEventKind::ParticipantJoined {
                participant: request.actor.clone(),
                display_name: required_text(display_name, "display_name")?,
            }
        }
        SessionCommand::StartActivity { descriptor } => {
            require_authority(state, &request.actor)?;
            if state.lifecycle != Lifecycle::Open {
                return Err(Rejection::SessionNotOpen);
            }
            let engine = matching_engine(activity, descriptor)?;
            SessionEventKind::ActivityStarted {
                descriptor: descriptor.clone(),
                initial_state: engine.initial_state(),
            }
        }
        SessionCommand::StopActivity => {
            require_authority(state, &request.actor)?;
            let active = state
                .activity
                .as_ref()
                .ok_or(Rejection::ActivityNotActive)?;
            SessionEventKind::ActivityStopped {
                descriptor: active.descriptor.clone(),
            }
        }
        SessionCommand::TransferAuthority { to } => {
            require_authority(state, &request.actor)?;
            require_participant(state, to)?;
            if to == &request.actor {
                return Err(Rejection::AuthorityAlreadyHeld);
            }
            SessionEventKind::AuthorityTransferred {
                from: request.actor.clone(),
                to: to.clone(),
            }
        }
    };
    Ok(EventKind::Session(event))
}

fn decide_activity(
    state: &SessionState,
    request: &CommandRequest,
    command: &ActivityCommandEnvelope,
    activity: Option<&dyn ActivityEngine>,
) -> Result<EventKind, Rejection> {
    require_participant(state, &request.actor)?;
    let active = state
        .activity
        .as_ref()
        .ok_or(Rejection::ActivityNotActive)?;
    if active.descriptor != command.descriptor {
        return Err(Rejection::ActivityDescriptorMismatch);
    }
    let engine = matching_engine(activity, &active.descriptor)?;
    let transition = engine
        .decide(&request.actor, active, command)
        .map_err(Rejection::ActivityRejected)?;
    let activity_sequence = active
        .revision
        .checked_add(1)
        .ok_or(Rejection::ActivityRevisionOverflow)?;
    Ok(EventKind::Activity(ActivityEventEnvelope {
        descriptor: active.descriptor.clone(),
        activity_sequence,
        event_type: transition.event_type,
        payload: transition.payload,
        resulting_state: transition.resulting_state,
    }))
}

fn matching_engine<'a>(
    activity: Option<&'a dyn ActivityEngine>,
    descriptor: &ActivityDescriptor,
) -> Result<&'a dyn ActivityEngine, Rejection> {
    let engine = activity.ok_or(Rejection::ActivityEngineUnavailable)?;
    if engine.descriptor() == *descriptor {
        Ok(engine)
    } else {
        Err(Rejection::ActivityEngineUnavailable)
    }
}

/// Decides and atomically applies a command to in-memory state.
///
/// # Errors
///
/// Returns a command rejection or event application failure. State is unchanged
/// for every failure, including a later failure in a multi-event decision.
pub fn execute(
    state: &mut SessionState,
    request: &CommandRequest,
    activity: Option<&dyn ActivityEngine>,
) -> Result<Decision, RuntimeError> {
    let decision = decide(state, request, activity)?;
    apply_decision_atomically(state, &decision)?;
    Ok(decision)
}

/// Applies every event on a candidate state and commits only on full success.
///
/// # Errors
///
/// Returns the first event application error without modifying the input state.
pub fn apply_decision_atomically(
    state: &mut SessionState,
    decision: &Decision,
) -> Result<(), ApplyError> {
    let mut candidate = state.clone();
    for event in &decision.events {
        candidate.apply(event)?;
    }
    *state = candidate;
    Ok(())
}

/// Reconstructs state by applying an ordered event suffix.
///
/// # Errors
///
/// Returns the first event application error, including sequence gaps.
pub fn replay(initial: SessionState, events: &[SessionEvent]) -> Result<SessionState, ApplyError> {
    events.iter().try_fold(initial, |mut state, event| {
        state.apply(event)?;
        Ok(state)
    })
}

/// Validates and replays a durable V0 log on its declared base state.
///
/// # Errors
///
/// Returns an error for an invalid log, wrong session/base revision, or an
/// invalid event.
pub fn replay_log(initial: SessionState, log: &EventLogV0) -> Result<SessionState, ReplayError> {
    log.validate()?;
    if initial.session_id != log.session_id {
        return Err(ReplayError::SessionMismatch);
    }
    if initial.revision != log.base_revision {
        return Err(ReplayError::BaseRevisionMismatch {
            expected: initial.revision,
            actual: log.base_revision,
        });
    }
    replay(initial, &log.events).map_err(ReplayError::Apply)
}

fn require_initialized(state: &SessionState) -> Result<(), Rejection> {
    if state.lifecycle == Lifecycle::Uninitialized {
        Err(Rejection::NotInitialized)
    } else {
        Ok(())
    }
}

fn require_participant(state: &SessionState, participant: &ParticipantId) -> Result<(), Rejection> {
    if state.participants.contains_key(participant) {
        Ok(())
    } else {
        Err(Rejection::UnknownParticipant(participant.0.clone()))
    }
}

fn require_authority(state: &SessionState, participant: &ParticipantId) -> Result<(), Rejection> {
    require_participant(state, participant)?;
    if state.authority.as_ref() == Some(participant) {
        Ok(())
    } else {
        Err(Rejection::NotAuthority)
    }
}

fn required_text(value: &str, field: &'static str) -> Result<String, Rejection> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(Rejection::EmptyField(field))
    } else {
        Ok(trimmed.to_owned())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize, Deserialize)]
#[error("{code}: {detail}")]
pub struct ActivityRejection {
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, Error, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", content = "details", rename_all = "snake_case")]
pub enum Rejection {
    #[error("revision conflict: current is {expected}, command expected {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("session is already initialized")]
    AlreadyInitialized,
    #[error("session is not initialized")]
    NotInitialized,
    #[error("session is not open")]
    SessionNotOpen,
    #[error("participant already joined")]
    ParticipantAlreadyJoined,
    #[error("unknown participant: {0}")]
    UnknownParticipant(String),
    #[error("actor does not hold authority")]
    NotAuthority,
    #[error("actor already holds authority")]
    AuthorityAlreadyHeld,
    #[error("activity is not active")]
    ActivityNotActive,
    #[error("activity descriptor does not match the active activity")]
    ActivityDescriptorMismatch,
    #[error("matching activity engine is unavailable")]
    ActivityEngineUnavailable,
    #[error("activity rejected command: {0}")]
    ActivityRejected(ActivityRejection),
    #[error("{0} must not be empty")]
    EmptyField(&'static str),
    #[error("session revision overflow")]
    RevisionOverflow,
    #[error("activity revision overflow")]
    ActivityRevisionOverflow,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Rejected(#[from] Rejection),
    #[error(transparent)]
    Apply(#[from] ApplyError),
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error(transparent)]
    Protocol(#[from] sytog_protocol::ProtocolError),
    #[error("event log belongs to another session")]
    SessionMismatch,
    #[error("expected base revision {expected}, log declares {actual}")]
    BaseRevisionMismatch { expected: u64, actual: u64 },
    #[error(transparent)]
    Apply(ApplyError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sytog_domain::{ActivityId, MessageId, SessionId};

    struct TestActivity;

    impl ActivityEngine for TestActivity {
        fn descriptor(&self) -> ActivityDescriptor {
            ActivityDescriptor {
                activity_id: ActivityId::from("test.activity"),
                version: "1.0.0".to_owned(),
            }
        }

        fn initial_state(&self) -> Value {
            json!({"count": 0})
        }

        fn decide(
            &self,
            _actor: &ParticipantId,
            _current: &ActiveActivity,
            _command: &ActivityCommandEnvelope,
        ) -> Result<ActivityTransition, ActivityRejection> {
            Ok(ActivityTransition {
                event_type: "incremented".to_owned(),
                payload: json!({"amount": 1}),
                resulting_state: json!({"count": 1}),
            })
        }
    }

    fn request(actor: &str, revision: u64, command: Command) -> CommandRequest {
        CommandRequest {
            message_id: MessageId::from("test-message"),
            actor: ParticipantId::from(actor),
            expected_revision: revision,
            command,
        }
    }

    fn session(command: SessionCommand) -> Command {
        Command::Session(command)
    }

    fn initialized() -> (SessionState, Vec<SessionEvent>) {
        let mut state = SessionState::uninitialized(SessionId::from("test"));
        let decision = execute(
            &mut state,
            &request(
                "alice",
                0,
                session(SessionCommand::CreateSession {
                    display_name: "Alice".to_owned(),
                }),
            ),
            None,
        )
        .expect("fixture command is valid");
        (state, decision.events)
    }

    #[test]
    fn rejected_command_does_not_change_state() {
        let (mut state, _) = initialized();
        let before = state.clone();
        let result = execute(
            &mut state,
            &request(
                "mallory",
                1,
                session(SessionCommand::StartActivity {
                    descriptor: TestActivity.descriptor(),
                }),
            ),
            Some(&TestActivity),
        );
        assert!(result.is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn replay_reconstructs_exact_state() {
        let (mut state, mut log) = initialized();
        for command in [
            request(
                "bob",
                1,
                session(SessionCommand::Join {
                    display_name: "Bob".to_owned(),
                }),
            ),
            request(
                "alice",
                2,
                session(SessionCommand::StartActivity {
                    descriptor: TestActivity.descriptor(),
                }),
            ),
            request(
                "bob",
                3,
                Command::Activity(ActivityCommandEnvelope {
                    descriptor: TestActivity.descriptor(),
                    command_type: "increment".to_owned(),
                    payload: json!({"amount": 1}),
                }),
            ),
        ] {
            log.extend(
                execute(&mut state, &command, Some(&TestActivity))
                    .expect("fixture command is valid")
                    .events,
            );
        }
        let replayed = replay(SessionState::uninitialized(SessionId::from("test")), &log)
            .expect("valid log replays");
        assert_eq!(replayed, state);
    }

    #[test]
    fn multi_event_application_is_atomic() {
        let (mut state, _) = initialized();
        let before = state.clone();
        let decision = Decision {
            events: vec![
                SessionEvent {
                    event_id: EventId::from("join:0"),
                    sequence: 2,
                    causation_id: MessageId::from("join"),
                    actor: ParticipantId::from("bob"),
                    kind: EventKind::Session(SessionEventKind::ParticipantJoined {
                        participant: ParticipantId::from("bob"),
                        display_name: "Bob".to_owned(),
                    }),
                },
                SessionEvent {
                    event_id: EventId::from("gap:0"),
                    sequence: 4,
                    causation_id: MessageId::from("gap"),
                    actor: ParticipantId::from("alice"),
                    kind: EventKind::Session(SessionEventKind::AuthorityTransferred {
                        from: ParticipantId::from("alice"),
                        to: ParticipantId::from("bob"),
                    }),
                },
            ],
            effects: Vec::new(),
        };
        assert!(apply_decision_atomically(&mut state, &decision).is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn replay_log_rejects_the_wrong_session() {
        let (state, events) = initialized();
        let log = EventLogV0 {
            family: sytog_protocol::PROTOCOL_FAMILY.to_owned(),
            protocol_version: sytog_protocol::PROTOCOL_VERSION,
            schema_version: sytog_protocol::EVENT_LOG_SCHEMA_VERSION,
            session_id: SessionId::from("other"),
            base_revision: 0,
            events,
        };
        assert!(matches!(
            replay_log(SessionState::uninitialized(SessionId::from("test")), &log,),
            Err(ReplayError::SessionMismatch)
        ));
        assert_eq!(state.revision, 1);
    }
}
