use std::{fs, path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use serde::Serialize;
use sytog_capabilities::{JobRequirement, NodeOffer, rank, validate_nodes};
use sytog_demo_counter::CounterActivity;
use sytog_demo_vote::VoteActivity;
use sytog_domain::{
    Command, CommandRequest, MessageId, ParticipantId, SessionCommand, SessionId, SessionState,
};
use sytog_node::{ClientConfig, ServerConfig};
use sytog_protocol::{
    EVENT_LOG_SCHEMA_VERSION, Envelope, EventLogV0, PROTOCOL_FAMILY, PROTOCOL_VERSION,
    SNAPSHOT_SCHEMA_VERSION, SnapshotV0,
};
use sytog_runtime::{execute, replay_log};
use thiserror::Error;

#[derive(Parser)]
#[command(name = "sytog", version, about = "SYTOG local vertical slice")]
struct Cli {
    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: TopLevel,
}

#[derive(Subcommand)]
enum TopLevel {
    /// Run the authoritative local WebSocket host.
    Serve {
        #[arg(long, default_value = "127.0.0.1:7878")]
        bind: String,
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
        #[arg(long, default_value = "demo-vote")]
        session: String,
    },
    /// Connect an interactive participant to a local WebSocket host.
    Connect {
        url: String,
        #[arg(long)]
        participant: String,
        #[arg(long, default_value = "demo-vote")]
        session: String,
        #[arg(long, default_value = "data/clients")]
        data_dir: PathBuf,
    },
    /// Run a built-in deterministic demonstration.
    Demo {
        #[command(subcommand)]
        kind: Demo,
    },
    /// Replay a JSON event log.
    Replay { event_log: PathBuf },
    /// Validate a protocol envelope, job, or node list.
    Validate { file: PathBuf },
    /// Match a job requirement against node offers.
    Capability {
        #[command(subcommand)]
        command: Capability,
    },
}

#[derive(Subcommand)]
enum Demo {
    Session,
    Capabilities,
    Vote,
}

#[derive(Subcommand)]
enum Capability {
    Match {
        job_file: PathBuf,
        nodes_file: PathBuf,
    },
}

#[derive(Debug, Error)]
enum CliError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Runtime(#[from] sytog_runtime::RuntimeError),
    #[error(transparent)]
    Node(#[from] sytog_node::NodeError),
    #[error(transparent)]
    Replay(#[from] sytog_runtime::ReplayError),
    #[error(transparent)]
    Protocol(#[from] sytog_protocol::ProtocolError),
    #[error(transparent)]
    Serialize(#[from] serde_json::Error),
    #[error("{0}")]
    Semantic(String),
    #[error("file does not match a supported schema")]
    UnknownSchema,
}

#[derive(Serialize)]
struct SessionDemo {
    event_log: EventLogV0,
    refused_command: String,
    final_state: SessionState,
    snapshot: SnapshotV0,
    replay_matches: bool,
}

#[derive(Serialize)]
struct ActivityDemo {
    event_log: EventLogV0,
    final_state: SessionState,
    snapshot: SnapshotV0,
    replay_matches: bool,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        TopLevel::Serve {
            bind,
            data_dir,
            session,
        } => {
            sytog_node::serve(
                ServerConfig {
                    bind,
                    data_dir,
                    session_id: SessionId(session),
                },
                Arc::new(VoteActivity),
            )
            .await?;
        }
        TopLevel::Connect {
            url,
            participant,
            session,
            data_dir,
        } => {
            sytog_node::connect_client(
                ClientConfig {
                    url,
                    data_dir,
                    session_id: SessionId(session),
                    participant_id: ParticipantId(participant),
                },
                parse_vote_network_command,
            )
            .await?;
        }
        TopLevel::Demo {
            kind: Demo::Session,
        } => print_value(&session_demo()?, cli.json)?,
        TopLevel::Demo {
            kind: Demo::Capabilities,
        } => {
            let job: JobRequirement = read_json(&PathBuf::from("fixtures/capabilities/job.json"))?;
            let nodes: Vec<NodeOffer> =
                read_json(&PathBuf::from("fixtures/capabilities/nodes.json"))?;
            validate_capability_inputs(&job, &nodes)?;
            print_value(&rank(&job, &nodes), cli.json)?;
        }
        TopLevel::Demo { kind: Demo::Vote } => print_value(&vote_demo()?, cli.json)?,
        TopLevel::Replay { event_log } => {
            let log: EventLogV0 = read_json(&event_log)?;
            let state = replay_log(SessionState::uninitialized(log.session_id.clone()), &log)?;
            print_value(&state, cli.json)?;
        }
        TopLevel::Validate { file } => validate_file(&file, cli.json)?,
        TopLevel::Capability {
            command:
                Capability::Match {
                    job_file,
                    nodes_file,
                },
        } => {
            let job: JobRequirement = read_json(&job_file)?;
            let nodes: Vec<NodeOffer> = read_json(&nodes_file)?;
            validate_capability_inputs(&job, &nodes)?;
            print_value(&rank(&job, &nodes), cli.json)?;
        }
    }
    Ok(())
}

fn parse_vote_network_command(line: &str) -> Result<sytog_domain::ActivityCommandEnvelope, String> {
    let words: Vec<_> = line.split_whitespace().collect();
    match words.as_slice() {
        ["open", options @ ..] if options.len() >= 2 => Ok(VoteActivity::open(options)),
        ["vote", choice] => Ok(VoteActivity::submit(choice)),
        ["close"] => Ok(VoteActivity::close()),
        _ => Err(
            "invalid command; use open <option...>, vote <choice>, close, state, or quit"
                .to_owned(),
        ),
    }
}

fn session_demo() -> Result<SessionDemo, CliError> {
    let counter = CounterActivity;
    let mut state = SessionState::uninitialized(SessionId::from("demo-session"));
    let mut log = Vec::new();
    for (message, actor, command) in [
        (
            "m1",
            "alice",
            session(SessionCommand::CreateSession {
                display_name: "Alice".to_owned(),
            }),
        ),
        (
            "m2",
            "bob",
            session(SessionCommand::Join {
                display_name: "Bob".to_owned(),
            }),
        ),
    ] {
        let request = request(message, actor, state.revision, command);
        log.extend(execute(&mut state, &request, Some(&counter))?.events);
    }

    let before_refusal = state.clone();
    let current_revision = state.revision;
    let refused = execute(
        &mut state,
        &request(
            "m3",
            "bob",
            current_revision,
            session(SessionCommand::StartActivity {
                descriptor: CounterActivity::descriptor(),
            }),
        ),
        Some(&counter),
    )
    .expect_err("Bob deliberately lacks authority");
    debug_assert_eq!(state, before_refusal);

    for (message, actor, command) in [
        (
            "m4",
            "alice",
            session(SessionCommand::StartActivity {
                descriptor: CounterActivity::descriptor(),
            }),
        ),
        ("m5", "bob", Command::Activity(CounterActivity::command(3))),
        (
            "m6",
            "alice",
            session(SessionCommand::TransferAuthority {
                to: ParticipantId::from("bob"),
            }),
        ),
    ] {
        let request = request(message, actor, state.revision, command);
        log.extend(execute(&mut state, &request, Some(&counter))?.events);
    }

    let session_id = SessionId::from("demo-session");
    let event_log = EventLogV0 {
        family: PROTOCOL_FAMILY.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        schema_version: EVENT_LOG_SCHEMA_VERSION,
        session_id: session_id.clone(),
        base_revision: 0,
        events: log,
    };
    let rebuilt = replay_log(SessionState::uninitialized(session_id.clone()), &event_log)?;
    Ok(SessionDemo {
        event_log,
        refused_command: refused.to_string(),
        snapshot: SnapshotV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            session_id,
            revision: state.revision,
            state: state.clone(),
        },
        replay_matches: rebuilt == state,
        final_state: state,
    })
}

fn vote_demo() -> Result<ActivityDemo, CliError> {
    let vote = VoteActivity;
    let session_id = SessionId::from("vote-session");
    let mut state = SessionState::uninitialized(session_id.clone());
    let mut log = Vec::new();
    let commands = [
        (
            "v1",
            "alice",
            session(SessionCommand::CreateSession {
                display_name: "Alice".to_owned(),
            }),
        ),
        (
            "v2",
            "bob",
            session(SessionCommand::Join {
                display_name: "Bob".to_owned(),
            }),
        ),
        (
            "v3",
            "alice",
            session(SessionCommand::StartActivity {
                descriptor: VoteActivity::descriptor(),
            }),
        ),
        (
            "v4",
            "alice",
            Command::Activity(VoteActivity::open(&["tea", "coffee"])),
        ),
        (
            "v5",
            "bob",
            Command::Activity(VoteActivity::submit("coffee")),
        ),
        (
            "v6",
            "alice",
            Command::Activity(VoteActivity::submit("tea")),
        ),
        ("v7", "alice", Command::Activity(VoteActivity::close())),
    ];
    for (message, actor, command) in commands {
        let request = request(message, actor, state.revision, command);
        log.extend(execute(&mut state, &request, Some(&vote))?.events);
    }

    let event_log = EventLogV0 {
        family: PROTOCOL_FAMILY.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        schema_version: EVENT_LOG_SCHEMA_VERSION,
        session_id: session_id.clone(),
        base_revision: 0,
        events: log,
    };
    let rebuilt = replay_log(SessionState::uninitialized(session_id.clone()), &event_log)?;
    Ok(ActivityDemo {
        event_log,
        snapshot: SnapshotV0 {
            family: PROTOCOL_FAMILY.to_owned(),
            protocol_version: PROTOCOL_VERSION,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            session_id,
            revision: state.revision,
            state: state.clone(),
        },
        replay_matches: rebuilt == state,
        final_state: state,
    })
}

fn session(command: SessionCommand) -> Command {
    Command::Session(command)
}

fn request(message: &str, actor: &str, revision: u64, command: Command) -> CommandRequest {
    CommandRequest {
        message_id: MessageId::from(message),
        actor: ParticipantId::from(actor),
        expected_revision: revision,
        command,
    }
}

fn validate_capability_inputs(job: &JobRequirement, nodes: &[NodeOffer]) -> Result<(), CliError> {
    job.validate()
        .map_err(|error| CliError::Semantic(error.to_string()))?;
    validate_nodes(nodes).map_err(|error| CliError::Semantic(error.to_string()))?;
    Ok(())
}

fn validate_file(path: &PathBuf, json: bool) -> Result<(), CliError> {
    let text = read(path)?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|source| CliError::Json {
            path: path.clone(),
            source,
        })?;
    if let Ok(envelope) = serde_json::from_value::<Envelope>(value.clone()) {
        envelope.validate()?;
        return print_value(
            &serde_json::json!({"valid": true, "schema": "envelope"}),
            json,
        );
    }
    if let Ok(log) = serde_json::from_value::<EventLogV0>(value.clone()) {
        log.validate()?;
        return print_value(
            &serde_json::json!({"valid": true, "schema": "event_log_v0"}),
            json,
        );
    }
    if let Ok(snapshot) = serde_json::from_value::<SnapshotV0>(value.clone()) {
        snapshot.validate()?;
        return print_value(
            &serde_json::json!({"valid": true, "schema": "snapshot_v0"}),
            json,
        );
    }
    if serde_json::from_value::<JobRequirement>(value.clone()).is_ok() {
        let job: JobRequirement = serde_json::from_value(value.clone())?;
        job.validate()
            .map_err(|error| CliError::Semantic(error.to_string()))?;
        return print_value(&serde_json::json!({"valid": true, "schema": "job"}), json);
    }
    if let Ok(nodes) = serde_json::from_value::<Vec<NodeOffer>>(value) {
        validate_nodes(&nodes).map_err(|error| CliError::Semantic(error.to_string()))?;
        return print_value(&serde_json::json!({"valid": true, "schema": "nodes"}), json);
    }
    Err(CliError::UnknownSchema)
}

fn read(path: &PathBuf) -> Result<String, CliError> {
    fs::read_to_string(path).map_err(|source| CliError::Read {
        path: path.clone(),
        source,
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T, CliError> {
    let text = read(path)?;
    serde_json::from_str(&text).map_err(|source| CliError::Json {
        path: path.clone(),
        source,
    })
}

fn print_value(value: &impl Serialize, json: bool) -> Result<(), CliError> {
    if json {
        println!("{}", serde_json::to_string(value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}
