Canonical source: French

Source document: [Français](0002-pure-rust-core-and-web-boundary.md)

[Français](0002-pure-rust-core-and-web-boundary.md) | [English](0002-pure-rust-core-and-web-boundary.en.md)

# ADR 0002: Pure Rust core and serialized web boundary

Status: accepted

Durable rules use Rust without I/O, async runtime, global clock, or implicit
randomness. TypeScript owns browser UI and platform APIs. Wasm exposes a narrow
serialized façade rather than internal Rust layouts.

Inputs that can vary must be supplied explicitly, which keeps replay and native
or browser behavior aligned.
