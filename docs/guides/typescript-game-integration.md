# Integrate an existing TypeScript game

Keep the game in TypeScript. An FFF-side adapter translates between its existing
API and a versioned activity contract:

```text
game UI / engine
      ↕
FFF adapter (commands, events, snapshots)
      ↕
serialized Wasm or network boundary
      ↕
SYTOG activity rules
```

Start by embedding or launching the game. Map only the coordination seam:
players, start/stop, submitted actions, public results, and a reconnectable
snapshot. The adapter must reject unknown protocol/activity versions and must
not treat local UI state as authoritative shared state.

Extract a language-neutral game engine only if replay, server validation, or
reuse justifies it. A rewrite in Rust is not an integration requirement.

