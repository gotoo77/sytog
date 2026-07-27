# Vertical slice scenarios

## Session

1. Alice creates `demo-session` and becomes authority.
2. Bob joins while the session is open.
3. Bob attempts to start the activity and is explicitly refused.
4. Alice starts `demo.counter@1.0.0`.
5. Bob increments the counter by three.
6. Alice transfers authority to Bob.
7. The journal is replayed from revision zero and equals the live state.

## Capability matching

For the fixture French streaming `qwen3:4b` job:

- `node-a` is compatible and receives an observation-informed score;
- `node-b` is rejected because the model is absent;
- `node-c` is rejected by local-network policy;
- `node-d` satisfies contract and policy but is currently saturated.

The matcher never turns `node-c` into a candidate because it is fast, and never
calls `node-d` executable because it is declared.

