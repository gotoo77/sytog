# Implementation status

## Implemented and executable

- typed session, participant, lifecycle, authority, activity envelope, revision;
- create/join/start/stop/route/transfer commands with structured refusals;
- `demo.counter` isolated as an example `ActivityEngine`;
- immutable sequenced events, pure reducer, requested effects;
- versioned snapshot/log, unique event ids, atomic apply, deterministic replay;
- versioned V0 envelope validation and stable fixtures;
- typed LLM and CPU contracts over a generic concrete offer;
- separate inventory, offer, policy, availability, and offer-scoped observations;
- hard, explainable per-offer matching and versioned deterministic score;
- CLI demos, replay, validation, matching, and JSON output;
- narrow Wasm capability-matching function;
- CI workflow for format, Clippy, tests, and Wasm compilation;
- unit/invariant tests for rejection immutability, replay, sequence, policy,
  saturation, protocol version, and deterministic ranking.

## Designed but not implemented

- durable storage and transport adapters;
- message deduplication and reconnect exchange;
- real multi-client simulation and authority-failure recovery;
- cryptographic identity, signatures, trust, and private projections;
- job reservation/execution/cancellation and resource enforcement;
- Observatory persistence and provenance;
- Noema, Delibra, FFF, game, Media Sync, and TypeScript implementations;
- generated Wasm/TypeScript package.

The V0 matcher trusts supplied locality, declarations, policy, availability, and
observations. It demonstrates the model; it is not safe authorization for real
resource execution.
