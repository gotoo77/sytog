# ADR 0007: V0.1 activity and capability boundaries

Status: accepted

The first vertical slice proved two accidental specializations. V0.1 therefore:

- keeps closed enums for generic session rules;
- routes activity commands/events through a versioned opaque envelope;
- implements `demo.counter` outside the core through a minimal `ActivityEngine`;
- identifies and evaluates concrete capability offers rather than only nodes;
- uses typed LLM and CPU contract families;
- scopes observations and availability to offer ids;
- publishes a V1 score breakdown;
- versions logs and snapshots independently from domain state.

This is not a dynamic plugin system. New contract enum variants remain deliberate
compile-time changes until several real families justify a schema registry.

