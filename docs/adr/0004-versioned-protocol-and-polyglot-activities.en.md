Canonical source: French

Source document: [Français](0004-versioned-protocol-and-polyglot-activities.md)

[Français](0004-versioned-protocol-and-polyglot-activities.md) | [English](0004-versioned-protocol-and-polyglot-activities.en.md)

# ADR 0004: Versioned protocol and polyglot activities

Status: accepted

All boundary messages carry protocol family and version. Unknown versions fail
explicitly. JSON is the V0 boundary format and fixtures are compatibility
contracts; internal Rust types are not designed around arbitrary JSON.

Activities use stable ids and versions. Existing games integrate through
adapters and need not be rewritten in Rust.
