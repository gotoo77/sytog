//! Authoritative local-network node with JSONL durability.

use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
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
    let mut local = load_client_state(&config)?;
    let mut outgoing_counter = 0_u64;

    send_client_message(
        &mut sink,
        &config,
        next_message_id(&config.participant_id, &mut outgoing_counter),
        local.revision,
        &NetworkMessage::Hello {
            last_sequence: local.revision,
        },
    )
    .await?;
    send_client_message(
        &mut sink,
        &config,
        next_message_id(&config.participant_id, &mut outgoing_counter),
        local.revision,
        &NetworkMessage::JoinSession {
            display_name: config.participant_id.0.clone(),
        },
    )
    .await?;

    println!(
        "connected as {} at local revision {}",
        config.participant_id.0, local.revision
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
                    println!("{}", serde_json::to_string_pretty(&local)?);
                    continue;
                }
                match parse_command(line) {
                    Ok(command) => {
                        let message_id = MessageId(format!(
                            "{}:command:{}",
                            config.participant_id.0, local.revision
                        ));
                        let request = CommandRequest {
                            message_id: message_id.clone(),
                            actor: config.participant_id.clone(),
                            expected_revision: local.revision,
                            command: Command::Activity(command),
                        };
                        send_client_message(
                            &mut sink,
                            &config,
                            message_id,
                            local.revision,
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
                            if event.sequence <= local.revision {
                                continue;
                            }
                            if event.sequence != local.revision + 1 {
                                gap = true;
                                break;
                            }
                            local.apply(&event)?;
                            println!(
                                "event {} sequence={} revision={}",
                                event.event_id.0, event.sequence, local.revision
                            );
                        }
                        save_client_state(&config, &local)?;
                        if gap {
                            send_client_message(
                                &mut sink,
                                &config,
                                next_message_id(&config.participant_id, &mut outgoing_counter),
                                local.revision,
                                &NetworkMessage::CatchUpRequest {
                                    after_sequence: local.revision,
                                },
                            )
                            .await?;
                        }
                    }
                    NetworkMessage::StateSnapshot { snapshot } => {
                        snapshot.validate()?;
                        ensure_session(&snapshot.session_id, &config.session_id)?;
                        local = snapshot.state;
                        save_client_state(&config, &local)?;
                        println!("snapshot applied at revision {}", local.revision);
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
                        if current_revision > local.revision {
                            send_client_message(
                                &mut sink,
                                &config,
                                next_message_id(&config.participant_id, &mut outgoing_counter),
                                local.revision,
                                &NetworkMessage::CatchUpRequest {
                                    after_sequence: local.revision,
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
                        if let Err(rejection) = host.submit(request).await {
                            send_rejection(&mut sink, &host, message_id, rejection).await?;
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
        let existing = journal.load_events()?;
        let (state, events) = if existing.is_empty() {
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
            journal.append_events(&decision.events)?;
            journal.write_metadata()?;
            journal.write_snapshot(&state)?;
            (state, decision.events)
        } else {
            let log = EventLogV0 {
                family: PROTOCOL_FAMILY.to_owned(),
                protocol_version: PROTOCOL_VERSION,
                schema_version: EVENT_LOG_SCHEMA_VERSION,
                session_id: config.session_id.clone(),
                base_revision: 0,
                events: existing.clone(),
            };
            let state = replay_log(SessionState::uninitialized(config.session_id.clone()), &log)?;
            (state, existing)
        };
        let (sender, _) = broadcast::channel(256);
        Ok(Self {
            session_id: config.session_id.clone(),
            activity,
            journal,
            canonical: Mutex::new(CanonicalSession { state, events }),
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
        self.accept(&mut canonical, &request, None)
    }

    async fn submit(&self, request: CommandRequest) -> Result<(), HostRejection> {
        let mut canonical = self.canonical.lock().await;
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
            return self.commit(
                &mut canonical,
                candidate,
                [start_decision.events, activity_decision.events].concat(),
            );
        }
        self.accept(&mut canonical, &request, Some(self.activity.as_ref()))
    }

    fn accept(
        &self,
        canonical: &mut CanonicalSession,
        request: &CommandRequest,
        activity: Option<&dyn ActivityEngine>,
    ) -> Result<(), HostRejection> {
        let decision = decide(&canonical.state, request, activity)
            .map_err(|error| HostRejection::from_runtime(&error, canonical.state.revision))?;
        let mut candidate = canonical.state.clone();
        apply_decision_atomically(&mut candidate, &decision).map_err(|error| {
            HostRejection::new("apply_failed", error.to_string(), canonical.state.revision)
        })?;
        self.commit(canonical, candidate, decision.events)
    }

    fn commit(
        &self,
        canonical: &mut CanonicalSession,
        candidate: SessionState,
        events: Vec<SessionEvent>,
    ) -> Result<(), HostRejection> {
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
        self.journal.append_events(&events).map_err(|error| {
            HostRejection::new(
                "persistence_failed",
                error.to_string(),
                canonical.state.revision,
            )
        })?;
        canonical.state = candidate;
        canonical.events.extend(events.clone());
        if let Err(error) = self.journal.write_snapshot(&canonical.state) {
            eprintln!("snapshot update failed after durable journal commit: {error}");
        }
        let _ = self.events.send(events);
        Ok(())
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

    fn load_events(&self) -> Result<Vec<SessionEvent>, NodeError> {
        let path = self.events_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        BufReader::new(file)
            .lines()
            .filter(|line| line.as_ref().map_or(true, |line| !line.trim().is_empty()))
            .map(|line| {
                let line = line?;
                serde_json::from_str(&line).map_err(NodeError::Json)
            })
            .collect()
    }

    fn append_events(&self, events: &[SessionEvent]) -> Result<(), NodeError> {
        fs::create_dir_all(&self.directory)?;
        let mut bytes = Vec::new();
        for event in events {
            serde_json::to_writer(&mut bytes, event)?;
            bytes.push(b'\n');
        }
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

fn write_json_atomically(path: &Path, value: &impl Serialize) -> Result<(), NodeError> {
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn load_client_state(config: &ClientConfig) -> Result<SessionState, NodeError> {
    let path = client_state_path(config);
    if !path.exists() {
        return Ok(SessionState::uninitialized(config.session_id.clone()));
    }
    let snapshot: SnapshotV0 = serde_json::from_slice(&fs::read(path)?)?;
    snapshot.validate()?;
    ensure_session(&snapshot.session_id, &config.session_id)?;
    Ok(snapshot.state)
}

fn save_client_state(config: &ClientConfig, state: &SessionState) -> Result<(), NodeError> {
    let path = client_state_path(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let snapshot = SnapshotV0 {
        family: PROTOCOL_FAMILY.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        session_id: state.session_id.clone(),
        revision: state.revision,
        state: state.clone(),
    };
    write_json_atomically(&path, &snapshot)
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
    #[error("invalid session directory")]
    InvalidSessionPath,
    #[error("client sent a host-only network message")]
    UnexpectedClientMessage,
    #[error("host sent a client-only network message")]
    UnexpectedHostMessage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sytog_demo_vote::VoteActivity;
    use sytog_demo_vote::VoteState;

    fn temporary_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sytog-{name}-{}", std::process::id()))
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
}
