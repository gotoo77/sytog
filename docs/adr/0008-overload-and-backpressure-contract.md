# ADR 0008: Overload and backpressure contract

Status: accepted

Implementation note: protocol slice 1 defines V2 vocabulary, validation,
versioned decoding, stable overload reasons, and the observable close contract.
The V0.2 node still emits and handles V1 only; no queue, timeout, admission,
catch-up, snapshot, or retention behavior from this ADR is active yet.

## Context

SYTOG V0.2 has one authoritative host. A command is decided, validated,
appended and synchronized to the canonical journal, committed in memory, then
published while the canonical lock is held. Exact replay and the journal order
are authoritative; notification delivery is not.

The current node has no explicit overload contract:

- `serve` spawns one task for every accepted TCP connection without a quota;
- `Host` keeps the complete event journal and accepted-command index in memory;
- accepted batches are published through a 256-slot Tokio broadcast channel;
- publication does not wait for subscribers;
- a lagging subscriber loses old broadcast slots, observes `Lagged`, then
  clones the complete missing suffix from the in-memory journal;
- `Hello` and `CatchUpRequest` return the entire suffix in one `EventBatch`;
- WebSocket serialization and writes have no size or duration limit;
- the client retains every event received after its history base revision.

The V0.2 breaking experiments demonstrated that 300 commits complete while one
subscriber remains idle. That subscriber then observes `Lagged` and recovers by
cloning all 300 missing events. This preserves canonical truth but does not
bound memory, connection work, catch-up work, or network writes.

SYTOG prioritizes stability, explicit behavior, exact replay, and deterministic
recovery over maximum throughput. A slow or hostile client must not silently
lose facts, and must not be able to block authoritative progress indefinitely.

## Decision drivers

- Preserve the canonical journal, exact replay, and single-host total order.
- Never report an event as delivered merely because it was published or queued.
- Prevent one slow connection from consuming unbounded memory or blocking
  unrelated clients.
- Bound server-wide admission as well as per-connection work.
- Make overload, lag, disconnect, and required resynchronization observable.
- Keep reconnect and catch-up deterministic from a client-owned durable cursor.
- Prefer explicit rejection or disconnection to silent loss or indefinite wait.
- Separate durable authority from notification and catch-up retention.
- Introduce the contract in small, testable slices.

## Recommendation

Adopt the combined strategy: bounded per-connection queues, explicit lag
detection, write deadlines, slow-consumer disconnect, and deterministic
reconnect/catch-up from a client-owned durable cursor. Add separate global
admission limits so the host may reject excess work before commit without
coupling authoritative progress to consumer speed.

Keep the complete durable archive during the first implementation slices.
Paginate and bound catch-up before introducing a hot retention floor. Add
snapshot-plus-suffix resync next, then separate the hot window from the exact
archive. Do not use destructive journal compaction as the first backpressure
mechanism.

## Four distinct pressure domains

The implementation must not collapse these concerns into one queue:

1. **Authoritative production pressure** covers command admission, sequencing,
   durable append, and commit. It protects the host as a whole.
2. **Per-connection pressure** covers live notifications waiting for one
   client. It isolates clients from one another.
3. **Journal retention** determines which historical facts remain available
   for replay, audit, and catch-up.
4. **Catch-up and network pressure** covers page construction, serialization,
   write duration, and work performed for reconnecting or hostile clients.

## Strategies considered

### A. Globally slow authoritative producers

- **Safety:** preserves order and can avoid notification loss while every
  consumer participates.
- **Availability:** one stalled consumer can stop all accepted work.
- **Isolation:** none; the slowest client controls the session.
- **Memory:** can be bounded if producers wait before creating more output.
- **Global blocking risk:** critical, including deadlock-like operational
  failure when a client never reads.
- **Client observation:** commands remain pending for an unbounded duration
  unless separate producer timeouts are added.
- **Replay, catch-up, restart:** compatible, but unnecessary because delivery
  is coupled to commit progress.
- **Complexity:** mechanically simple, operationally dangerous.
- **Invariant effect:** may bound queues, but violates availability and the
  requirement that a client cannot block authoritative progress indefinitely.

Rejected as the primary contract. Global admission may still be bounded and
may reject new commands explicitly; it must not wait for live consumers.

### B. Give every connection a bounded output queue

