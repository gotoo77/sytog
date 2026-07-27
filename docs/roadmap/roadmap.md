# Roadmap

## Phase 0 — foundation (this repository)

Pure domain, V0 envelope, decision/reduction, replay/snapshot, demo activity,
functional offers, policy, availability, observations, deterministic matching,
CLI, fixtures, Wasm boundary, and threat model.

## Phase 1 — local multi-client session

Add memory transport/storage adapters because a simulation needs them. Implement
deduplication, reconnect handshake, snapshot suffix negotiation, and authority
absence/transfer scenarios.

## Phase 2 — first FFF web shell

Generate the Wasm package, add a TypeScript boundary wrapper, lobby, participant
views, and one demo activity. Keep browser permissions outside Rust.

## Phase 3 — simple network

Add authenticated WebSocket signaling/transport, presence leases, persistence,
payload limits, and recovery. Preserve the same command/event contracts.

## Phase 4 — P2P

Add WebRTC DataChannel with relay/fallback, LAN discovery where appropriate,
identity keys, trust policy, and adversarial simulations.

## Phase 5 — first real game

Integrate GOTUS or PuzzleGuess through an adapter, public/private projections,
and reconnectable snapshots. Let evidence shape the reusable activity API.

## Phase 6 — distributed execution

Add proposal, local revalidation/consent, reservation, progress, cancellation,
result validation, release, and multidimensional observations. Sandbox first.

## Phases 7–9

Connect Noema capability publication/execution; implement Media Sync around an
abstract player and temporal intention; then enrich FFF with social features.

Consensus, generic CRDTs, migration, federation, and marketplaces remain
evidence-driven options rather than promised architecture.

