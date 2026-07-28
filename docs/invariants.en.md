[Français](invariants.md) | [English](invariants.en.md)

# SYTOG invariants and properties

This document is the verifiable map of SYTOG properties since the `v0.2.0`
baseline, updated as hardening experiments progress. It describes what the
system enforces, what tests have only demonstrated, what remains a target, and
which limitations are already known. It is not a general promise beyond the
stated scopes and assumptions.

## Statuses

| Status | Meaning |
| --- | --- |
| **Guaranteed** | All listed paths currently enforce the property in code. |
| **Demonstrated** | A test or experiment observed it under stated assumptions, without a general proof. |
| **Target** | The property is desired but is not yet assured. |
| **Refuted / limited** | A precise counterexample or limitation is already known. |

A status always applies to the scope written in the same section. For example,
a property guaranteed by `EventLogV0::validate` is not necessarily guaranteed
for a network event that a client discards before validation.

## Overview

| Identifier | Property | Current status |
| --- | --- | --- |
| INV-001 | Sequence continuity | Guaranteed |
| INV-002 | `event_id` uniqueness in the canonical journal | Guaranteed |
| INV-003 | Repeatable `causation_id` | Guaranteed |
| INV-004 | Deterministic replay with the same implementation | Demonstrated |
| INV-005 | Semantic convergence of client replicas | Demonstrated |
| INV-006 | Safe handling of duplicate events | Guaranteed |
| INV-007 | Durable deduplication of accepted commands | Guaranteed |
| INV-008 | Linearization by the authoritative host | Guaranteed |
| INV-009 | Recovery from a partial final JSONL line | Refuted / limited |
| INV-010 | Rejection of intermediate JSONL corruption | Guaranteed |
| INV-011 | Reconnection and sequence-based catch-up | Demonstrated |
| INV-012 | Bounded memory and backpressure | Refuted / limited |
| INV-013 | Persistence before memory commit and broadcast | Guaranteed |

## Journal and replay

### INV-001 — Sequence continuity

**Status: Guaranteed**

**Exact statement.** In every accepted `EventLogV0`, the event at index `i`
has sequence `base_revision + i + 1`, with no gap, duplicate, or reversal. The
V0.2 canonical journal uses `base_revision = 0`. A `SessionState` also applies
only the event immediately following its revision.

**Assumptions and scope.** The guarantee applies to journals passed through
`EventLogV0::validate` and events passed through `SessionState::apply`. It says
nothing about a JSONL file that has not yet been loaded and validated.

**Enforcement point.**

