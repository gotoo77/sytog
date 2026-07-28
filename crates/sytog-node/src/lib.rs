//! Authoritative local-network node with JSONL durability.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sytog_domain::{
    ActivityCommandEnvelope, Command, CommandRequest, MessageId, ParticipantId, SessionCommand,
    SessionEvent, SessionId, SessionState,
};
use sytog_protocol::{
    EVENT_LOG_SCHEMA_VERSION, EventLogV0, PROTOCOL_FAMILY, PROTOCOL_VERSION,
    SNAPSHOT_SCHEMA_VERSION, SnapshotV0,
};
use sytog_runtime::{ActivityEngine, Rejection, apply_decision_atomically, decide, replay_log};
use sytog_transport::{NetworkMessage, TransportError, decode, envelope, receive, send};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader as AsyncBufReader},
    net::{TcpListener, TcpStream},
    sync::{Mutex, broadcast},
};
use tokio_tungstenite::accept_async;

const HOST_ID: &str = "host";
const ACCEPTED_BATCH_SCHEMA_VERSION: u16 = 1;
const CLIENT_REPLICA_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceivedEvent {
    Applied,
    AlreadySeen,
    Gap,
}

#[derive(Clone, Debug)]
struct ClientReplica {
    state: SessionState,
    history_base_revision: u64,
    events: BTreeMap<u64, SessionEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ClientReplicaV1 {
    schema_version: u16,
    history_base_revision: u64,
    snapshot: SnapshotV0,
    events: Vec<SessionEvent>,
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind: String,
    pub data_dir: PathBuf,
    pub session_id: SessionId,
}

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub url: String,
    pub data_dir: PathBuf,
    pub session_id: SessionId,
    pub participant_id: ParticipantId,
}

#[derive(Debug)]
struct CanonicalSession {
    state: SessionState,
    events: Vec<SessionEvent>,
    accepted_commands: BTreeMap<MessageId, AcceptedCommandV1>,
}

struct Host {
    session_id: SessionId,
    activity: Arc<dyn ActivityEngine + Send + Sync>,
    journal: JournalStore,
    canonical: Mutex<CanonicalSession>,
    events: broadcast::Sender<Vec<SessionEvent>>,
}

#[derive(Clone, Debug)]
struct JournalStore {
    directory: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionMetadata {
    family: String,
    protocol_version: u16,
    session_id: SessionId,
    authority_id: ParticipantId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AcceptedCommandV1 {
    request: CommandRequest,
    events: Vec<SessionEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AcceptedBatchV1 {
    record_type: String,
    schema_version: u16,
    commands: Vec<AcceptedCommandV1>,
}

#[derive(Debug)]
struct LoadedJournal {
    events: Vec<SessionEvent>,
    accepted_commands: BTreeMap<MessageId, AcceptedCommandV1>,
    recovery: Option<JournalRecovery>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JournalRecovery {
    safe_offset: u64,
    original_length: u64,
}

#[derive(Clone, Debug)]
struct Submission {
    events: Vec<SessionEvent>,
    duplicate: bool,
}

/// Runs an authoritative WebSocket host until Ctrl-C.
///
/// # Errors
///
/// Returns a bind, persistence, bootstrap, or listener failure.
pub async fn serve(
    config: ServerConfig,
    activity: Arc<dyn ActivityEngine + Send + Sync>,
) -> Result<(), NodeError> {
    let listener = TcpListener::bind(&config.bind).await?;
    let address = listener.local_addr()?;
    let host = Arc::new(Host::load_or_create(&config, activity)?);
    println!(
        "SYTOG host listening on ws://{address} (session {})",
        config.session_id.0
    );

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let host = Arc::clone(&host);
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, peer, host).await {
                        eprintln!("connection {peer} closed: {error}");
                    }
                });
            }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                println!("SYTOG host stopped");
                return Ok(());
            }
        }
    }
}

/// Runs an interactive client and persists its reduced local state.
///
/// `parse_command` is supplied by the activity adapter; the node only handles
/// the generic `state` and `quit` commands itself.
///
/// # Errors
///
/// Returns a connection, protocol, local persistence, or reducer failure.
// Keeping the stdin/WebSocket select loop together makes the client state
// machine and its ordering visible in one place.
#[allow(clippy::too_many_lines)]
pub async fn connect_client(
    config: ClientConfig,
    parse_command: fn(&str) -> Result<ActivityCommandEnvelope, String>,
) -> Result<(), NodeError> {
    let socket = sytog_transport::connect(&config.url).await?;
    let (mut sink, mut stream) = socket.split();
    let mut local = load_client_replica(&config)?;
    let mut outgoing_counter = 0_u64;

    send_client_message(
        &mut sink,
        &config,
        next_message_id(&config.participant_id, &mut outgoing_counter),
        local.state.revision,
        &NetworkMessage::Hello {
            last_sequence: local.state.revision,
        },
    )
    .await?;
    send_client_message(
        &mut sink,
        &config,
        next_message_id(&config.participant_id, &mut outgoing_counter),
        local.state.revision,
        &NetworkMessage::JoinSession {
            display_name: config.participant_id.0.clone(),
        },
    )
    .await?;

    println!(
        "connected as {} at local revision {}",
        config.participant_id.0, local.state.revision
    );
    println!("commands: activity command | state | quit");
    let stdin = AsyncBufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else {
                    sink.close().await?;
                    return Ok(());
                };
                let line = line.trim();
                if line == "quit" {
                    sink.close().await?;
                    return Ok(());
                }
                if line == "state" {
                    println!("{}", serde_json::to_string_pretty(&local.state)?);
                    continue;
                }
                match parse_command(line) {
                    Ok(command) => {
                        let message_id = MessageId(format!(
                            "{}:command:{}",
                            config.participant_id.0, local.state.revision
                        ));
                        let request = CommandRequest {
                            message_id: message_id.clone(),
                            actor: config.participant_id.clone(),
                            expected_revision: local.state.revision,
                            command: Command::Activity(command),
                        };
                        send_client_message(
                            &mut sink,
                            &config,
                            message_id,
                            local.state.revision,
                            &NetworkMessage::SubmitCommand { request },
                        )
                        .await?;
                    }
                    Err(error) => eprintln!("{error}"),
                }
            }
            incoming = receive(&mut stream) => {
                let Some(incoming) = incoming? else { return Ok(()); };
                ensure_session(&incoming.session_id, &config.session_id)?;
                match decode(&incoming)? {
                    NetworkMessage::EventBatch { events, .. } => {
                        let mut gap = false;
                        for event in events {
                            match local.apply_received_event(&event)? {
                                ReceivedEvent::Applied => {
                                    println!(
                                        "event {} sequence={} revision={}",
                                        event.event_id.0, event.sequence, local.state.revision
                                    );
                                }
                                ReceivedEvent::AlreadySeen => {}
                                ReceivedEvent::Gap => {
                                    gap = true;
                                    break;
                                }
                            }
                        }
                        save_client_replica(&config, &local)?;
                        if gap {
                            send_client_message(
                                &mut sink,
                                &config,
                                next_message_id(&config.participant_id, &mut outgoing_counter),
                                local.state.revision,
                                &NetworkMessage::CatchUpRequest {
                                    after_sequence: local.state.revision,
                                },
                            )
                            .await?;
                        }
                    }
                    NetworkMessage::StateSnapshot { snapshot } => {
                        snapshot.validate()?;
                        ensure_session(&snapshot.session_id, &config.session_id)?;
                        local = ClientReplica::from_snapshot(snapshot);
                        save_client_replica(&config, &local)?;
                        println!("snapshot applied at revision {}", local.state.revision);
                    }
                    NetworkMessage::CommandRejected {
                        code,
                        detail,
                        current_revision,
                        ..
                    } => {
                        eprintln!(
                            "command rejected: code={code} detail={detail} current_revision={current_revision}"
                        );
                        if current_revision > local.state.revision {
                            send_client_message(
                                &mut sink,
                                &config,
                                next_message_id(&config.participant_id, &mut outgoing_counter),
                                local.state.revision,
                                &NetworkMessage::CatchUpRequest {
                                    after_sequence: local.state.revision,
                                },
                            )
                            .await?;
                        }
                    }
                    NetworkMessage::Hello { .. }
                    | NetworkMessage::JoinSession { .. }
                    | NetworkMessage::SubmitCommand { .. }
                    | NetworkMessage::CatchUpRequest { .. } => {
                        return Err(NodeError::UnexpectedClientMessage);
                    }
                }
            }
        }
    }
}

