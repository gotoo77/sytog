Canonical source: French

Source document: [Français](0003-commands-events-effects-and-authority.md)

[Français](0003-commands-events-effects-and-authority.md) | [English](0003-commands-events-effects-and-authority.en.md)

# ADR 0003: Commands, events, effects, and initial authority

Status: accepted

Commands are fallible intentions. Accepted facts are immutable, ordered events.
Reducers alone change state. External work is described as effects. The session
creator is initial logical authority and may transfer authority manually.

V0 deliberately excludes leader election and consensus. Logical authority is
not a network peer, server, display, or machine owner.
