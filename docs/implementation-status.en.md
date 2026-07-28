[Français](implementation-status.md) | [English](implementation-status.en.md)

# Implementation status

## Implemented and executable

- typed session, participants, lifecycle, authority, activity envelopes, and
  revision;
- create, join, activity, and transfer commands with structured rejections;
- `demo.counter` and `demo.vote` isolated behind `ActivityEngine`;
- immutable sequenced events, pure reduction, atomic application, and
  deterministic replay;
- versioned snapshots, journals, and protocol envelopes;
- LLM and CPU contracts, inventory, policies, availability, offer-scoped
  observations, and explainable matching scores;
- single-authority WebSocket host and multi-process clients;
- accepted-event broadcast and catch-up from a known sequence;
- canonical JSON Lines journal synchronized before memory commit and broadcast;
- host reconstruction from the journal after restart;
- CLI façade for demos, validation, replay, matching, `serve`, and `connect`;
- narrow Wasm façade for matching;
- CI for formatting, Clippy, tests, and Wasm compilation.

## Designed but not implemented

- durable command deduplication and automatic recovery of a partially written
  final JSONL line;
- network snapshots and journal compaction;
- application TLS, cryptographic identity, signatures, and private projections;
- LAN discovery, WebRTC, NAT traversal, consensus, and multiple authorities;
- remote job reservation, execution, and cancellation;
- Observatory persistence and provenance;
- Noema, Delibra, FFF, games, Media Sync, and TypeScript implementations;
- generated Wasm/TypeScript package.

The V0 matcher trusts supplied declarations, policies, availability, and
observations. It demonstrates the model; it is not safe authorization for real
resource execution.