- **Safety:** safe if queue overflow never mutates canonical truth and never
  pretends dropped notifications were delivered.
- **Availability:** authoritative work and other clients continue.
- **Isolation:** strong, up to global connection and memory quotas.
- **Memory:** bounded per connection only when both item count and serialized
  byte budget are enforced.
- **Global blocking risk:** low; aggregate memory remains unbounded without a
  connection quota.
- **Client observation:** overflow needs an explicit slow-consumer transition.
- **Replay, catch-up, restart:** compatible when the client reconnects from its
  durable applied cursor.
- **Complexity:** medium; requires a writer task, queue accounting, cancellation,
  and shutdown coordination.
- **Invariant effect:** enables bounded per-connection work, but does not bound
  the journal or catch-up by itself.

Accepted as one component, not as the complete contract.

### C. Disconnect consumers that are too slow

- **Safety:** canonical events remain durable; a close notification itself can
  be lost, so reconnect behavior must also cover unexplained transport loss.
- **Availability:** high for the host and healthy clients.
- **Isolation:** strong with connection quotas and write deadlines.
- **Memory:** bounded if disconnection follows a bounded queue or write timeout.
- **Global blocking risk:** low.
- **Client observation:** best-effort protocol reason plus WebSocket close;
  any close means delivery is unknown beyond the client's durable cursor.
- **Replay, catch-up, restart:** compatible when reconnect is mandatory.
- **Complexity:** low to medium, but correct cursor semantics are essential.
- **Invariant effect:** guarantees bounded connection lifetime under stalled
  writes; cannot guarantee that the close reason reaches a broken peer.

Accepted when combined with bounded queues and deterministic reconnect.

### D. Drop intermediate notifications and require explicit resynchronization

- **Safety:** safe because notifications are hints and the journal is
  authoritative.
- **Availability:** high.
- **Isolation:** good.
- **Memory:** bounded for live notification queues.
- **Global blocking risk:** low.
- **Client observation:** unsafe if the drop is invisible; safe only with an
  explicit lag/resync state or connection close.
- **Replay, catch-up, restart:** naturally compatible.
- **Complexity:** medium because the protocol must distinguish live delivery
  from required catch-up.
- **Invariant effect:** canonical history remains exact; live continuity becomes
  limited and must never be presented as guaranteed delivery.

Accepted only with an explicit transition. Silent notification loss is
rejected.

### E. Bounded queue, lag detection, disconnect, and reconnect/catch-up

- **Safety:** preserves canonical truth and makes the recovery boundary explicit.
- **Availability:** high for the host; overloaded clients reconnect.
- **Isolation:** strong when all per-connection and global limits are enforced.
- **Memory:** bounded for live output and concurrent catch-up; journal memory is
  a separate concern.
- **Global blocking risk:** low except at the authoritative admission boundary.
- **Client observation:** explicit slow-consumer or resync-required reason when
  deliverable; otherwise ordinary close with the same reconnect rule.
- **Replay, catch-up, restart:** fully aligned with durable client cursors.
- **Complexity:** medium to high, but decomposable.
- **Invariant effect:** can guarantee bounded live work and no silent recovery;
  catch-up availability remains limited by retention.

Accepted as the V0.2 overload contract.

### F. Bound or compact the journal with snapshots and retention

- **Safety:** unsafe if compaction deletes the only authoritative facts needed
  for audit or replay. Safe when a validated snapshot is a new replay base and
  an immutable archive is retained according to policy.
- **Availability:** improves restart and catch-up cost; clients behind the
  retention floor require a full resync.
- **Isolation:** reduces the cost an old client can impose.
- **Memory:** bounds the hot suffix; total durable archive size needs a separate
  policy.
- **Global blocking risk:** compaction itself must be incremental and must not
  stop commits indefinitely.
- **Client observation:** the server must expose the earliest available
  sequence and require snapshot resync when a cursor is older.
- **Replay, catch-up, restart:** compatible only after snapshot-plus-suffix
  replay is validated as authoritative.
- **Complexity:** high; it changes persistence, recovery, and audit semantics.
- **Invariant effect:** can bound hot retention, but arbitrary historical
  catch-up becomes impossible without an archive.

Deferred until snapshot-plus-suffix replay is implemented and verified. The
first overload slices must not destructively compact the canonical archive.