// This is the transport adapter's complete per-connection state machine.
#[allow(clippy::too_many_lines)]
async fn handle_connection(
    stream: TcpStream,
    _peer: SocketAddr,
    host: Arc<Host>,
) -> Result<(), NodeError> {
    let socket = accept_async(stream).await?;
    let (mut sink, mut stream) = socket.split();
    let mut subscription = host.events.subscribe();
    let mut participant: Option<ParticipantId> = None;
    let mut sent_sequence = 0_u64;

    loop {
        tokio::select! {
            incoming = receive(&mut stream) => {
                let Some(incoming) = incoming? else { return Ok(()); };
                ensure_session(&incoming.session_id, &host.session_id)?;
                let sender = incoming.sender_id.clone();
                let message_id = incoming.message_id.clone();
                match decode(&incoming)? {
                    NetworkMessage::Hello { last_sequence } => {
                        sent_sequence = last_sequence;
                        let batch = host.events_after(last_sequence).await;
                        if !batch.is_empty() {
                            sent_sequence = batch.last().map_or(sent_sequence, |event| event.sequence);
                            send_host_message(
                                &mut sink,
                                &host.session_id,
                                &NetworkMessage::EventBatch {
                                    from_sequence: last_sequence + 1,
                                    events: batch,
                                },
                                sent_sequence,
                            ).await?;
                        }
                    }
                    NetworkMessage::JoinSession { display_name } => {
                        participant = Some(sender.clone());
                        if let Err(rejection) =
                            host.join(sender, display_name, message_id.clone()).await
                        {
                            send_rejection(
                                &mut sink,
                                &host,
                                message_id,
                                rejection,
                            ).await?;
                        }
                    }
                    NetworkMessage::SubmitCommand { request } => {
                        if participant.as_ref() != Some(&sender) || request.actor != sender {
                            send_rejection(
                                &mut sink,
                                &host,
                                message_id,
                                HostRejection::new(
                                    "sender_mismatch",
                                    "connection participant and command actor differ",
                                    host.current_revision().await,
                                ),
                            ).await?;
                            continue;
                        }
                        match host.submit(request).await {
                            Ok(submission) if submission.duplicate => {
                                if let (Some(first), Some(last)) =
                                    (submission.events.first(), submission.events.last())
                                {
                                    let from_sequence = first.sequence;
                                    let revision = last.sequence;
                                    send_host_message(
                                        &mut sink,
                                        &host.session_id,
                                        &NetworkMessage::EventBatch {
                                            from_sequence,
                                            events: submission.events,
                                        },
                                        revision,
                                    )
                                    .await?;
                                }
                            }
                            Ok(_) => {}
                            Err(rejection) => {
                                send_rejection(&mut sink, &host, message_id, rejection).await?;
                            }
                        }
                    }
                    NetworkMessage::CatchUpRequest { after_sequence } => {
                        let batch = host.events_after(after_sequence).await;
                        if !batch.is_empty() {
                            sent_sequence = batch.last().map_or(sent_sequence, |event| event.sequence);
                            send_host_message(
                                &mut sink,
                                &host.session_id,
                                &NetworkMessage::EventBatch {
                                    from_sequence: after_sequence + 1,
                                    events: batch,
                                },
                                sent_sequence,
                            ).await?;
                        }
                    }
                    NetworkMessage::CommandRejected { .. }
                    | NetworkMessage::EventBatch { .. }
                    | NetworkMessage::StateSnapshot { .. } => {
                        return Err(NodeError::UnexpectedHostMessage);
                    }
                }
            }
            broadcast = subscription.recv() => {
                let events = match broadcast {
                    Ok(events) => events,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        host.events_after(sent_sequence).await
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                };
                let events: Vec<_> = events
                    .into_iter()
                    .filter(|event| event.sequence > sent_sequence)
                    .collect();
                if events.is_empty() {
                    continue;
                }
                let from_sequence = events[0].sequence;
                sent_sequence = events.last().map_or(sent_sequence, |event| event.sequence);
                send_host_message(
                    &mut sink,
                    &host.session_id,
                    &NetworkMessage::EventBatch {
                        from_sequence,
                        events,
                    },
                    sent_sequence,
                ).await?;
            }
        }
    }
}

impl Host {
    fn load_or_create(
        config: &ServerConfig,
        activity: Arc<dyn ActivityEngine + Send + Sync>,
    ) -> Result<Self, NodeError> {
        let journal = JournalStore::new(&config.data_dir, &config.session_id);
        let loaded = journal.load()?;
        let (state, events, accepted_commands) = if loaded.events.is_empty() {
            if let Some(recovery) = &loaded.recovery {
                journal.apply_recovery(recovery)?;
            }
            let mut state = SessionState::uninitialized(config.session_id.clone());
            let request = CommandRequest {
                message_id: MessageId::from("bootstrap"),
                actor: ParticipantId::from(HOST_ID),
                expected_revision: 0,
                command: Command::Session(SessionCommand::CreateSession {
                    display_name: "SYTOG Host".to_owned(),
                }),
            };
            let decision = sytog_runtime::execute(&mut state, &request, None)?;
            let accepted = AcceptedCommandV1 {
                request,
                events: decision.events.clone(),
            };
            journal.append_accepted(&[accepted.clone()])?;
            journal.write_metadata()?;
            journal.write_snapshot(&state)?;
            (
                state,
                decision.events,
                BTreeMap::from([(accepted.request.message_id.clone(), accepted)]),
            )
        } else {
            let log = EventLogV0 {
                family: PROTOCOL_FAMILY.to_owned(),
                protocol_version: PROTOCOL_VERSION,
                schema_version: EVENT_LOG_SCHEMA_VERSION,
                session_id: config.session_id.clone(),
                base_revision: 0,
                events: loaded.events.clone(),
            };
            let state = replay_log(SessionState::uninitialized(config.session_id.clone()), &log)?;
            if let Some(recovery) = &loaded.recovery {
                journal.apply_recovery(recovery)?;
            }
            (state, loaded.events, loaded.accepted_commands)
        };
        let (sender, _) = broadcast::channel(256);
        Ok(Self {
            session_id: config.session_id.clone(),
            activity,
            journal,
            canonical: Mutex::new(CanonicalSession {
                state,
                events,
                accepted_commands,
            }),
            events: sender,
        })
    }

    async fn join(
        &self,
        participant: ParticipantId,
        display_name: String,
        message_id: MessageId,
    ) -> Result<(), HostRejection> {
        let mut canonical = self.canonical.lock().await;
        if canonical.state.participants.contains_key(&participant) {
            return Ok(());
        }
        let request = CommandRequest {
            message_id,
            actor: participant,
            expected_revision: canonical.state.revision,
            command: Command::Session(SessionCommand::Join { display_name }),
        };
        self.accept(&mut canonical, &request, None).map(|_| ())
    }

    async fn submit(&self, request: CommandRequest) -> Result<Submission, HostRejection> {
        let mut canonical = self.canonical.lock().await;
        if let Some(accepted) = canonical.accepted_commands.get(&request.message_id) {
            if accepted.request == request {
                return Ok(Submission {
                    events: accepted.events.clone(),
                    duplicate: true,
                });
            }
            return Err(HostRejection::new(
                "command_id_collision",
                format!(
                    "message_id {} was already accepted with different command content",
                    request.message_id.0
                ),
                canonical.state.revision,
            ));
        }
        if request.expected_revision != canonical.state.revision {
            let rejection = Rejection::RevisionConflict {
                expected: canonical.state.revision,
                actual: request.expected_revision,
            };
            return Err(HostRejection::from_runtime(
                &rejection,
                canonical.state.revision,
            ));
        }

        if matches!(request.command, Command::Activity(_)) && canonical.state.activity.is_none() {
            let original_request = request.clone();
            let start = CommandRequest {
                message_id: MessageId(format!("auto-start:{}", request.message_id.0)),
                actor: ParticipantId::from(HOST_ID),
                expected_revision: canonical.state.revision,
                command: Command::Session(SessionCommand::StartActivity {
                    descriptor: self.activity.descriptor(),
                }),
            };
            let mut candidate = canonical.state.clone();
            let start_decision = decide(&candidate, &start, Some(self.activity.as_ref()))
                .map_err(|error| HostRejection::from_runtime(&error, canonical.state.revision))?;
            apply_decision_atomically(&mut candidate, &start_decision).map_err(|error| {
                HostRejection::new("apply_failed", error.to_string(), canonical.state.revision)
            })?;

            let mut adjusted = request;
            adjusted.expected_revision = candidate.revision;
            let activity_decision = decide(&candidate, &adjusted, Some(self.activity.as_ref()))
                .map_err(|error| HostRejection::from_runtime(&error, canonical.state.revision))?;
            apply_decision_atomically(&mut candidate, &activity_decision).map_err(|error| {
                HostRejection::new("apply_failed", error.to_string(), canonical.state.revision)
            })?;
            let activity_accepted = AcceptedCommandV1 {
                request: original_request,
                events: [start_decision.events, activity_decision.events].concat(),
            };
            return self.commit(&mut canonical, candidate, vec![activity_accepted]);
        }
        self.accept(&mut canonical, &request, Some(self.activity.as_ref()))
    }

    fn accept(
        &self,
        canonical: &mut CanonicalSession,
        request: &CommandRequest,
        activity: Option<&dyn ActivityEngine>,
    ) -> Result<Submission, HostRejection> {
        let decision = decide(&canonical.state, request, activity)
            .map_err(|error| HostRejection::from_runtime(&error, canonical.state.revision))?;
        let mut candidate = canonical.state.clone();
        apply_decision_atomically(&mut candidate, &decision).map_err(|error| {
            HostRejection::new("apply_failed", error.to_string(), canonical.state.revision)
        })?;
        self.commit(
            canonical,
            candidate,
            vec![AcceptedCommandV1 {
                request: request.clone(),
                events: decision.events,
            }],
        )
    }

    fn commit(
        &self,
        canonical: &mut CanonicalSession,
        candidate: SessionState,
        accepted_commands: Vec<AcceptedCommandV1>,
    ) -> Result<Submission, HostRejection> {
        for accepted in &accepted_commands {
            if canonical
                .accepted_commands
                .contains_key(&accepted.request.message_id)
            {
                return Err(HostRejection::new(
                    "command_id_collision",
                    format!(
                        "message_id {} already exists in the accepted-command index",
                        accepted.request.message_id.0
                    ),
                    canonical.state.revision,
                ));
            }
        }
        let events: Vec<_> = accepted_commands
            .iter()
            .flat_map(|accepted| accepted.events.iter().cloned())
            .collect();
        let mut prospective_events = canonical.events.clone();
        prospective_events.extend(events.clone());
        EventLogV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: EVENT_LOG_SCHEMA_VERSION,
            session_id: self.session_id.clone(),
            base_revision: 0,
            events: prospective_events,
        }
        .validate()
        .map_err(|error| {
            HostRejection::new(
                "journal_invariant_failed",
                error.to_string(),
                canonical.state.revision,
            )
        })?;
        self.journal
            .append_accepted(&accepted_commands)
            .map_err(|error| {
                HostRejection::new(
                    "persistence_failed",
                    error.to_string(),
                    canonical.state.revision,
                )
            })?;
        canonical.state = candidate;
        canonical.events.extend(events.clone());
        for accepted in accepted_commands {
            canonical
                .accepted_commands
                .insert(accepted.request.message_id.clone(), accepted);
        }
        if let Err(error) = self.journal.write_snapshot(&canonical.state) {
            eprintln!("snapshot update failed after durable journal commit: {error}");
        }
        let _ = self.events.send(events.clone());
        Ok(Submission {
            events,
            duplicate: false,
        })
    }

    async fn events_after(&self, sequence: u64) -> Vec<SessionEvent> {
        self.canonical
            .lock()
            .await
            .events
            .iter()
            .filter(|event| event.sequence > sequence)
            .cloned()
            .collect()
    }

    async fn current_revision(&self) -> u64 {
        self.canonical.lock().await.state.revision
    }
}

