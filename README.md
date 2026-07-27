# SYTOG

> SYTOG is a portable distributed runtime for synchronized sessions, activities
> and capabilities.

This repository contains a deliberately small V0 proving two independent paths:

- a deterministic session core: validated commands → immutable events → reducer,
  snapshot, journal, and replay;
- a functional capability registry: inventory, declarations, local exposure
  policy, current availability, observations, and explainable deterministic
  matching.

SYTOG is not FFF, a game engine, an AI engine, or a cluster manager. Friends Fun
Factory is a product above SYTOG; GOTUS and PuzzleGuess integrate through
polyglot adapters; Noema implements AI capabilities; Delibra defines cognitive
workflows; Observatory/Probe owns empirical history.

## Repository

```text
crates/
  sytog-domain/        durable session types and reducer
  sytog-protocol/      versioned boundary envelope
  sytog-runtime/       pure decision, effects, replay, snapshot
  sytog-demo-counter/  example activity outside the generic core
  sytog-capabilities/  offers, policy, availability, matching
  sytog-cli/           local demonstrations and file operations
  sytog-wasm/          narrow serialized browser façade
fixtures/              stable V0 protocol, log, job, and nodes
docs/                  architecture, ADRs, guides, threat model, roadmap
```

No empty transport or storage crates are present. Those adapters should appear
only when a real scenario needs them.

## Develop

Rust 1.85.1 is pinned.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p sytog-wasm --target wasm32-unknown-unknown
cargo run -p sytog-cli -- demo session
cargo run -p sytog-cli -- demo capabilities
cargo run -p sytog-cli -- --json capability match \
  fixtures/capabilities/job.json fixtures/capabilities/nodes.json
cargo run -p sytog-cli -- --json capability match \
  fixtures/capabilities/job-cpu.json fixtures/capabilities/nodes.json
cargo run -p sytog-cli -- replay fixtures/session/demo-event-log.json
cargo run -p sytog-cli -- validate fixtures/protocol/envelope-v1.json
```

## Status

Implemented means local, deterministic, in-memory behavior. Networking,
durability adapters, cryptographic identity, reconnection exchange, execution,
Media Sync, and product UIs remain explicitly conceptual. See
[`docs/implementation-status.md`](docs/implementation-status.md).