## Decision matrix

| Strategy | Safety | Availability / isolation | Memory | Observable recovery | Replay compatibility | Complexity | Decision |
|---|---|---|---|---|---|---|---|
| Global producer slowdown | Strong ordering, coupled delivery | Poor / none | Potentially bounded | Pending commands | Compatible | Low | Reject |
| Per-connection bounded queue | Strong if overflow is explicit | High / strong | Per-client bounded | Needs lag transition | Compatible | Medium | Adopt |
| Disconnect slow consumers | Canonical truth preserved | High / strong | Bounded with deadlines | Close then reconnect | Compatible | Medium | Adopt |
| Drop live notifications | Safe only when visible | High / good | Bounded | Explicit resync required | Compatible | Medium | Adopt conditionally |
| Queue + lag + reconnect | Strong and explicit | High / strong | Bounded except journal | Deterministic cursor catch-up | Compatible | Medium-high | Recommend |
| Snapshot + retention | Strong after validated replay base | High / good | Hot data bounded | Snapshot then suffix | Conditional | High | Defer, then adopt |

## Proposed V0.2 contract

### 1. Authoritative command admission

- Slow consumers never participate in the commit critical path.
- The host has an explicit global bound on connections, admitted commands, and
  concurrent catch-up work.
- A command rejected before admission receives `server_overloaded` and creates
  no canonical event or accepted-command receipt.
- Once admitted to authoritative sequencing, a command reaches a durable
  success or structured rejection independently of connection lifetime.
- Once durably committed, a command remains accepted even if its submitting
  connection closes before receiving an outcome. Existing `message_id`
  deduplication recovers that outcome.
- Disk exhaustion or persistence failure remains `persistence_failed`; it is
  not converted into a successful command.
- Limit values and admission timeouts are explicit configuration, not hidden
  scheduler behavior.

This bounds authoritative pressure through explicit admission or rejection,
not by waiting for every client to consume notifications.

### 2. Per-connection output

- Each connection owns one bounded data queue and one writer task.
- The queue is bounded by both message count and serialized bytes, using the
  exact encoded size or a conservative upper bound. Item count alone is
  insufficient because one catch-up page can be large.
- Canonical publication performs a non-blocking enqueue. It never awaits a
  connection's WebSocket sink.
- A reserved control path can cancel the writer and attempt a final
  protocol-level close even when the data queue is full.
- Every write and close attempt has a deadline. Deadline expiry aborts the
  connection task and releases its quota.
- Exceeding queue count, byte budget, or write deadline moves the connection
  exactly once from `live` to `resync_required`, then to `closed`.

### 3. Delivery and cursors

- `published`, `enqueued`, `written to a WebSocket sink`, `received`, `applied`,
  and `persisted by the client` are distinct states.
- The server does not claim end-to-end delivery and does not advance a
  client-confirmed cursor merely when it enqueues or writes an event.
- The recovery cursor is owned by the client and means **highest contiguous
  sequence applied and durably persisted locally**.
- On every connection, including an unexplained close, the client reconnects
  with that durable cursor. It may receive duplicates and must process them
  according to INV-006.
- The current server-side `sent_sequence` remains only a connection-local
  scheduling cursor. It is not a delivery acknowledgement.

### 4. Observable slow-consumer behavior

- The preferred protocol signal is
  `ResyncRequired { reason, current_sequence, earliest_available_sequence,
  snapshot_revision }`, followed by a WebSocket close with a stable SYTOG close
  code and short reason.
- Reasons include `outbound_queue_full`, `outbound_byte_budget_exceeded`,
  `write_timeout`, `catch_up_limit_exceeded`, and `cursor_before_retention`.
- Delivery of the final signal is best effort because a broken or blocked
  transport cannot acknowledge it. Therefore any transport close has the same
  safe client rule: persist the current replica, reconnect, and announce its
  durable applied cursor.
- The server never skips a range and then resumes live events on the same
  connection as though continuity had been preserved.

### 5. Catch-up

- Catch-up is paginated. Every page is bounded by event count and serialized
  bytes, and identifies `from_sequence`, `through_sequence`,
  `current_sequence`, and whether more pages remain.
- The server bounds concurrent catch-ups, pages per request, total events,
  total bytes, wall-clock duration, and write duration.
- A page always contains one contiguous canonical range. Page boundaries do
  not change replay results.
