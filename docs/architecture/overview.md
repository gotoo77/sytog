# Architecture V0

## Diagnostic and decisions

The initial repository contained only the mission text and no Git repository,
source code, or toolchain. V0 therefore starts from a clean boundary rather than
preserving a legacy design.

The dependency direction is:

```text
CLI / Wasm / future adapters
          ↓
runtime / capabilities
          ↓
domain / protocol boundary
```

The domain has no clock, randomness, network, filesystem, thread, or async
runtime. A command carries its actor, causation id, and expected revision.
Session commands and events remain closed enums. Activity commands and events
cross a versioned envelope whose JSON payload is opaque to the session core.
An `ActivityEngine` adapter validates and reduces that payload. `demo.counter`
implements this seam in a separate example crate.

`decide` validates without mutation and returns facts plus requested external
effects. `apply` is the sole state transition. Decisions are applied to a clone
and committed only after every event succeeds.

The V0 authority is the creator. It validates commands and assigns monotonically
increasing event sequences. Transfer is explicit and event-sourced. This is an
authority model, not consensus.

Capability matching keeps six facts separate:

1. hardware inventory explains physical constraints;
2. declared capability states a functional contract;
3. exposure policy expresses sovereign consent and limits;
4. current availability says whether it can run now;
5. observations report historical behavior;
6. the job describes an abstract need.

Each concrete offer has an id and a typed contract. V0.1 proves two distinct
families: LLM inference and CPU compute. Observations target an offer rather
than a whole node. Hard contract and policy failures are `rejected`; transient
capacity failures are `unavailable`; only executable offers are `compatible`.
Ranking uses offer-scoped observations and contract headroom, then node and
offer ids as stable tie-breakers. The result exposes a versioned score breakdown.

## Vertical slice

The session demo creates a session, joins Bob, refuses Bob's unauthorized start,
accepts Alice's start, increments the demo activity, transfers authority, takes
a snapshot, exports events, and verifies exact replay.

The capability demo loads four simulated nodes and explains a compatible node,
a missing model, forbidden locality, and saturation. It does not execute jobs.

## Immediate invariants

- Every accepted event has exactly the next sequence.
- A rejected command cannot mutate state.
- State is reconstructed only by ordered events.
- Command revision conflicts are explicit; no silent merging occurs.
- Local exposure policy is a hard gate and cannot be improved by scoring.
- Availability never changes a declaration or policy.
- Equal matching inputs produce equal ordering.
- Protocol and activity versions are explicit strings/numbers from V0.

## Assumptions

- V0 identifiers are opaque caller-provided strings; authenticated keys come
  later.
- One logical authority orders a session at a time.
- Event log storage is an adapter concern; V0 uses JSON files and memory.
- Snapshot compatibility is exact within V0; migrations precede a V1 break.
- Network locality is a trusted simulation input in V0, not a security proof.
- Observations influence rank but never bypass contract, consent, or availability.

## Over-design risks avoided

No generic plugin framework, universal activity trait, network abstraction,
distributed consensus, CRDT, scheduler, execution sandbox, database layer, or
all-purpose resource cost scalar exists yet. Each would freeze guesses before
the first product integration provides evidence.
