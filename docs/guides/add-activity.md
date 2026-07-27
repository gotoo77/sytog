# Add an activity

V0.1 exposes only the minimal `ActivityEngine` seam. Add the smallest typed
command, event, and state needed by a real activity:

1. assign a stable id and semantic version;
2. define commands as intentions and validate actor permissions and lifecycle;
3. define immutable, serializable events;
4. reduce every event without I/O, time, or randomness;
5. make nondeterministic context explicit in the command;
6. define public/private projections outside secret-bearing shared state;
7. add replay, rejected-command, sequence, and snapshot-plus-suffix tests;
8. add stable fixtures before changing a published schema.

Implement `descriptor`, `initial_state`, and `decide`; translate at the envelope
boundary. Split an activity into its own crate only when it has independent
behavior and a consumer. Do not add transport or UI logic to its reducer.