- Live notification delivery does not overtake unfinished catch-up on the same
  connection. The connection catches up to a captured high-water mark, then
  reconciles events committed after that mark before entering `live`.
- Exceeding a catch-up budget closes the connection with
  `catch_up_limit_exceeded`; the cursor is not advanced by the server.
- A client may reconnect and continue from the last page it durably applied.

### 6. Retention, snapshots, and replay

- The durable canonical archive and the hot catch-up window are separate
  concepts.
- The first implementation keeps the complete append-only archive and bounds
  only live queues, concurrent work, and page sizes.
- Before hot retention is bounded, SYTOG must implement and validate replay
  from `snapshot at N + contiguous suffix N+1..current`.
- After that milestone the server advertises an
  `earliest_available_sequence`. A cursor at or above the floor receives event
  pages. A cursor below the floor receives a compatible snapshot plus suffix,
  or `ResyncRequired` if no compatible snapshot can be served within limits.
- Destructive deletion of the only exact replay source is outside this ADR.
  Archive retention, export, and deletion require a separate policy decision.

### 7. Voluntarily slow or hostile clients

- Connection admission uses a global semaphore and may later add per-address
  quotas after identities and proxy trust are defined.
- Handshake, idle reads, writes, and catch-up have deadlines.
- Input messages, envelope bytes, catch-up frequency, and concurrent requests
  are bounded.
- A client cannot reserve unbounded output memory, retain a task forever, or
  force unlimited catch-up cloning.
- Repeated overload may be rate-limited, but policy must not alter canonical
  event history or silently suppress an accepted command outcome.

## Client-observable state machine

| State | Server behavior | Client-visible outcome | Required client action |
|---|---|---|---|
| `connecting` | Validate limits and durable cursor | `Hello` accepted, catch-up plan, or explicit rejection | Keep local cursor unchanged |
| `catching_up` | Send bounded contiguous pages | Page metadata and events | Validate, apply, persist, then request/accept next page |
| `live` | Enqueue bounded live batches | Ordered events while connection keeps up | Apply and persist contiguously |
| `resync_required` | Stop normal event enqueue; attempt notice and close | `ResyncRequired`, close reason, or unexplained close | Reconnect from last durably applied cursor |
| `full_resync` | Serve compatible snapshot plus bounded suffix | Snapshot revision and suffix plan | Validate snapshot identity, replace local base, persist, then apply suffix |
| `rejected` | Admit no work | Stable overload/error code | Retry with bounded backoff; do not assume a command committed |

For command submission, a transport close leaves the outcome unknown. The
client retries the same request with the same `message_id`; durable
deduplication returns the accepted result or allows a previously unaccepted
request to be evaluated.

## Invariant impact after full implementation

### Guaranteed

- A slow connection cannot block authoritative commit indefinitely.
- Every connection has explicit queue, byte, and write-time bounds.
- Queue overflow or write timeout cannot be hidden as continuous delivery.
- Catch-up pages are contiguous and bounded.
- A client resumes only from its own durable contiguous cursor.
- An accepted command remains recoverable by `message_id` after disconnect.
- Overload rejection before admission creates no canonical fact.

### Limited

- Catch-up is available only from the retained floor or a compatible snapshot.
- Availability under overload is limited: the host may reject commands or
  connections explicitly.
- End-to-end delivery cannot be guaranteed without client acknowledgements;
  the protocol guarantees deterministic recovery instead.
- Total durable storage remains unbounded until an archive-retention policy is
  chosen.

### Impossible under conflicting requirements

- Arbitrary historical catch-up, finite local retention, and no external
  archive cannot all be guaranteed simultaneously.
- A final close reason cannot be guaranteed to reach a peer whose transport is
  already blocked or broken.
- Strict bounded total memory is impossible while the complete journal,
  accepted-command index, and unbounded canonical state remain resident.

## Implementation plan

1. **Protocol vocabulary only.** Add stable overload reasons,
   `ResyncRequired`, paged `EventBatch` metadata, and tests for round-trip,
   unknown versions, contiguous ranges, and close semantics.
2. **Per-connection writer isolation.** Split reading from writing, add a
   bounded count-and-byte queue, write timeout, reserved shutdown path, and
   deterministic queue-overflow tests. Keep the complete journal.
