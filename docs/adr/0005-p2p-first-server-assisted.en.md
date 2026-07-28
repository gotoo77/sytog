Canonical source: French

Source document: [Français](0005-p2p-first-server-assisted.md)

[Français](0005-p2p-first-server-assisted.md) | [English](0005-p2p-first-server-assisted.en.md)

# ADR 0005: P2P-first, server-assisted

Status: accepted

The architecture must permit direct peers and LAN/self-hosted use while allowing
signaling, relay, and WebSocket fallback. No transport is privileged in the
domain, and V0 implements none because an in-memory deterministic slice is
enough to test semantics.
