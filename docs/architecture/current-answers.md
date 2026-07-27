# Current architectural answers

- **Session:** an event-sourced coordination context with identity, lifecycle,
  participants, logical authority, optional activity, and monotonic revision.
- **Participant:** a logical actor id and presentation metadata; it is not
  assumed to be a human, device, or connection.
- **Activity:** a versioned adapter whose typed commands produce events and
  state through an opaque session envelope. `demo.counter` is an example outside
  the generic domain/runtime.
- **Capability:** a declared functional contract a node can be asked to satisfy,
  not its hardware inventory.
- **State ownership:** the session log is authoritative; each participant may
  reconstruct a replica. No UI or server inherently owns it.
- **Validation and ordering:** the current logical authority validates commands
  and assigns event sequences.
- **Replay:** start from the known initial state or a compatible snapshot and
  apply the ordered suffix exactly once.
- **Reconnection:** exchange session id and last revision, then receive a
  snapshot plus suffix or only the missing suffix. The exchange is Phase 1.
- **Authority transfer:** an authorized command emits an immutable transfer
  event to an existing participant. Automatic election is out of scope.
- **Protocol versions:** the envelope carries family and numeric version;
  unknown versions are rejected. Stable JSON fixtures guard compatibility.
- **Activity versions:** stable activity id plus semantic version. An adapter
  negotiates compatibility; V0 requires exact known behavior.
- **Existing games:** retain their language and expose commands, events, and
  snapshots through an FFF adapter; extract business logic only when valuable.
- **FFF boundary:** FFF owns rooms UX, invitations, lobby, product navigation,
  and screens. SYTOG owns generic coordination semantics.
- **Noema boundary:** Noema knows engines, models, and invocation. SYTOG sees an
  `llm.inference` offer and coordinates its use.
- **Delibra boundary:** Delibra defines deliberation workflows and artifacts;
  SYTOG discovers resources and coordinates execution.
- **Observatory boundary:** Observatory stores and analyzes empirical traces.
  The deterministic core only consumes explicit observation inputs.
- **Inventory vs capability:** inventory describes possessions; capability
  describes a callable outcome.
- **Declared vs observed vs available:** promise, empirical history, and present
  executable capacity are separate data with separate matching roles.
- **Exposure policy:** evaluate it as a hard local gate before availability and
  score; manual consent must be explicitly granted.
- **Matching explanation:** every concrete node/offer result has status, stable
  reasons, and a V1 breakdown of success, latency, headroom, and final score.
- **Preventing non-consensual use:** policy rejection makes a node ineligible;
  future execution must re-check policy locally before reservation and launch.
- **Testing without network:** pure decisions, reducer/replay tests, stable
  fixtures, and deterministic simulated offers cover the core invariants.