3. **Connection and authoritative admission.** Add global connection,
   command-waiter, and catch-up semaphores with explicit rejection codes and
   cancellation tests. Preserve commit and deduplication semantics.
4. **Paged catch-up over the existing archive.** Introduce high-water marks,
   page limits, total catch-up budgets, and live-transition tests. Avoid
   cloning the whole suffix.
5. **Snapshot resync.** Make `StateSnapshot` operational, validate
   snapshot-plus-suffix replay, and add `earliest_available_sequence`
   negotiation. Still retain the archive.
6. **Hot-window retention.** Move the hot suffix out of the unbounded canonical
   `Vec`, segment durable storage, and serve pages without loading the complete
   archive. Keep exact archived replay.
7. **Retention policy.** Decide archive export, duration, byte quota, and
   deletion separately. Only then enable compaction that can remove local
   history.
8. **Operational hardening.** Add metrics and structured logs for queue
   occupancy, lag transitions, disconnect reasons, catch-up pages and bytes,
   admission rejections, write timeouts, and retention-floor resyncs.

Each slice must keep the previous protocol readable or explicitly bump the
protocol version. No slice may silently reinterpret an old cursor.

## Required tests

### Property and invariant tests

- Accepted command order and journal contents are independent of one stalled
  output queue.
- Queue occupancy and accounted bytes never exceed configured bounds.
- Exactly one transition to `resync_required` occurs per overloaded connection.
- No event after a dropped range is exposed as continuous live delivery.
- Every catch-up page is non-empty, contiguous, ordered, and within both count
  and byte limits.
- Concatenating all pages equals the canonical suffix exactly.
- Page size and timing do not affect the final reduced state.
- A high-water-mark handoff cannot lose or duplicate facts across
  `catching_up -> live`.
- Reconnecting from every cursor in a generated journal converges, or produces
  an explicit snapshot/full-resync requirement.
- Retrying a command with the same `message_id` after disconnect returns the
  durable accepted outcome without another append.
- Admission rejection produces no event, receipt, revision change, or
  broadcast.
- Snapshot plus retained suffix replays to exactly the same state and revision
  as the full archive.

### Load and failure scenarios

- One non-reading WebSocket client while healthy clients submit and receive.
- Many slow clients up to and beyond the connection quota.
- Oversized event and batch attempts against the byte budget.
- Writer blocked past its deadline.
- Broadcast lag during concurrent paged catch-up.
- Catch-up from the retention floor, one event before it, and sequence zero.
- Disconnect after enqueue, during write, after write, and after client apply
  but before local persistence.
- Host restart during every catch-up page boundary.
- Disk-full and append-failure injection while clients are overloaded.
- Reconnect storms with bounded backoff and catch-up concurrency.
- Long sessions demonstrating bounded hot memory and stable command latency.

## Human decisions required before implementation

1. Numeric defaults and configurability for:
   - maximum connections and command waiters;
   - per-connection messages and serialized bytes;
   - write, handshake, idle, and catch-up deadlines;
   - catch-up page events/bytes and total session events/bytes;
   - concurrent catch-ups and retry guidance.
2. Whether the first protocol change remains wire-compatible V1 or requires a
   protocol-version bump.
3. The stable protocol message, WebSocket close code, and retry semantics for
   slow-consumer and server-overload outcomes.
4. Whether clients explicitly acknowledge durable application, or whether the
   client-owned reconnect cursor remains the only confirmation.
5. What constitutes a compatible snapshot and whether full resync may discard
   locally retained event identity history.
6. The distinction between hot retention and audit archive, including archive
   export, retention duration, byte quota, and deletion authority.
7. Whether admission limits are per host, per session, per authenticated
   identity, or per network address once identity and proxy trust exist.

## Consequences

The authoritative path remains stable when one client is slow, while the host
may still reject new global work explicitly to protect itself. Clients gain a
simple safe rule: persist only contiguous applied facts, never infer delivery
from connection lifetime, and reconnect from that cursor after any close.

The design adds protocol states, queue accounting, deadlines, and operational
limits. It intentionally accepts disconnection and retry as normal flow. Exact
replay remains the authority; live delivery is a bounded optimization.

Journal compaction is not an immediate backpressure fix. It becomes safe only
after snapshot-plus-suffix replay and archive policy are separately validated.