impl JournalStore {
    fn new(data_dir: &Path, session_id: &SessionId) -> Self {
        Self {
            directory: data_dir.join("sessions").join(&session_id.0),
        }
    }

    fn events_path(&self) -> PathBuf {
        self.directory.join("events.jsonl")
    }

    fn load(&self) -> Result<LoadedJournal, NodeError> {
        let path = self.events_path();
        if !path.exists() {
            return Ok(LoadedJournal {
                events: Vec::new(),
                accepted_commands: BTreeMap::new(),
                recovery: None,
            });
        }
        let bytes = fs::read(&path)?;
        let original_length =
            u64::try_from(bytes.len()).map_err(|_| NodeError::JournalOffsetOverflow)?;
        let (complete, recovery) = if bytes.last().is_some_and(|byte| *byte != b'\n') {
            let safe_length = bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |offset| offset + 1);
            (
                &bytes[..safe_length],
                Some(JournalRecovery {
                    safe_offset: u64::try_from(safe_length)
                        .map_err(|_| NodeError::JournalOffsetOverflow)?,
                    original_length,
                }),
            )
        } else {
            (bytes.as_slice(), None)
        };
        let mut events = Vec::new();
        let mut accepted_commands = BTreeMap::new();
        let mut line_offset = 0_usize;
        for line in complete.split_inclusive(|byte| *byte == b'\n') {
            let content = &line[..line.len().saturating_sub(1)];
            let current_offset =
                u64::try_from(line_offset).map_err(|_| NodeError::JournalOffsetOverflow)?;
            line_offset = line_offset
                .checked_add(line.len())
                .ok_or(NodeError::JournalOffsetOverflow)?;
            if content.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value: Value = serde_json::from_slice(content).map_err(|source| {
                NodeError::InvalidJournalLine {
                    offset: current_offset,
                    source,
                }
            })?;
            if value.get("record_type").is_some() {
                let batch: AcceptedBatchV1 = serde_json::from_value(value).map_err(|source| {
                    NodeError::InvalidJournalLine {
                        offset: current_offset,
                        source,
                    }
                })?;
                if batch.record_type != "accepted_commands" {
                    return Err(NodeError::UnknownJournalRecord(batch.record_type));
                }
                if batch.schema_version != ACCEPTED_BATCH_SCHEMA_VERSION {
                    return Err(NodeError::UnsupportedAcceptedBatchSchema(
                        batch.schema_version,
                    ));
                }
                if batch.commands.is_empty() {
                    return Err(NodeError::EmptyAcceptedBatch);
                }
                for accepted in batch.commands {
                    let message_id = accepted.request.message_id.clone();
                    if accepted.events.is_empty() {
                        return Err(NodeError::AcceptedCommandWithoutEvents(message_id.0));
                    }
                    if !accepted
                        .events
                        .iter()
                        .any(|event| event.causation_id == message_id)
                    {
                        return Err(NodeError::AcceptedCommandMissingCausation(message_id.0));
                    }
                    if accepted_commands
                        .insert(message_id.clone(), accepted.clone())
                        .is_some()
                    {
                        return Err(NodeError::DuplicateAcceptedCommand(message_id.0));
                    }
                    events.extend(accepted.events);
                }
            } else {
                events.push(serde_json::from_value(value).map_err(|source| {
                    NodeError::InvalidJournalLine {
                        offset: current_offset,
                        source,
                    }
                })?);
            }
        }
        Ok(LoadedJournal {
            events,
            accepted_commands,
            recovery,
        })
    }

    fn apply_recovery(&self, recovery: &JournalRecovery) -> Result<(), NodeError> {
        let path = self.events_path();
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let actual_length = file.metadata()?.len();
        if actual_length != recovery.original_length {
            return Err(NodeError::JournalChangedDuringRecovery {
                expected: recovery.original_length,
                actual: actual_length,
            });
        }
        file.set_len(recovery.safe_offset)?;
        file.sync_data()?;
        eprintln!(
            "journal recovery: truncated incomplete suffix in {} from byte {} to safe offset {}",
            path.display(),
            recovery.original_length,
            recovery.safe_offset
        );
        Ok(())
    }

    fn append_accepted(&self, commands: &[AcceptedCommandV1]) -> Result<(), NodeError> {
        fs::create_dir_all(&self.directory)?;
        let batch = AcceptedBatchV1 {
            record_type: "accepted_commands".to_owned(),
            schema_version: ACCEPTED_BATCH_SCHEMA_VERSION,
            commands: commands.to_vec(),
        };
        let mut bytes = serde_json::to_vec(&batch)?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path())?;
        file.write_all(&bytes)?;
        file.sync_data()?;
        Ok(())
    }

    fn write_metadata(&self) -> Result<(), NodeError> {
        fs::create_dir_all(&self.directory)?;
        let metadata = SessionMetadata {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            session_id: SessionId(
                self.directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(NodeError::InvalidSessionPath)?
                    .to_owned(),
            ),
            authority_id: ParticipantId::from(HOST_ID),
        };
        write_json_atomically(&self.directory.join("metadata.json"), &metadata)
    }

    fn write_snapshot(&self, state: &SessionState) -> Result<(), NodeError> {
        let snapshot = SnapshotV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            session_id: state.session_id.clone(),
            revision: state.revision,
            state: state.clone(),
        };
        write_json_atomically(&self.directory.join("snapshot.json"), &snapshot)
    }
}

impl ClientReplica {
    fn uninitialized(session_id: SessionId) -> Self {
        Self {
            state: SessionState::uninitialized(session_id),
            history_base_revision: 0,
            events: BTreeMap::new(),
        }
    }

    fn from_snapshot(snapshot: SnapshotV0) -> Self {
        Self {
            history_base_revision: snapshot.revision,
            state: snapshot.state,
            events: BTreeMap::new(),
        }
    }

    fn from_v1(stored: ClientReplicaV1) -> Result<Self, NodeError> {
        if stored.schema_version != CLIENT_REPLICA_SCHEMA_VERSION {
            return Err(NodeError::UnsupportedClientReplicaSchema(
                stored.schema_version,
            ));
        }
        stored.snapshot.validate()?;
        let log = EventLogV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: EVENT_LOG_SCHEMA_VERSION,
            session_id: stored.snapshot.session_id.clone(),
            base_revision: stored.history_base_revision,
            events: stored.events.clone(),
        };
        log.validate()?;
        let expected_revision = stored
            .history_base_revision
            .checked_add(
                u64::try_from(stored.events.len()).map_err(|_| NodeError::ClientHistoryOverflow)?,
            )
            .ok_or(NodeError::ClientHistoryOverflow)?;
        if expected_revision != stored.snapshot.revision {
            return Err(NodeError::ClientHistoryRevisionMismatch {
                expected: stored.snapshot.revision,
                actual: expected_revision,
            });
        }
        Ok(Self {
            state: stored.snapshot.state,
            history_base_revision: stored.history_base_revision,
            events: stored
                .events
                .into_iter()
                .map(|event| (event.sequence, event))
                .collect(),
        })
    }

    fn apply_received_event(&mut self, event: &SessionEvent) -> Result<ReceivedEvent, NodeError> {
        if event.sequence <= self.history_base_revision {
            return Err(NodeError::EventHistoryUnavailable(event.sequence));
        }
        if let Some(known) = self.events.get(&event.sequence) {
            if known == event {
                return Ok(ReceivedEvent::AlreadySeen);
            }
            if known.event_id == event.event_id {
                return Err(NodeError::EventIdCollision {
                    event_id: event.event_id.0.clone(),
                    known_sequence: known.sequence,
                    incoming_sequence: event.sequence,
                });
            }
            return Err(NodeError::EventSequenceCollision {
                sequence: event.sequence,
                known_event_id: known.event_id.0.clone(),
                incoming_event_id: event.event_id.0.clone(),
            });
        }
        if let Some(known) = self
            .events
            .values()
            .find(|known| known.event_id == event.event_id)
        {
            return Err(NodeError::EventIdCollision {
                event_id: event.event_id.0.clone(),
                known_sequence: known.sequence,
                incoming_sequence: event.sequence,
            });
        }
        let expected = self
            .state
            .revision
            .checked_add(1)
            .ok_or(NodeError::ClientHistoryOverflow)?;
        if event.sequence != expected {
            return Ok(ReceivedEvent::Gap);
        }
        self.state.apply(event)?;
        self.events.insert(event.sequence, event.clone());
        Ok(ReceivedEvent::Applied)
    }
}

fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<(), NodeError> {
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn load_client_replica(config: &ClientConfig) -> Result<ClientReplica, NodeError> {
    let path = client_state_path(config);
    if !path.exists() {
        return Ok(ClientReplica::uninitialized(config.session_id.clone()));
    }
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let replica = if value.get("snapshot").is_some() {
        ClientReplica::from_v1(serde_json::from_value(value)?)?
    } else {
        let snapshot: SnapshotV0 = serde_json::from_value(value)?;
        snapshot.validate()?;
        ClientReplica::from_snapshot(snapshot)
    };
    ensure_session(&replica.state.session_id, &config.session_id)?;
    Ok(replica)
}

fn save_client_replica(config: &ClientConfig, replica: &ClientReplica) -> Result<(), NodeError> {
    let path = client_state_path(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let snapshot = SnapshotV0 {
        family: PROTOCOL_FAMILY.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        session_id: replica.state.session_id.clone(),
        revision: replica.state.revision,
        state: replica.state.clone(),
    };
    let stored = ClientReplicaV1 {
        schema_version: CLIENT_REPLICA_SCHEMA_VERSION,
        history_base_revision: replica.history_base_revision,
        snapshot,
        events: replica.events.values().cloned().collect(),
    };
    write_json_atomically(&path, &stored)
}

fn client_state_path(config: &ClientConfig) -> PathBuf {
    config.data_dir.join(format!(
        "{}-{}.json",
        config.session_id.0, config.participant_id.0
    ))
}

fn next_message_id(participant: &ParticipantId, counter: &mut u64) -> MessageId {
    *counter += 1;
    MessageId(format!("{}-{}", participant.0, *counter))
}

async fn send_client_message<S>(
    sink: &mut S,
    config: &ClientConfig,
    message_id: MessageId,
    revision: u64,
    message: &NetworkMessage,
) -> Result<(), NodeError>
where
    S: futures_util::Sink<
            tokio_tungstenite::tungstenite::Message,
            Error = tokio_tungstenite::tungstenite::Error,
        > + Unpin,
{
    send(
        sink,
        &envelope(
            config.session_id.clone(),
            config.participant_id.clone(),
            message_id,
            Some(revision),
            message,
        )?,
    )
    .await?;
    Ok(())
}

async fn send_host_message<S>(
    sink: &mut S,
    session_id: &SessionId,
    message: &NetworkMessage,
    revision: u64,
) -> Result<(), NodeError>
where
    S: futures_util::Sink<
            tokio_tungstenite::tungstenite::Message,
            Error = tokio_tungstenite::tungstenite::Error,
        > + Unpin,
{
    send(
        sink,
        &envelope(
            session_id.clone(),
            ParticipantId::from(HOST_ID),
            MessageId(format!("host-{revision}-{}", message.message_type())),
            Some(revision),
            message,
        )?,
    )
    .await?;
    Ok(())
}

async fn send_rejection<S>(
    sink: &mut S,
    host: &Host,
    message_id: MessageId,
    rejection: HostRejection,
) -> Result<(), NodeError>
where
    S: futures_util::Sink<
            tokio_tungstenite::tungstenite::Message,
            Error = tokio_tungstenite::tungstenite::Error,
        > + Unpin,
{
    send_host_message(
        sink,
        &host.session_id,
        &NetworkMessage::CommandRejected {
            message_id,
            code: rejection.code,
            detail: rejection.detail,
            current_revision: rejection.current_revision,
        },
        rejection.current_revision,
    )
    .await
}

fn ensure_session(actual: &SessionId, expected: &SessionId) -> Result<(), NodeError> {
    if actual == expected {
        Ok(())
    } else {
        Err(NodeError::Transport(TransportError::SessionMismatch {
            expected: expected.0.clone(),
            actual: actual.0.clone(),
        }))
    }
}

#[derive(Clone, Debug)]
struct HostRejection {
    code: String,
    detail: String,
    current_revision: u64,
}

impl HostRejection {
    fn new(code: &str, detail: impl Into<String>, current_revision: u64) -> Self {
        Self {
            code: code.to_owned(),
            detail: detail.into(),
            current_revision,
        }
    }

    fn from_runtime(rejection: &Rejection, current_revision: u64) -> Self {
        let serialized = serde_json::to_value(rejection).unwrap_or_default();
        let code = serialized
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("runtime_rejection")
            .to_owned();
        Self::new(&code, rejection.to_string(), current_revision)
    }
}

#[derive(Debug, Error)]
pub enum NodeError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error(transparent)]
    Runtime(#[from] sytog_runtime::RuntimeError),
    #[error(transparent)]
    Replay(#[from] sytog_runtime::ReplayError),
    #[error(transparent)]
    Apply(#[from] sytog_domain::ApplyError),
    #[error(transparent)]
    Protocol(#[from] sytog_protocol::ProtocolError),
    #[error("journal byte offset overflow")]
    JournalOffsetOverflow,
    #[error("invalid journal line at byte offset {offset}: {source}")]
    InvalidJournalLine {
        offset: u64,
        #[source]
        source: serde_json::Error,
    },
    #[error("journal changed during recovery: expected length {expected}, actual length {actual}")]
    JournalChangedDuringRecovery { expected: u64, actual: u64 },
    #[error("invalid session directory")]
    InvalidSessionPath,
    #[error("unsupported accepted-command batch schema version: {0}")]
    UnsupportedAcceptedBatchSchema(u16),
    #[error("unknown journal record type: {0}")]
    UnknownJournalRecord(String),
    #[error("accepted-command batch must not be empty")]
    EmptyAcceptedBatch,
    #[error("accepted command {0} has no events")]
    AcceptedCommandWithoutEvents(String),
    #[error("accepted command {0} has no event with matching causation")]
    AcceptedCommandMissingCausation(String),
    #[error("duplicate accepted command in journal: {0}")]
    DuplicateAcceptedCommand(String),
    #[error("unsupported client replica schema version: {0}")]
    UnsupportedClientReplicaSchema(u16),
    #[error("client event history length overflow")]
    ClientHistoryOverflow,
    #[error("client history ends at revision {actual}, snapshot is at {expected}")]
    ClientHistoryRevisionMismatch { expected: u64, actual: u64 },
    #[error("cannot verify old event at sequence {0}: client history is unavailable")]
    EventHistoryUnavailable(u64),
    #[error(
        "event sequence collision at {sequence}: known={known_event_id}, incoming={incoming_event_id}"
    )]
    EventSequenceCollision {
        sequence: u64,
        known_event_id: String,
        incoming_event_id: String,
    },
    #[error(
        "event id collision for {event_id}: known sequence={known_sequence}, incoming sequence={incoming_sequence}"
    )]
    EventIdCollision {
        event_id: String,
        known_sequence: u64,
        incoming_sequence: u64,
    },
    #[error("client sent a host-only network message")]
    UnexpectedClientMessage,
    #[error("host sent a client-only network message")]
    UnexpectedHostMessage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashSet,
        sync::atomic::{AtomicBool, Ordering},
        thread,
        time::Duration,
    };
    use sytog_demo_vote::VoteActivity;
    use sytog_demo_vote::VoteState;
    use sytog_domain::{ActiveActivity, ActivityDescriptor, EventId, EventKind, SessionEventKind};
    use sytog_runtime::{ActivityRejection, ActivityTransition};

    const ASYNC_TEST_TIMEOUT: Duration = Duration::from_secs(10);

    fn temporary_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sytog-{name}-{}", std::process::id()))
    }

    fn created_event(display_name: &str) -> SessionEvent {
        SessionEvent {
            event_id: EventId::from("create:0"),
            sequence: 1,
            causation_id: MessageId::from("create"),
            actor: ParticipantId::from(HOST_ID),
            kind: EventKind::Session(SessionEventKind::SessionCreated {
                creator: ParticipantId::from(HOST_ID),
                display_name: display_name.to_owned(),
            }),
        }
    }

    fn server_config(name: &str) -> (PathBuf, ServerConfig) {
        let directory = temporary_directory(name);
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from(name),
        };
        (directory, config)
    }

    async fn create_versioned_vote(
        name: &str,
        include_vote: bool,
    ) -> (PathBuf, ServerConfig, CommandRequest) {
        let (directory, config) = server_config(name);
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("join-alice"),
        )
        .await
        .expect("Alice joins");
        let open = CommandRequest {
            message_id: MessageId::from("open-vote"),
            actor: ParticipantId::from("alice"),
            expected_revision: 2,
            command: Command::Activity(VoteActivity::open(&["tea", "coffee"])),
        };
        host.submit(open.clone()).await.expect("vote opens");
        if include_vote {
            host.submit(CommandRequest {
                message_id: MessageId::from("vote-once"),
                actor: ParticipantId::from("alice"),
                expected_revision: 4,
                command: Command::Activity(VoteActivity::submit("tea")),
            })
            .await
            .expect("vote is accepted");
        }
        drop(host);
        (directory, config, open)
    }

    fn journal_path(config: &ServerConfig) -> PathBuf {
        JournalStore::new(&config.data_dir, &config.session_id).events_path()
    }

    fn last_record_start(bytes: &[u8]) -> usize {
        assert_eq!(bytes.last(), Some(&b'\n'), "journal ends with newline");
        bytes[..bytes.len() - 1]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |offset| offset + 1)
    }

    fn write_legacy_two_event_journal(config: &ServerConfig) -> usize {
        let journal = JournalStore::new(&config.data_dir, &config.session_id);
        fs::create_dir_all(&journal.directory).expect("journal directory is created");
        let mut state = SessionState::uninitialized(config.session_id.clone());
        let create = CommandRequest {
            message_id: MessageId::from("legacy-create"),
            actor: ParticipantId::from(HOST_ID),
            expected_revision: 0,
            command: Command::Session(SessionCommand::CreateSession {
                display_name: "Legacy Host".to_owned(),
            }),
        };
        let first =
            sytog_runtime::execute(&mut state, &create, None).expect("legacy create succeeds");
        let join = CommandRequest {
            message_id: MessageId::from("legacy-join"),
            actor: ParticipantId::from("alice"),
            expected_revision: 1,
            command: Command::Session(SessionCommand::Join {
                display_name: "Alice".to_owned(),
            }),
        };
        let second = sytog_runtime::execute(&mut state, &join, None).expect("legacy join succeeds");
        let mut bytes = serde_json::to_vec(&first.events[0]).expect("first event serializes");
        bytes.push(b'\n');
        let safe_offset = bytes.len();
        bytes.extend(serde_json::to_vec(&second.events[0]).expect("second event serializes"));
        bytes.push(b'\n');
        fs::write(journal.events_path(), bytes).expect("legacy journal is written");
        safe_offset
    }

    struct DelayedVoteActivity {
        entered_slow_decision: Arc<AtomicBool>,
        delay: Duration,
    }

    impl ActivityEngine for DelayedVoteActivity {
        fn descriptor(&self) -> ActivityDescriptor {
            VoteActivity::descriptor()
        }

        fn initial_state(&self) -> Value {
            VoteActivity.initial_state()
        }

        fn decide(
            &self,
            actor: &ParticipantId,
            current: &ActiveActivity,
            command: &ActivityCommandEnvelope,
        ) -> Result<ActivityTransition, ActivityRejection> {
            if command.payload.get("choice").and_then(Value::as_str) == Some("slow") {
                self.entered_slow_decision.store(true, Ordering::Release);
                thread::sleep(self.delay);
            }
            VoteActivity.decide(actor, current, command)
        }
    }

    fn versioned_receipts(path: &Path) -> Vec<AcceptedCommandV1> {
        let bytes = fs::read(path).expect("journal is readable");
        let mut receipts = Vec::new();
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        {
            let value: Value = serde_json::from_slice(line).expect("journal line is JSON");
            if value.get("record_type").is_some() {
                receipts.extend(
                    serde_json::from_value::<AcceptedBatchV1>(value)
                        .expect("versioned receipt is valid")
                        .commands,
                );
            }
        }
        receipts
    }

    fn assert_versioned_receipts_are_unique(path: &Path) {
        let receipts = versioned_receipts(path);
        let mut message_ids = HashSet::new();
        for accepted in receipts {
            assert!(
                message_ids.insert(accepted.request.message_id.clone()),
                "a V1 receipt must occur only once in the physical journal"
            );
            assert!(
                !accepted.events.is_empty(),
                "every accepted-command receipt contains its complete events"
            );
        }
    }

    async fn assert_complete_canonical_history(host: &Host) {
        let canonical = host.canonical.lock().await;
        EventLogV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: EVENT_LOG_SCHEMA_VERSION,
            session_id: host.session_id.clone(),
            base_revision: 0,
            events: canonical.events.clone(),
        }
        .validate()
        .expect("canonical history has contiguous sequences and unique event ids");

        let receipt_ids: HashSet<_> = canonical.accepted_commands.keys().collect();
        assert_eq!(
            receipt_ids.len(),
            canonical.accepted_commands.len(),
            "accepted command identifiers are unique"
        );
        for accepted in canonical.accepted_commands.values() {
            assert!(
                canonical
                    .events
                    .windows(accepted.events.len())
                    .any(|window| window == accepted.events),
                "every receipt maps to one contiguous event range"
            );
        }
    }

    async fn open_vote_for(host: &Host, participants: &[&str], options: &[&str]) -> CommandRequest {
        for participant in participants {
            host.join(
                ParticipantId::from(*participant),
                (*participant).to_owned(),
                MessageId(format!("join-{participant}")),
            )
            .await
            .expect("participant joins");
        }
        let request = CommandRequest {
            message_id: MessageId::from("open-concurrent-vote"),
            actor: ParticipantId::from(participants[0]),
            expected_revision: host.current_revision().await,
            command: Command::Activity(VoteActivity::open(options)),
        };
        host.submit(request.clone()).await.expect("vote opens");
        request
    }

    async fn spawn_test_server(host: Arc<Host>, connection_count: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let address = listener.local_addr().expect("listener has an address");
        tokio::spawn(async move {
            for _ in 0..connection_count {
                let (stream, peer) = listener.accept().await.expect("client connects");
                let host = Arc::clone(&host);
                tokio::spawn(async move {
                    let _ = handle_connection(stream, peer, host).await;
                });
            }
        });
        format!("ws://{address}")
    }

    async fn send_network_message(
        socket: &mut sytog_transport::ClientSocket,
        session_id: &SessionId,
        participant: &ParticipantId,
        message_id: &MessageId,
        revision: u64,
        message: &NetworkMessage,
    ) {
        let wrapped = envelope(
            session_id.clone(),
            participant.clone(),
            message_id.clone(),
            Some(revision),
            message,
        )
        .expect("network message is wrapped");
        send(socket, &wrapped)
            .await
            .expect("network message is sent");
    }

    async fn submit_from_connection(
        url: String,
        session_id: SessionId,
        participant: ParticipantId,
        revision: u64,
        request: CommandRequest,
        ready: Arc<tokio::sync::Barrier>,
    ) -> Result<Vec<SessionEvent>, String> {
        let mut socket = sytog_transport::connect(&url)
            .await
            .expect("client connects");
        send_network_message(
            &mut socket,
            &session_id,
            &participant,
            &MessageId(format!("hello-{}", participant.0)),
            revision,
            &NetworkMessage::Hello {
                last_sequence: revision,
            },
        )
        .await;
        send_network_message(
            &mut socket,
            &session_id,
            &participant,
            &MessageId(format!("rejoin-{}", participant.0)),
            revision,
            &NetworkMessage::JoinSession {
                display_name: participant.0.clone(),
            },
        )
        .await;
        ready.wait().await;
        send_network_message(
            &mut socket,
            &session_id,
            &participant,
            &request.message_id,
            revision,
            &NetworkMessage::SubmitCommand {
                request: request.clone(),
            },
        )
        .await;

        tokio::time::timeout(ASYNC_TEST_TIMEOUT, async {
            while let Some(incoming) = receive(&mut socket).await.expect("host response is valid") {
                match decode(&incoming).expect("host message decodes") {
                    NetworkMessage::EventBatch { events, .. }
                        if events
                            .iter()
                            .any(|event| event.causation_id == request.message_id) =>
                    {
                        return Ok(events);
                    }
                    NetworkMessage::CommandRejected {
                        message_id, code, ..
                    } if message_id == request.message_id => return Err(code),
                    _ => {}
                }
            }
            panic!("connection closed without a command outcome");
        })
        .await
        .expect("host returns a command outcome before the test timeout")
    }

    async fn receive_catch_up_batch(
        url: &str,
        session_id: &SessionId,
        participant: &str,
        last_sequence: u64,
    ) -> (u64, Vec<SessionEvent>) {
        let participant = ParticipantId::from(participant);
        let mut socket = sytog_transport::connect(url)
            .await
            .expect("catch-up client connects");
        send_network_message(
            &mut socket,
            session_id,
            &participant,
            &MessageId(format!("catch-up-{}", participant.0)),
            last_sequence,
            &NetworkMessage::Hello { last_sequence },
        )
        .await;
        let incoming = tokio::time::timeout(ASYNC_TEST_TIMEOUT, receive(&mut socket))
            .await
            .expect("catch-up response arrives before the test timeout")
            .expect("catch-up response is valid")
            .expect("host sends catch-up batch");
        let NetworkMessage::EventBatch {
            from_sequence,
            events,
        } = decode(&incoming).expect("catch-up batch decodes")
        else {
            panic!("expected catch-up event batch");
        };
        (from_sequence, events)
    }

    async fn catch_up_connection(
        url: &str,
        session_id: &SessionId,
        participant: &str,
    ) -> ClientReplica {
        let (_, events) = receive_catch_up_batch(url, session_id, participant, 0).await;
        let mut replica = ClientReplica::uninitialized(session_id.clone());
        for event in events {
            assert_eq!(
                replica
                    .apply_received_event(&event)
                    .expect("catch-up event applies"),
                ReceivedEvent::Applied
            );
        }
        replica
    }

    #[test]
    fn identical_received_event_is_a_safe_noop() {
        let mut replica = ClientReplica::uninitialized(SessionId::from("duplicate-event"));
        let event = created_event("Host");
        assert_eq!(
            replica
                .apply_received_event(&event)
                .expect("first event applies"),
            ReceivedEvent::Applied
        );
        let before = replica.state.clone();
        assert_eq!(
            replica
                .apply_received_event(&event)
                .expect("identical event is accepted"),
            ReceivedEvent::AlreadySeen
        );
        assert_eq!(replica.state, before);
    }

    #[test]
    fn reused_event_id_with_different_content_is_rejected() {
        let mut replica = ClientReplica::uninitialized(SessionId::from("event-id-collision"));
        replica
            .apply_received_event(&created_event("Host"))
            .expect("first event applies");
        assert!(matches!(
            replica.apply_received_event(&created_event("Different Host")),
            Err(NodeError::EventIdCollision { .. })
        ));
    }

    #[test]
    fn old_sequence_with_different_content_is_rejected() {
        let mut replica = ClientReplica::uninitialized(SessionId::from("sequence-collision"));
        replica
            .apply_received_event(&created_event("Host"))
            .expect("first event applies");
        let mut conflicting = created_event("Host");
        conflicting.event_id = EventId::from("other:0");
        assert!(matches!(
            replica.apply_received_event(&conflicting),
            Err(NodeError::EventSequenceCollision { .. })
        ));
    }

    #[test]
    fn received_event_identity_survives_client_restart() {
        let directory = temporary_directory("client-event-history");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ClientConfig {
            url: "ws://127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("client-event-history"),
            participant_id: ParticipantId::from("alice"),
        };
        let event = created_event("Host");
        let mut replica = ClientReplica::uninitialized(config.session_id.clone());
        replica
            .apply_received_event(&event)
            .expect("first event applies");
        save_client_replica(&config, &replica).expect("replica persists");

        let mut reloaded = load_client_replica(&config).expect("replica reloads");
        assert_eq!(
            reloaded
                .apply_received_event(&event)
                .expect("identical event remains known"),
            ReceivedEvent::AlreadySeen
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[test]
    fn host_restarts_from_its_durable_journal() {
        let directory = temporary_directory("restart");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("restart-session"),
        };
        let first = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        let first_revision = first.canonical.blocking_lock().state.revision;
        drop(first);

        let restarted =
            Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host replays journal");
        assert_eq!(
            restarted.canonical.blocking_lock().state.revision,
            first_revision
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn host_loads_legacy_events_and_appends_versioned_acceptances() {
        let directory = temporary_directory("legacy-journal");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("legacy-journal"),
        };
        let journal = JournalStore::new(&directory, &config.session_id);
        fs::create_dir_all(&journal.directory).expect("journal directory is created");
        let mut state = SessionState::uninitialized(config.session_id.clone());
        let request = CommandRequest {
            message_id: MessageId::from("legacy-bootstrap"),
            actor: ParticipantId::from(HOST_ID),
            expected_revision: 0,
            command: Command::Session(SessionCommand::CreateSession {
                display_name: "Legacy Host".to_owned(),
            }),
        };
        let decision =
            sytog_runtime::execute(&mut state, &request, None).expect("bootstrap succeeds");
        let mut legacy_line =
            serde_json::to_vec(&decision.events[0]).expect("legacy event serializes");
        legacy_line.push(b'\n');
        fs::write(journal.events_path(), legacy_line).expect("legacy journal is written");

        let host = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("legacy journal remains readable");
        assert_eq!(host.current_revision().await, 1);
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("versioned-join"),
        )
        .await
        .expect("new acceptance appends after legacy event");
        drop(host);

        let restarted = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("mixed legacy and versioned journal replays");
        assert_eq!(restarted.current_revision().await, 2);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn two_participants_converge_and_catch_up_from_the_journal() {
        let directory = temporary_directory("distributed-vote");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("distributed-vote"),
        };
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("join-alice"),
        )
        .await
        .expect("Alice joins");
        host.join(
            ParticipantId::from("bob"),
            "Bob".to_owned(),
            MessageId::from("join-bob"),
        )
        .await
        .expect("Bob joins");

        for (message, actor, command) in [
            ("open", "alice", VoteActivity::open(&["tea", "coffee"])),
            ("bob-vote", "bob", VoteActivity::submit("coffee")),
            ("alice-vote", "alice", VoteActivity::submit("tea")),
            ("close", "alice", VoteActivity::close()),
        ] {
            let revision = host.current_revision().await;
            host.submit(CommandRequest {
                message_id: MessageId::from(message),
                actor: ParticipantId::from(actor),
                expected_revision: revision,
                command: Command::Activity(command),
            })
            .await
            .expect("activity command is accepted");
        }

        let canonical = host.canonical.lock().await;
        assert_eq!(canonical.state.revision, 8);
        let vote: VoteState = serde_json::from_value(
            canonical
                .state
                .activity
                .as_ref()
                .expect("vote is active")
                .state
                .clone(),
        )
        .expect("vote state is valid");
        assert_eq!(vote.result.expect("vote is closed")["coffee"], 1);
        drop(canonical);
        assert_eq!(host.events_after(3).await.len(), 5);

        let stale = host
            .submit(CommandRequest {
                message_id: MessageId::from("stale"),
                actor: ParticipantId::from("alice"),
                expected_revision: 3,
                command: Command::Activity(VoteActivity::close()),
            })
            .await
            .expect_err("stale command is rejected");
        assert_eq!(stale.code, "revision_conflict");

        drop(host);
        let restarted = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("host replays complete journal");
        assert_eq!(restarted.current_revision().await, 8);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn accepted_command_is_deduplicated_without_new_events() {
        let directory = temporary_directory("accepted-command-dedup");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("accepted-command-dedup"),
        };
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("join-alice"),
        )
        .await
        .expect("Alice joins");
        let request = CommandRequest {
            message_id: MessageId::from("open-once"),
            actor: ParticipantId::from("alice"),
            expected_revision: 2,
            command: Command::Activity(VoteActivity::open(&["tea", "coffee"])),
        };
        let accepted = host
            .submit(request.clone())
            .await
            .expect("first submission is accepted");
        let revision = host.current_revision().await;
        let duplicate = host
            .submit(request)
            .await
            .expect("an accepted command returns its prior success");
        assert!(duplicate.duplicate);
        assert_eq!(duplicate.events, accepted.events);
        assert_eq!(host.current_revision().await, revision);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn accepted_command_id_with_different_content_is_rejected_explicitly() {
        let directory = temporary_directory("command-id-collision");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("command-id-collision"),
        };
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("join-alice"),
        )
        .await
        .expect("Alice joins");
        host.submit(CommandRequest {
            message_id: MessageId::from("reused-command"),
            actor: ParticipantId::from("alice"),
            expected_revision: 2,
            command: Command::Activity(VoteActivity::open(&["tea", "coffee"])),
        })
        .await
        .expect("first submission is accepted");

        let rejection = host
            .submit(CommandRequest {
                message_id: MessageId::from("reused-command"),
                actor: ParticipantId::from("alice"),
                expected_revision: 4,
                command: Command::Activity(VoteActivity::submit("tea")),
            })
            .await
            .expect_err("different command content must be rejected");
        assert_eq!(rejection.code, "command_id_collision");
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn accepted_command_deduplication_survives_restart() {
        let directory = temporary_directory("command-dedup-restart");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("command-dedup-restart"),
        };
        let request = CommandRequest {
            message_id: MessageId::from("durable-command"),
            actor: ParticipantId::from("alice"),
            expected_revision: 2,
            command: Command::Activity(VoteActivity::open(&["tea", "coffee"])),
        };
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("join-alice"),
        )
        .await
        .expect("Alice joins");
        let accepted = host
            .submit(request.clone())
            .await
            .expect("first submission is accepted");
        let revision = host.current_revision().await;
        drop(host);

        let restarted =
            Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host restarts");
        let duplicate = restarted
            .submit(request)
            .await
            .expect("accepted command remains known after restart");
        assert!(duplicate.duplicate);
        assert_eq!(duplicate.events, accepted.events);
        assert_eq!(restarted.current_revision().await, revision);
        let collision = restarted
            .submit(CommandRequest {
                message_id: MessageId::from("durable-command"),
                actor: ParticipantId::from("alice"),
                expected_revision: revision,
                command: Command::Activity(VoteActivity::submit("tea")),
            })
            .await
            .expect_err("persisted command identity rejects different content");
        assert_eq!(collision.code, "command_id_collision");
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn rejected_command_id_can_be_reevaluated() {
        let directory = temporary_directory("rejected-command-retry");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("old test directory can be removed");
        }
        let config = ServerConfig {
            bind: "127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: SessionId::from("rejected-command-retry"),
        };
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("join-alice"),
        )
        .await
        .expect("Alice joins");
        let message_id = MessageId::from("retry-after-rejection");
        host.submit(CommandRequest {
            message_id: message_id.clone(),
            actor: ParticipantId::from("alice"),
            expected_revision: 2,
            command: Command::Activity(VoteActivity::close()),
        })
        .await
        .expect_err("invalid first submission is rejected");
        assert_eq!(host.current_revision().await, 2);
        drop(host);

        let restarted =
            Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host restarts");
        restarted
            .submit(CommandRequest {
                message_id,
                actor: ParticipantId::from("alice"),
                expected_revision: 2,
                command: Command::Activity(VoteActivity::open(&["tea", "coffee"])),
            })
            .await
            .expect("a rejected identifier is not retained");
        assert_eq!(restarted.current_revision().await, 4);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_distinct_commands_at_one_revision_are_linearized() {
        let (directory, config) = server_config("concurrent-same-revision");
        let host =
            Arc::new(Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host starts"));
        open_vote_for(&host, &["alice", "bob"], &["tea", "coffee"]).await;
        let revision = host.current_revision().await;
        let alice = CommandRequest {
            message_id: MessageId::from("alice-concurrent-vote"),
            actor: ParticipantId::from("alice"),
            expected_revision: revision,
            command: Command::Activity(VoteActivity::submit("tea")),
        };
        let bob = CommandRequest {
            message_id: MessageId::from("bob-concurrent-vote"),
            actor: ParticipantId::from("bob"),
            expected_revision: revision,
            command: Command::Activity(VoteActivity::submit("coffee")),
        };

        let alice_host = Arc::clone(&host);
        let bob_host = Arc::clone(&host);
        let (alice_result, bob_result) = tokio::join!(
            tokio::spawn(async move { alice_host.submit(alice).await }),
            tokio::spawn(async move { bob_host.submit(bob).await }),
        );
        let outcomes = [
            alice_result.expect("Alice task completes"),
            bob_result.expect("Bob task completes"),
        ];
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.as_ref().err())
                .filter(|rejection| rejection.code == "revision_conflict")
                .count(),
            1
        );
        assert_eq!(host.current_revision().await, revision + 1);
        assert_complete_canonical_history(&host).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        drop(host);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_identical_command_is_appended_once_and_replayed_to_both_callers() {
        let (directory, config) = server_config("concurrent-identical-command");
        let host =
            Arc::new(Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host starts"));
        host.join(
            ParticipantId::from("alice"),
            "Alice".to_owned(),
            MessageId::from("join-alice"),
        )
        .await
        .expect("Alice joins");
        let request = CommandRequest {
            message_id: MessageId::from("same-command"),
            actor: ParticipantId::from("alice"),
            expected_revision: 2,
            command: Command::Activity(VoteActivity::open(&["tea", "coffee"])),
        };
        let first_host = Arc::clone(&host);
        let second_host = Arc::clone(&host);
        let first_request = request.clone();
        let second_request = request.clone();
        let (first, second) = tokio::join!(
            tokio::spawn(async move { first_host.submit(first_request).await }),
            tokio::spawn(async move { second_host.submit(second_request).await }),
        );
        let first = first
            .expect("first task completes")
            .expect("first succeeds");
        let second = second
            .expect("second task completes")
            .expect("second succeeds");

        assert_eq!(first.events, second.events);
        assert_ne!(first.duplicate, second.duplicate);
        assert_eq!(host.current_revision().await, 4);
        assert_eq!(
            versioned_receipts(&journal_path(&config))
                .iter()
                .filter(|accepted| accepted.request.message_id == request.message_id)
                .count(),
            1
        );
        let accepted = host
            .canonical
            .lock()
            .await
            .accepted_commands
            .get(&request.message_id)
            .expect("receipt is indexed")
            .clone();
        assert_eq!(accepted.events, first.events);
        assert_eq!(
            accepted
                .events
                .windows(2)
                .next()
                .expect("auto-start command has two events")[1]
                .sequence,
            accepted.events[0].sequence + 1
        );
        assert_complete_canonical_history(&host).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        drop(host);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_command_id_collision_has_one_winner_and_one_explicit_rejection() {
        let (directory, config) = server_config("concurrent-command-id-collision");
        let host =
            Arc::new(Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host starts"));
        open_vote_for(&host, &["alice"], &["tea", "coffee"]).await;
        let revision = host.current_revision().await;
        let tea = CommandRequest {
            message_id: MessageId::from("colliding-command"),
            actor: ParticipantId::from("alice"),
            expected_revision: revision,
            command: Command::Activity(VoteActivity::submit("tea")),
        };
        let coffee = CommandRequest {
            command: Command::Activity(VoteActivity::submit("coffee")),
            ..tea.clone()
        };
        let tea_host = Arc::clone(&host);
        let coffee_host = Arc::clone(&host);
        let (tea_result, coffee_result) = tokio::join!(
            tokio::spawn(async move { tea_host.submit(tea).await }),
            tokio::spawn(async move { coffee_host.submit(coffee).await }),
        );
        let outcomes = [
            tea_result.expect("tea task completes"),
            coffee_result.expect("coffee task completes"),
        ];
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.as_ref().err())
                .filter(|rejection| rejection.code == "command_id_collision")
                .count(),
            1
        );
        assert_eq!(
            versioned_receipts(&journal_path(&config))
                .iter()
                .filter(|accepted| accepted.request.message_id.0 == "colliding-command")
                .count(),
            1
        );
        assert_complete_canonical_history(&host).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        drop(host);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn slow_command_holds_its_place_without_partial_interleaving() {
        let (directory, config) = server_config("slow-command-linearization");
        let entered = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(DelayedVoteActivity {
            entered_slow_decision: Arc::clone(&entered),
            delay: Duration::from_millis(100),
        });
        let host = Arc::new(Host::load_or_create(&config, activity).expect("host starts"));
        open_vote_for(&host, &["alice", "bob"], &["slow", "fast"]).await;
        let revision = host.current_revision().await;
        let slow = CommandRequest {
            message_id: MessageId::from("slow-command"),
            actor: ParticipantId::from("alice"),
            expected_revision: revision,
            command: Command::Activity(VoteActivity::submit("slow")),
        };
        let fast = CommandRequest {
            message_id: MessageId::from("fast-command"),
            actor: ParticipantId::from("bob"),
            expected_revision: revision + 1,
            command: Command::Activity(VoteActivity::submit("fast")),
        };

        let slow_host = Arc::clone(&host);
        let slow_task = tokio::spawn(async move { slow_host.submit(slow).await });
        tokio::time::timeout(ASYNC_TEST_TIMEOUT, async {
            while !entered.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("slow decision starts before the test timeout");
        let fast_host = Arc::clone(&host);
        let fast_task = tokio::spawn(async move { fast_host.submit(fast).await });
        let slow_result = slow_task
            .await
            .expect("slow task completes")
            .expect("slow command succeeds");
        let fast_result = fast_task
            .await
            .expect("fast task completes")
            .expect("fast command succeeds");

        assert_eq!(slow_result.events[0].sequence, revision + 1);
        assert_eq!(fast_result.events[0].sequence, revision + 2);
        assert_complete_canonical_history(&host).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        drop(host);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn separate_connections_share_one_canonical_order_and_catch_up_state() {
        let (directory, config) = server_config("concurrent-connections");
        let host =
            Arc::new(Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host starts"));
        open_vote_for(&host, &["alice", "bob"], &["tea", "coffee"]).await;
        let revision = host.current_revision().await;
        let url = spawn_test_server(Arc::clone(&host), 4).await;
        let ready = Arc::new(tokio::sync::Barrier::new(2));
        let alice_request = CommandRequest {
            message_id: MessageId::from("alice-network-vote"),
            actor: ParticipantId::from("alice"),
            expected_revision: revision,
            command: Command::Activity(VoteActivity::submit("tea")),
        };
        let bob_request = CommandRequest {
            message_id: MessageId::from("bob-network-vote"),
            actor: ParticipantId::from("bob"),
            expected_revision: revision,
            command: Command::Activity(VoteActivity::submit("coffee")),
        };
        let (alice, bob) = tokio::join!(
            submit_from_connection(
                url.clone(),
                config.session_id.clone(),
                ParticipantId::from("alice"),
                revision,
                alice_request,
                Arc::clone(&ready),
            ),
            submit_from_connection(
                url.clone(),
                config.session_id.clone(),
                ParticipantId::from("bob"),
                revision,
                bob_request,
                Arc::clone(&ready),
            ),
        );
        let outcomes = [alice, bob];
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| { matches!(outcome, Err(code) if code == "revision_conflict") })
                .count(),
            1
        );

        let (alice_replica, bob_replica) = tokio::join!(
            catch_up_connection(&url, &config.session_id, "catch-up-alice"),
            catch_up_connection(&url, &config.session_id, "catch-up-bob"),
        );
        let canonical_state = host.canonical.lock().await.state.clone();
        assert_eq!(alice_replica.state, canonical_state);
        assert_eq!(bob_replica.state, canonical_state);
        assert_complete_canonical_history(&host).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        drop(host);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn disconnect_during_acceptance_does_not_erase_a_durable_command() {
        let (directory, config) = server_config("disconnect-during-acceptance");
        let entered = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(DelayedVoteActivity {
            entered_slow_decision: Arc::clone(&entered),
            delay: Duration::from_millis(100),
        });
        let host = Arc::new(Host::load_or_create(&config, activity).expect("host starts"));
        open_vote_for(&host, &["alice"], &["slow", "other"]).await;
        let revision = host.current_revision().await;
        let url = spawn_test_server(Arc::clone(&host), 1).await;
        let mut socket = sytog_transport::connect(&url)
            .await
            .expect("client connects");
        let participant = ParticipantId::from("alice");
        send_network_message(
            &mut socket,
            &config.session_id,
            &participant,
            &MessageId::from("disconnect-hello"),
            revision,
            &NetworkMessage::Hello {
                last_sequence: revision,
            },
        )
        .await;
        send_network_message(
            &mut socket,
            &config.session_id,
            &participant,
            &MessageId::from("disconnect-rejoin"),
            revision,
            &NetworkMessage::JoinSession {
                display_name: "Alice".to_owned(),
            },
        )
        .await;
        let request = CommandRequest {
            message_id: MessageId::from("disconnect-command"),
            actor: participant.clone(),
            expected_revision: revision,
            command: Command::Activity(VoteActivity::submit("slow")),
        };
        send_network_message(
            &mut socket,
            &config.session_id,
            &participant,
            &request.message_id,
            revision,
            &NetworkMessage::SubmitCommand {
                request: request.clone(),
            },
        )
        .await;
        tokio::time::timeout(ASYNC_TEST_TIMEOUT, async {
            while !entered.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("slow decision starts before the test timeout");
        socket.close(None).await.expect("client disconnects");
        assert_eq!(host.current_revision().await, revision + 1);
        assert!(
            host.canonical
                .lock()
                .await
                .accepted_commands
                .contains_key(&request.message_id)
        );
        drop(host);

        let restarted = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("durable command survives restart");
        assert_eq!(restarted.current_revision().await, revision + 1);
        assert!(
            restarted
                .canonical
                .lock()
                .await
                .accepted_commands
                .contains_key(&request.message_id)
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_burst_accounts_for_every_command_without_history_gaps() {
        let (directory, config) = server_config("concurrent-burst");
        let host =
            Arc::new(Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host starts"));
        let participants: Vec<_> = (0..12).map(|index| format!("client-{index}")).collect();
        let participant_refs: Vec<_> = participants.iter().map(String::as_str).collect();
        open_vote_for(&host, &participant_refs, &["yes", "no"]).await;
        let revision = host.current_revision().await;
        let url = spawn_test_server(Arc::clone(&host), participants.len()).await;
        let ready = Arc::new(tokio::sync::Barrier::new(participants.len()));
        let tasks: Vec<_> = participants
            .into_iter()
            .enumerate()
            .map(|(index, participant)| {
                let request = CommandRequest {
                    message_id: MessageId(format!("burst-{index}")),
                    actor: ParticipantId::from(participant.as_str()),
                    expected_revision: revision,
                    command: Command::Activity(VoteActivity::submit(if index % 2 == 0 {
                        "yes"
                    } else {
                        "no"
                    })),
                };
                submit_from_connection(
                    url.clone(),
                    config.session_id.clone(),
                    ParticipantId::from(participant.as_str()),
                    revision,
                    request,
                    Arc::clone(&ready),
                )
            })
            .collect();
        let outcomes = futures_util::future::join_all(tasks).await;
        assert_eq!(outcomes.len(), 12);
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| { matches!(outcome, Err(code) if code == "revision_conflict") })
                .count(),
            11
        );
        assert_eq!(host.current_revision().await, revision + 1);
        assert_complete_canonical_history(&host).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        drop(host);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_order_and_receipts_survive_restart_and_replay() {
        let (directory, config) = server_config("concurrent-restart-order");
        let host =
            Arc::new(Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host starts"));
        let tasks: Vec<_> = (0..20)
            .map(|index| {
                let host = Arc::clone(&host);
                tokio::spawn(async move {
                    host.join(
                        ParticipantId(format!("participant-{index}")),
                        format!("Participant {index}"),
                        MessageId(format!("concurrent-join-{index}")),
                    )
                    .await
                })
            })
            .collect();
        for outcome in futures_util::future::join_all(tasks).await {
            outcome
                .expect("join task completes")
                .expect("concurrent join succeeds");
        }
        assert_complete_canonical_history(&host).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        let (before_state, before_events, before_receipts) = {
            let canonical = host.canonical.lock().await;
            (
                canonical.state.clone(),
                canonical.events.clone(),
                canonical.accepted_commands.clone(),
            )
        };
        drop(host);

        let restarted =
            Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host replays journal");
        let canonical = restarted.canonical.lock().await;
        assert_eq!(canonical.state, before_state);
        assert_eq!(canonical.events, before_events);
        assert_eq!(canonical.accepted_commands, before_receipts);
        drop(canonical);
        assert_complete_canonical_history(&restarted).await;
        assert_versioned_receipts_are_unique(&journal_path(&config));
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn persisted_old_replica_catches_up_large_suffix_after_host_restart() {
        const PARTICIPANT_COUNT: usize = 300;
        const CHECKPOINT_PARTICIPANTS: usize = 24;

        let (directory, config) = server_config("old-replica-large-catch-up");
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        for index in 0..CHECKPOINT_PARTICIPANTS {
            host.join(
                ParticipantId(format!("participant-{index}")),
                format!("Participant {index}"),
                MessageId(format!("join-{index}")),
            )
            .await
            .expect("checkpoint participant joins");
        }

        let checkpoint_events = host.canonical.lock().await.events.clone();
        let mut stale_replica = ClientReplica::uninitialized(config.session_id.clone());
        for event in &checkpoint_events {
            assert_eq!(
                stale_replica
                    .apply_received_event(event)
                    .expect("checkpoint event applies"),
                ReceivedEvent::Applied
            );
        }
        let checkpoint_revision = stale_replica.state.revision;
        let client_config = ClientConfig {
            url: "ws://127.0.0.1:0".to_owned(),
            data_dir: directory.clone(),
            session_id: config.session_id.clone(),
            participant_id: ParticipantId::from("old-client"),
        };
        save_client_replica(&client_config, &stale_replica).expect("old replica persists");
        let mut stale_replica = load_client_replica(&client_config).expect("old replica reloads");
        assert_eq!(stale_replica.state.revision, checkpoint_revision);

        for index in CHECKPOINT_PARTICIPANTS..PARTICIPANT_COUNT {
            host.join(
                ParticipantId(format!("participant-{index}")),
                format!("Participant {index}"),
                MessageId(format!("join-{index}")),
            )
            .await
            .expect("later participant joins");
        }
        let final_revision = host.current_revision().await;
        assert!(
            final_revision - checkpoint_revision > 256,
            "the stale suffix exceeds the broadcast channel capacity"
        );
        drop(host);

        let restarted = Arc::new(
            Host::load_or_create(&config, Arc::new(VoteActivity))
                .expect("host replays the complete journal"),
        );
        let canonical_state = restarted.canonical.lock().await.state.clone();
        assert_eq!(restarted.current_revision().await, final_revision);
        let url = spawn_test_server(Arc::clone(&restarted), 1).await;
        let (from_sequence, events) =
            receive_catch_up_batch(&url, &config.session_id, "old-client", checkpoint_revision)
                .await;

        assert_eq!(from_sequence, checkpoint_revision + 1);
        assert_eq!(
            u64::try_from(events.len()).expect("suffix length fits"),
            final_revision - checkpoint_revision
        );
        assert_eq!(
            events.first().expect("suffix has a first event").sequence,
            checkpoint_revision + 1
        );
        assert_eq!(
            events.last().expect("suffix has a last event").sequence,
            final_revision
        );
        for event in &events {
            assert_eq!(
                stale_replica
                    .apply_received_event(event)
                    .expect("catch-up event applies"),
                ReceivedEvent::Applied
            );
        }
        assert_eq!(stale_replica.state, canonical_state);
        save_client_replica(&client_config, &stale_replica).expect("caught-up replica persists");
        let reloaded = load_client_replica(&client_config).expect("caught-up replica reloads");
        assert_eq!(reloaded.state, canonical_state);
        assert_eq!(reloaded.events, stale_replica.events);

        drop(restarted);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[test]
    fn truncated_legacy_final_line_recovers_valid_prefix() {
        let (directory, config) = server_config("truncated-legacy-final");
        let safe_offset = write_legacy_two_event_journal(&config);
        let path = journal_path(&config);
        let bytes = fs::read(&path).expect("journal is readable");
        fs::write(&path, &bytes[..safe_offset + 12]).expect("last line is truncated");

        let restarted = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("valid legacy prefix is recovered");
        assert_eq!(restarted.canonical.blocking_lock().state.revision, 1);
        assert_eq!(
            fs::metadata(&path).expect("journal metadata").len(),
            u64::try_from(safe_offset).expect("offset fits")
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[test]
    fn invalid_final_bytes_recover_valid_prefix() {
        let (directory, config) = server_config("invalid-final-bytes");
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        let revision = host.canonical.blocking_lock().state.revision;
        drop(host);
        let path = journal_path(&config);
        let safe_offset = fs::metadata(&path).expect("journal metadata").len();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("journal opens")
            .write_all(&[0xff, 0xfe, 0xfd])
            .expect("invalid suffix is appended");

        let restarted = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("invalid unterminated suffix is recovered");
        assert_eq!(restarted.canonical.blocking_lock().state.revision, revision);
        assert_eq!(
            fs::metadata(&path).expect("journal metadata").len(),
            safe_offset
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[test]
    fn terminated_invalid_final_line_fails_without_repair() {
        let (directory, config) = server_config("terminated-invalid-final-line");
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        drop(host);
        let path = journal_path(&config);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("journal opens")
            .write_all(&[0xff, 0xfe, b'\n'])
            .expect("terminated invalid line is appended");
        let corrupted = fs::read(&path).expect("journal is readable");

        assert!(Host::load_or_create(&config, Arc::new(VoteActivity)).is_err());
        assert_eq!(
            fs::read(&path).expect("journal is readable"),
            corrupted,
            "a terminated invalid line is corruption, not a partial append"
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[test]
    fn final_empty_line_is_valid_and_unchanged() {
        let (directory, config) = server_config("final-empty-line");
        let host = Host::load_or_create(&config, Arc::new(VoteActivity)).expect("host bootstraps");
        let revision = host.canonical.blocking_lock().state.revision;
        drop(host);
        let path = journal_path(&config);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("journal opens")
            .write_all(b"\n")
            .expect("empty line is appended");
        let before = fs::read(&path).expect("journal is readable");

        let restarted =
            Host::load_or_create(&config, Arc::new(VoteActivity)).expect("empty line is valid");
        assert_eq!(restarted.canonical.blocking_lock().state.revision, revision);
        assert_eq!(fs::read(&path).expect("journal is readable"), before);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn syntactic_corruption_in_the_middle_fails_without_repair() {
        let (directory, config, _) = create_versioned_vote("middle-syntax-corruption", false).await;
        let path = journal_path(&config);
        let bytes = fs::read(&path).expect("journal is readable");
        let first_end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("first line ends");
        let second_end = bytes[first_end + 1..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| first_end + 1 + offset)
            .expect("second line ends");
        let mut corrupted = bytes[..=first_end].to_vec();
        corrupted.extend_from_slice(b"not-json\n");
        corrupted.extend_from_slice(&bytes[second_end + 1..]);
        fs::write(&path, &corrupted).expect("middle line is corrupted");

        assert!(Host::load_or_create(&config, Arc::new(VoteActivity)).is_err());
        assert_eq!(
            fs::read(&path).expect("journal is readable"),
            corrupted,
            "middle corruption must not be repaired"
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn semantic_corruption_fails_without_repair() {
        let (directory, config, _) = create_versioned_vote("semantic-corruption", false).await;
        let path = journal_path(&config);
        let bytes = fs::read(&path).expect("journal is readable");
        let mut lines: Vec<Value> = bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).expect("line is valid JSON"))
            .collect();
        lines[1]["commands"][0]["events"][0]["sequence"] = Value::from(99);
        let mut corrupted = Vec::new();
        for line in lines {
            serde_json::to_writer(&mut corrupted, &line).expect("line serializes");
            corrupted.push(b'\n');
        }
        corrupted.extend_from_slice(b"{\"incomplete\":");
        fs::write(&path, &corrupted).expect("semantic corruption is written");

        assert!(Host::load_or_create(&config, Arc::new(VoteActivity)).is_err());
        assert_eq!(
            fs::read(&path).expect("journal is readable"),
            corrupted,
            "semantic corruption must not be repaired"
        );
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn truncated_v1_receipt_recovers_once() {
        let (directory, config, _) = create_versioned_vote("truncated-v1-receipt", false).await;
        let path = journal_path(&config);
        let safe_offset = fs::metadata(&path).expect("journal metadata").len();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("journal opens")
            .write_all(b"{\"record_type\":\"accepted_commands\",\"schema_version\":1")
            .expect("partial receipt is appended");
        let pending = JournalStore::new(&config.data_dir, &config.session_id)
            .load()
            .expect("valid prefix is inspectable")
            .recovery
            .expect("recovery diagnostic is explicit");
        assert_eq!(pending.safe_offset, safe_offset);
        assert_eq!(
            pending.original_length,
            fs::metadata(&path).expect("journal metadata").len()
        );

        let first = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("partial receipt is recovered");
        assert_eq!(first.current_revision().await, 4);
        drop(first);
        assert_eq!(
            fs::metadata(&path).expect("journal metadata").len(),
            safe_offset
        );
        let recovered = fs::read(&path).expect("recovered journal is readable");

        let second = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("second restart is idempotent");
        assert_eq!(second.current_revision().await, 4);
        assert_eq!(fs::read(&path).expect("journal is readable"), recovered);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }

    #[tokio::test]
    async fn truncated_v1_event_preserves_prior_command_deduplication() {
        let (directory, config, open) = create_versioned_vote("truncated-v1-event", true).await;
        let path = journal_path(&config);
        let bytes = fs::read(&path).expect("journal is readable");
        let last_start = last_record_start(&bytes);
        let event_marker = bytes[last_start..]
            .windows(b"\"events\":[".len())
            .position(|window| window == b"\"events\":[")
            .map(|offset| last_start + offset)
            .expect("last receipt contains events");
        let cut = event_marker + b"\"events\":[".len() + 24;
        fs::write(&path, &bytes[..cut]).expect("event payload is truncated");

        let restarted = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("partial final event is recovered");
        assert_eq!(restarted.current_revision().await, 4);
        let duplicate = restarted
            .submit(open)
            .await
            .expect("prior accepted command remains deduplicated");
        assert!(duplicate.duplicate);
        drop(restarted);
        assert_eq!(
            fs::metadata(&path).expect("journal metadata").len(),
            u64::try_from(last_start).expect("offset fits")
        );

        let second = Host::load_or_create(&config, Arc::new(VoteActivity))
            .expect("second restart remains valid");
        assert_eq!(second.current_revision().await, 4);
        fs::remove_dir_all(directory).expect("test directory can be removed");
    }
}