- [`EventLogV0::validate`](../crates/sytog-protocol/src/lib.rs#L37-L73)
  computes the expected sequence;
- [`SessionState::apply`](../crates/sytog-domain/src/lib.rs#L107-L131) rejects
  every other sequence;
- the node validates the prospective journal before persistence in
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L630-L697).

**Behavior on violation.** The validator returns `UnexpectedEventSequence`, or
the reducer returns `UnexpectedSequence`. `replay_log` stops without producing
a partially accepted state.

**Existing tests.**

- `sytog_protocol::tests::log_rejects_sequence_gaps`;
- `sytog_runtime::tests::multi_event_application_is_atomic`.

**Reproducible breaking attempt.** Copy a journal, remove its second line or
change a sequence from `2` to `3`, then try to rebuild a host from that copy.
Startup must fail before full replay.

### INV-002 — `event_id` uniqueness in the canonical journal

**Status: Guaranteed**

**Exact statement.** Two events in the same validated `EventLogV0` cannot
share an `event_id`, whether their other fields are identical or not.

**Assumptions and scope.** The guarantee applies to the complete journal
validated at load time and to the prospective journal built by the host before
every append. It does not cover the client path described in INV-006.

**Enforcement point.**

- the `event_ids` set in
  [`EventLogV0::validate`](../crates/sytog-protocol/src/lib.rs#L51-L70);
- prospective validation in
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L630-L681) before writing.

**Behavior on violation.** The journal is rejected with
`ProtocolError::DuplicateEventId`. The host turns a prospective collision into
a `journal_invariant_failed` rejection and does not commit the command.

**Existing tests.**

- `sytog_protocol::tests::log_rejects_duplicate_event_ids`.

**Reproducible breaking attempt.** Copy one JSONL line, give it the next
sequence without changing its `event_id`, then restart the host from that copy.
It must reject the journal because of the duplicate identifier.

### INV-003 — Repeatable `causation_id`

**Status: Guaranteed**

**Exact statement.** `causation_id` is not a unique key. Several valid events
may carry the same `causation_id` when their `event_id` values and sequences are
distinct. `EventId::from_causation` can distinguish them by ordinal.

**Assumptions and scope.** The validator guarantees that the field is not
empty, but it verifies neither the existence of the causal command nor that all
of its events actually share this identifier. This property expresses a model
permission, not proof of complete causal traceability.

**Enforcement point.**

- [`EventId::from_causation`](../crates/sytog-domain/src/lib.rs#L32-L36);
- [`SessionEvent`](../crates/sytog-domain/src/lib.rs#L294-L302) separates
  `event_id` and `causation_id`;
- [`EventLogV0::validate`](../crates/sytog-protocol/src/lib.rs#L51-L70)
  requires uniqueness only for `event_id`.

**Behavior on violation.** Repeating `causation_id` is not a violation. An
empty identifier is rejected; an `event_id` collision is rejected under
INV-002.

**Existing tests.**

- `sytog_protocol::tests::log_allows_shared_causation_with_unique_event_ids`.

**Reproducible breaking attempt.** Build two contiguous events with the same
`causation_id` and identifiers `<cause>:0` and `<cause>:1`. The journal must be
accepted. Reusing `<cause>:0` must then trigger INV-002.

### INV-004 — Deterministic replay with the same implementation

**Status: Demonstrated**

**Exact statement.** Given an initial state, a valid journal, the same Rust
reducer version, and the same dependencies, two replays apply events in the
same order and produce semantically equal `SessionState` values.

**Assumptions and scope.** The journal, session, and base revision are valid.
The property does not yet guarantee identical bytes or hashes across different
implementations, serializers, or versions: no portable canonical
serialization is specified.

**Enforcement point.**

- [`replay`](../crates/sytog-runtime/src/lib.rs#L234-L244) is an ordered
  reduction with no external effect;
- [`replay_log`](../crates/sytog-runtime/src/lib.rs#L246-L264) validates
  identity, base, and journal before reduction.

**Behavior on violation.** A protocol, session, base revision, or application
error stops replay. Silent divergence between two valid replays would be a
critical failure that is not currently detected automatically by a canonical
hash.

**Existing tests.**

- `sytog_runtime::tests::replay_reconstructs_exact_state`;
- `sytog_runtime::tests::replay_log_rejects_the_wrong_session`;
- `sytog_node::tests::host_restarts_from_its_durable_journal`.

**Reproducible breaking attempt.** Replay the same fixture twice from
`SessionState::uninitialized`, serialize both states with the same
configuration, and compare semantic equality. Repeat with a missing sequence
to verify rejection rather than divergence.

### INV-009 — Recovery from a partial final JSONL line

**Status: Refuted / limited**

**Exact target statement.** After a crash that left only the last JSONL line
incomplete, the host should identify that uncommitted suffix with certainty,
remove it, and reconstruct the last valid durable prefix.

**Current state, assumptions, and scope.** This recovery does not exist.
`JournalStore::load` deserializes every non-empty line and propagates the first
JSON error. A truncated final line—whether a raw V0 event or a V1 accepted
command batch—therefore currently blocks restart, just like any other
corruption.

**Enforcement point.**

- strict reading in
  [`JournalStore::load`](../crates/sytog-node/src/lib.rs#L726-L784);
- append writes a batch and calls `sync_data` in
  [`JournalStore::append_accepted`](../crates/sytog-node/src/lib.rs#L786-L801), without
  framing or a commit marker.

**Behavior on violation.** Loading returns `NodeError::Json` or an I/O error.
The host does not start and truncates nothing automatically.

**Existing tests.** No test covers a partial final write.

**Reproducible breaking attempt.** Copy a session directory, truncate the last
bytes of `events.jsonl` in the middle of its final JSON object, then start the
host from the copy. In the current state, startup must fail: this counterexample
confirms the limitation.

### INV-010 — Rejection of intermediate JSONL corruption

**Status: Guaranteed**

**Exact statement.** If any non-empty line in the JSONL journal cannot be read
or deserialized as a raw V0 event or a recognized V1 batch, loading fails. No
suffix following that line is silently replayed.

**Assumptions and scope.** The guarantee covers errors visible to the line
reader and `serde_json`. A modification that remains structurally valid JSON is
then subject to protocol and reducer invariants.

**Enforcement point.**

- strict loading and index reconstruction in
  [`JournalStore::load`](../crates/sytog-node/src/lib.rs#L726-L784);
- complete validation and replay in
  [`Host::load_or_create`](../crates/sytog-node/src/lib.rs#L472-L525).

**Behavior on violation.** The host refuses to start. It does not produce a
state from the valid prefix alone and does not rewrite the journal.

**Existing tests.** No dedicated intermediate-corruption test exists; the load
path enforces the guarantee but still needs a regression test.

**Reproducible breaking attempt.** On a copy of a journal containing at least
three lines, replace the second line with `not-json` and restart the host.
Startup must fail without modifying the copy.

## Commands, concurrency, and durability

### INV-007 — Durable deduplication of accepted commands

**Status: Guaranteed**

**Exact statement.** For a stable `(session_id, message_id)` pair that
was already accepted, every new submission of the same command must return the
previously accepted result without deciding, persisting, or broadcasting new
events. The same identifier with different content must be a fatal collision
or a structured rejection.

**Assumptions and scope.** The guarantee applies to accepted commands written
as a versioned V1 batch. Raw event lines produced by `v0.2.0` remain readable,
but they do not contain the request needed to deduplicate their historical
commands. Identity compares the complete `CommandRequest`, including actor,
expected revision, and payload.

**Enforcement point.**

- `SubmitCommand` carries the
  [`CommandRequest`](../crates/sytog-transport/src/lib.rs#L14-L41);
- [`Host::submit`](../crates/sytog-node/src/lib.rs#L546-L606) consults the durable index
  before checking revision;
- `AcceptedBatchV1` persists the accepted request and its exact returned event
  list in the same versioned line;
- [`JournalStore::load`](../crates/sytog-node/src/lib.rs#L726-L784) reconstructs the
  command index after restart.

**Behavior on repetition or collision.**

- same `message_id` and same accepted request: previous events are returned
  without another decision, write, revision, or global broadcast;
- accepted `message_id` with a different request: structured
  `command_id_collision` rejection before any decision;
- a rejected command is not recorded: its identifier may be evaluated again,
  including after restart. This is the explicit policy that only accepted
  facts belong to the canonical journal.

**Existing tests.**

- `accepted_command_is_deduplicated_without_new_events`;
- `accepted_command_id_with_different_content_is_rejected_explicitly`;
- `accepted_command_deduplication_survives_restart`;
- `rejected_command_id_can_be_reevaluated`;
- `host_loads_legacy_events_and_appends_versioned_acceptances`.

**Reproducible breaking attempt.** Submit a command, retain its request and
events, restart the host, then resubmit exactly the same request. Revision and
journal must remain unchanged and the response must contain the same events.
Then change one request field while retaining `message_id`: the host must
return `command_id_collision`.

### INV-008 — Linearization by the authoritative host

**Status: Guaranteed**

**Exact statement.** Within one V0.2 host process, session and activity
commands that reach `Host::join` or `Host::submit` are handled one at a time
under the same canonical lock. Every accepted command observes one revision and
records its events in a unique total order.

**Assumptions and scope.** There is one authoritative process and one `Host`
instance. Lock acquisition order among concurrent commands is not predetermined
and need not be identical across runs. Only the order eventually recorded in
the journal is canonical and reproducible through replay.

**Enforcement point.**

- lock in [`Host::join`](../crates/sytog-node/src/lib.rs#L527-L544);
- lock and revision check in
  [`Host::submit`](../crates/sytog-node/src/lib.rs#L546-L573);
- validation, persistence, and commit remain under that guard through
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L630-L697).

**Behavior under concurrency.** One command wins the lock and may be accepted.
Another carrying the same `expected_revision` is then rejected with
`revision_conflict`. There is no merge or distributed ordering among several
authorities.

**Existing tests.**

- `sytog_node::tests::two_participants_converge_and_catch_up_from_the_journal`
  verifies rejection of a stale revision;
- no test currently issues several submissions truly simultaneously.

**Reproducible breaking attempt.** Open ten connections, give them the same
latest revision, then release ten valid commands simultaneously. Verify that
one contiguous order appears in the journal, stale losers receive structured
rejections, and replay reproduces that order.

### INV-013 — Persistence before memory commit and broadcast

**Status: Guaranteed**

**Exact statement.** When an append returns successfully, the host has
validated the prospective journal, written the batch, and called `sync_data`
before replacing its canonical in-memory state and before broadcasting events.

**Assumptions and scope.** The guarantee assumes that the filesystem and
`sync_data` honor their contracts and that append returns normally. It does not
guarantee physical atomicity of the batch: an error or crash during `write_all`
may leave a partial suffix while preventing the memory commit.

**Enforcement point.**

- ordering in [`Host::commit`](../crates/sytog-node/src/lib.rs#L630-L697);
- writing and synchronization in
  [`JournalStore::append_accepted`](../crates/sytog-node/src/lib.rs#L786-L801).

**Behavior on violation.** An append error becomes `persistence_failed` and
prevents memory commit and broadcast. If the write was partial, the next
restart currently encounters INV-009.

**Existing tests.** Deduplication-after-restart and mixed V0/V1 journal tests
cover the successful path. No test yet injects a crash or error at every append
point.

**Reproducible breaking attempt.** Use an instrumented storage adapter that
fails after N bytes for every N in a multi-event batch. After every failure,
verify that no event was broadcast and measure whether the journal restarts
without intervention.

## Network and convergence

### INV-005 — Semantic convergence of client replicas

**Status: Demonstrated**

**Exact statement.** Clients starting from the same state and reducing the
same complete, canonical, ordered stream with the same code version arrive at
semantically equal `SessionState` values.

**Assumptions and scope.** There is one host, events are not altered, every
missing event eventually arrives, and clients and host use the same schema and
reducer. The equal hash observed in V0.2 is not a cross-implementation
guarantee: field order, Unicode, numbers, options, whitespace, and the hash
algorithm are not specified as a canonical serialization.

**Enforcement point.**

- local reduction in
  [`connect_client`](../crates/sytog-node/src/lib.rs#L253-L293);
- canonical stream produced after commit in
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L630-L697).

**Behavior on violation.** Detected gaps trigger catch-up. No automatic state
or hash comparison with the host currently detects silent divergence.

**Existing tests and experiments.**

- `sytog_node::tests::two_participants_converge_and_catch_up_from_the_journal`
  exercises two participants and an event suffix;
- the manual V0.2 path produced host, Alice, and Bob snapshots that were
  semantically and byte-for-byte equal with the same implementation.

**Reproducible breaking attempt.** Capture one journal, deliver it in batches
of different sizes and delays to two fresh reducers, then compare their states.
Repeat while dropping, duplicating, and altering an event to verify that every
difference is detected rather than silently reduced.

### INV-006 — Safe handling of duplicate events

**Status: Guaranteed**

**Exact statement.** An already applied event should be ignored only
when its `event_id`, sequence, and content are strictly identical to the known
canonical fact. The same `event_id` or sequence with different content must
trigger an invariant violation.

**Assumptions and scope.** The guarantee applies to events whose complete
identity is present in the client's V1 history. A legacy V0 snapshot contains
only state and revision: an event older than the available history is therefore
rejected with `EventHistoryUnavailable`, never assumed to be identical. The
canonical journal remains additionally protected by INV-001 and INV-002.

**Enforcement point.**

- `ClientReplicaV1` persists the snapshot, history base revision, and known
  events;
- [`ClientReplica::apply_received_event`](../crates/sytog-node/src/lib.rs#L890-L933)
  compares the complete event by sequence and searches for every reused
  `event_id` before reduction;
- [`load_client_replica`](../crates/sytog-node/src/lib.rs#L944-L958) validates the
  versioned history after restart.

**Behavior on duplication or collision.**

- strictly equal known event: `AlreadySeen`, with no modification;
- same `event_id` with different content or sequence: `EventIdCollision` and
  fail closed;
- same sequence with another event: `EventSequenceCollision` and fail closed;
- unverifiable old sequence because history is missing:
  `EventHistoryUnavailable`;
- non-contiguous future sequence: catch-up request.

**Existing tests.**

- `identical_received_event_is_a_safe_noop`;
- `reused_event_id_with_different_content_is_rejected`;
- `old_sequence_with_different_content_is_rejected`;
- `received_event_identity_survives_client_restart`.

**Reproducible breaking attempt.** Bring a V1 client to revision N, then send
the canonical event N, the same `event_id` with a different payload, and a
different `event_id` at sequence N. Only the first must be a no-op; the next
two must fail explicitly.

### INV-011 — Reconnection and sequence-based catch-up

**Status: Demonstrated**

**Exact statement.** A client with a local snapshot at revision N can announce
N, request strictly later events, and reduce the contiguous suffix through the
host's current revision.

**Assumptions and scope.** The host still holds the entire journal in memory,
the session and local snapshot are valid, the connection eventually delivers
the suffix, and no compaction has removed required events.

**Enforcement point.**

- `Hello.last_sequence` and `CatchUpRequest.after_sequence` in
  [`NetworkMessage`](../crates/sytog-transport/src/lib.rs#L14-L41);
- hello and catch-up responses in
  [`handle_connection`](../crates/sytog-node/src/lib.rs#L350-L430);
- gap detection and another request in
  [`connect_client`](../crates/sytog-node/src/lib.rs#L257-L285).

**Behavior on violation.** A visible gap triggers another request from the
local revision. There is no convergence timeout, no network snapshot actually
sent, and no strategy if the suffix is no longer available.

**Existing tests and experiments.**

- `two_participants_converge_and_catch_up_from_the_journal` verifies
  `events_after(3)` but not a complete WebSocket reconnection;
- reconnecting a lagging client and restarting the host were checked manually
  during V0.2.

**Reproducible breaking attempt.** Disconnect Bob at N, produce several events
with Alice, reconnect Bob with his old snapshot, and verify that he receives
exactly `N+1..current`. Repeat with a very old snapshot and artificial delay
between batches.

### INV-012 — Bounded memory and backpressure

**Status: Refuted / limited**

**Exact target statement.** Host memory, the amount cloned for catch-up, and
queued work for a slow client must have explicit bounds and documented overload
behavior.

**Current state, assumptions, and scope.** The broadcast channel is bounded at
256 batches, but the canonical journal remains entirely in a `Vec`. Every
`events_after` filters and clones the whole requested suffix into a new `Vec`.
The V1 client identity history also retains every event received after its base
revision. A lagging receiver falls back to the same complete catch-up. There is
no pagination, maximum window, compaction, quota, or overload rejection.

**Current enforcement point.**

- channel capacity in
  [`Host::load_or_create`](../crates/sytog-node/src/lib.rs#L513-L524);
- recovery after `Lagged` in
  [`handle_connection`](../crates/sytog-node/src/lib.rs#L440-L451);
- unbounded suffix clone in
  [`Host::events_after`](../crates/sytog-node/src/lib.rs#L699-L708).

**Current behavior under pressure.** The in-memory journal grows with the
session. An old catch-up allocates in proportion to the suffix. A slow client
may lag; its task then tries to clone and send every missing event. No service
bound is guaranteed.

**Existing tests.** No load, slow-client, saturated-channel, or very-old-client
test exists.

**Reproducible breaking attempt.** Produce a large journal, keep one client
from reading its socket, then request from sequence zero with a second client.
Measure memory, batch size, command latency, and behavior after exceeding 256
batches.

## Proposed breaking-experiment order

1. **Duplicates and collisions** — define history identity before any other
   measurement.
2. **JSONL corruption** — establish what is durably committed and recoverable.
3. **Concurrency** — exercise canonical ordering once history is reliable.
4. **Old reconnection** — verify convergence over a large suffix.
5. **Pressure and backpressure** — measure bounds after the previous semantics
   are stable.

The first two families protect canonical-journal integrity. Load tests or many
parallel clients would mostly produce noise until history identity,
idempotence, and recoverability are defined.
