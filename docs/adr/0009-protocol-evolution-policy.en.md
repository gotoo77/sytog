Canonical source: French

Source document: [Français](0009-protocol-evolution-policy.md)

[Français](0009-protocol-evolution-policy.md) | [English](0009-protocol-evolution-policy.en.md)

# ADR 0009: Protocol evolution policy

Status: proposed

This decision defines negotiation, activation, compatibility, and lifecycle
rules for SYTOG network protocol versions. It does not change the server: V1
remains the only active version and V2 remains defined but inactive.

## Context

[ADR 0004](0004-versioned-protocol-and-polyglot-activities.md) requires an
explicit family and version on every envelope. [ADR
0008](0008-overload-and-backpressure-contract.md) introduced V2 vocabulary
without activating it. The code now distinguishes:

- `LATEST_PROTOCOL_VERSION = 2`, the most recent defined version;
- `ACTIVE_SERVER_PROTOCOL_VERSION = 1`, the version currently emitted by the
  existing server and client.

This separation currently prevents implicit activation, but it is insufficient
once several versions must coexist. A contract is still needed to advertise
capabilities, select a common version, evolve the server preference, and retire
an old version.

The first application message cannot be a neutral negotiation format: `Hello`
is already versioned and has different shapes in V1 and V2. Decoding `Hello`
before selecting its protocol would recreate exactly the ambiguity that
versioning is intended to remove.

Network protocol versions are independent of journal, snapshot, and activity
schema versions. Evolving one of those formats does not automatically change
the network protocol version.

## Goals

- select exactly one version before any application message;
- allow V1 and V2 to coexist on the same WebSocket endpoint;
- preserve V1 clients during an explicitly bounded migration;
- make incompatibilities and transitions observable;
- prevent `LATEST_PROTOCOL_VERSION` from participating in activation;
- limit the number of simultaneously active and tested network paths;
- make every deprecation and retirement intentional and reversible before
  compatibility code is deleted.

## Strategies considered

### A. WebSocket subprotocol negotiation

The client offers `Sec-WebSocket-Protocol` tokens such as `sytog.v2` and
`sytog.v1`. The server selects a token from the intersection with its activated
versions and returns it in the HTTP `101` response.

- the version is fixed before `Hello`;
- the mechanism is standard and available to native and browser WebSocket
  clients;
- one endpoint can host several versions;
- no additional application-level “V0” format is needed;
- failure happens before application-session admission;
- browsers may expose only handshake failure rather than the HTTP response
  body;
- current V1 clients, which offer no subprotocol, require an explicit
  transition rule.

### B. Neutral negotiation message before `Hello`

The WebSocket connection is first accepted, then a new unversioned message
exchanges version lists.

- the server can return a structured error over the WebSocket;
- a bootstrap mini-protocol must be created, versioned, and secured;
- another state, parser, size limit, and timeout are required before the normal
  protocol;
- old clients immediately send V1 `Hello` and still require special detection.

### C. Version list inside `Hello`

- the transport remains unchanged;
- the server must know which `Hello` shape to decode before it knows the
  version;
- V1 and V2 already use different cursor fields;
- an error or extension could silently reinterpret `Hello`.

This strategy is circular and does not provide a clean version boundary.

### D. One endpoint per version

Paths such as `/v1` and `/v2` select the version before the handshake.

- selection is simple and explicit;
- no bootstrap protocol is required;
- every version adds routing, configuration, and operational documentation;
- discovery and migration require different URLs;
- endpoints may diverge in quotas, security, or deployment.

This remains a fallback if future infrastructure cannot preserve WebSocket
subprotocol headers.

### E. Try the highest version, then reconnect with fallback

- the server needs no selection mechanism;
- every incompatibility costs at least one extra connection;
- fallback depends on error ordering and can hide misconfiguration;
- an intermediary can induce downgrade;
- behavior becomes harder to explain under overload.

Implicit automatic fallback conflicts with SYTOG's determinism goal.

## Decision matrix

| Strategy | Before `Hello` | Same endpoint | V1 compatibility | Determinism | Complexity | Decision |
|---|---:|---:|---:|---:|---:|---|
| WebSocket subprotocol | Yes | Yes | Explicit transition | Strong | Low to medium | Adopt |
| Neutral message | Yes, after upgrade | Yes | Special detection | Strong | Medium to high | Reject |
| List in `Hello` | No | Yes | Ambiguous | Weak | Superficially low | Reject |
| Endpoint per version | Yes | No | Simple | Strong | Medium operationally | Fallback |
| Reconnect with fallback | After failure | Yes | Possible | Weak | Medium client-side | Reject |

## Decision

### Negotiation

Negotiation takes place during the WebSocket HTTP handshake, before any SYTOG
envelope and before `Hello`.

The client offers the set of network versions it supports using subprotocol
tokens:

- `sytog.v1` for V1;
- `sytog.v2` for V2;
- future versions use `sytog.vN`.

A SYTOG version token is canonical when it matches exactly `sytog.vN`, where
`N` is a strictly positive decimal integer without a leading zero. Offer
processing separates three cases:

1. an invalid WebSocket header or a token claiming the `sytog.v` prefix without
   matching this grammar makes the whole offer malformed; the handshake is
   rejected with the stable `invalid_protocol_offer` code and no partial
   selection occurs;
2. a syntactically valid but unknown, unsupported, or inactive token does not
   participate in the intersection; it does not invalidate another common
   version;
3. after normalization, an empty intersection produces
   `no_common_protocol`.

Syntactically valid duplicates are normalized into a set. They are not
rejected, do not change preference, and give no additional weight to a version.

Thus the `sytog.v3, sytog.v2` offer selects V2 when V2 is supported and
activated, even if V3 is unknown, unsupported, or inactive. If neither version
is common, the result is `no_common_protocol`, not
`invalid_protocol_offer`.

An absent header is not a malformed header: it follows only the V1 legacy rule
below. A present but empty or syntactically invalid header produces
`invalid_protocol_offer`.

Client token order does not express preference. A client that refuses a version
does not offer it. The server computes the intersection of:

1. versions offered by the client;
2. versions supported by the binary;
3. versions activated in host configuration.

The server then walks its configured preference order and selects the first
common version. This function is pure, deterministic, and independent of client
token order. The server never selects a version that was not offered, is not
supported by the binary, or is not activated.

For an explicit offer, the `101 Switching Protocols` response returns exactly
the selected token. That version is immutable for the entire connection. Every
subsequent envelope must carry the same version and is decoded only by its
dedicated decoder. A mismatch closes the connection as a protocol error; it
does not trigger fallback.

Ignoring an offered but unselected capability does not contradict ADR 0004: no
envelope using that version is accepted. An unknown version without a common
version fails explicitly at the handshake, and any envelope whose version
differs from the selected version then fails explicitly.

### Transitional V1 compatibility

During the initial migration only, absence of `Sec-WebSocket-Protocol` may be
configured as an implicit offer of the single `sytog.v1` token. This mode:

- is valid only while V1 is activated;
- can never select V2;
- returns a `101` response without a subprotocol header, because the legacy
  client offered none, while pinning V1 internally;
- is exposed in metrics and logs as `legacy_v1`;
- has a distinct configuration switch;
- is removed when V1 is retired.

Thus an old client cannot activate V2 accidentally, and future absence of an
offer never means “select the latest version.”

### No common version

If the intersection is empty, the server rejects the handshake before creating
an application session. It emits no SYTOG envelope, no version-dependent close
frame, and no authoritative effect.

The response is `400 Bad Request`, with
`Content-Type: application/problem+json`, a
`SYTOG-Supported-Protocols` header containing activated tokens in server
preference order, and a body whose stable machine-readable code is
`no_common_protocol`. A malformed offer uses the same HTTP boundary with the
`invalid_protocol_offer` code. The response does not reflect malformed values
supplied by the client.

The HTTP status, header, and `application/problem+json` body are diagnostic
enrichment. The client's only functional dependency is the handshake result: a
`101` response with a token it offered, or failure. A client, retry policy, or
safety decision must not depend on being able to read or parse the body because
a browser API may expose only connection failure. Details supplement logs and
telemetry but trigger no automatic fallback. The complete JSON problem schema
is fixed in the negotiation vocabulary slice before any network
implementation.

No client automatically tries a version it did not originally offer. Retrying
with a different offer is an explicit client policy.

## Formal version states

| State | Definition |
|---|---|
| Defined | Schema, semantics, documentation, fixtures, and validation exist. `LATEST_PROTOCOL_VERSION` names only the greatest defined version. |
| Supported by the binary | The binary contains the encoders, decoders, handlers, and conformance tests required for end-to-end operation. It may remain disabled. |
| Activated by the server | Host configuration allows this version for new connections. The activated set is a subset of the supported set. |
| Preferred | First version in the server's selection order among common activated versions. It must be activated and supported. |
| Deprecated | Still supported and potentially activated, but its retirement is announced and observed. It is never retired in the same normal release in which it is first deprecated. |
| Retired | Rejected for all new connections and absent from the activated set. Its decoder may remain in the binary for fixtures, diagnostics, or historical migrations. |

Deleting code for a retired version is a separate step. It requires evidence
that no persistent format or migration tool still depends on it.
The presence of **retained historical code** is an orthogonal property, not
another network state: a decoder alone does not make a version supported,
activated, or selectable. Such code is not part of the end-to-end supported set
and does not count toward the active network-version window.

When this ADR is adopted:

- V1 is defined, supported, activated, and preferred; it is not deprecated;
- V2 is defined and its boundary types are available in the transport library,
  but it is not yet supported end to end by the server, activated, or
  preferred;
- no version is deprecated or retired.

In the current implementation, `ACTIVE_SERVER_PROTOCOL_VERSION` remains the V1
scalar used by the legacy path. Future negotiation work must introduce an
explicit activated set and preference order; it must not redefine that scalar
as an alias of `LATEST_PROTOCOL_VERSION`.

## Compatibility window

A server activates at most two consecutive major network versions. This limit
applies only to versions accepted for new connections on one host; it counts
neither merely defined versions nor decoders retained in offline tools. When
V`N` is activated, V`N-1` remains activated for at least one complete normal
release. A later release may announce its deprecation; retirement occurs only
in another later release.

V1 is not deprecated automatically by the existence or future activation of
V2. Deprecating and then retiring V1 require separate human decisions,
supported by usage telemetry and migration notes.

An urgent security or integrity fix may shorten this window. The exception
requires a documented decision, an explicit error for affected clients, and a
rollback procedure where rollback remains safe. It may immediately reduce the
activated set, but cannot exceed the two-version limit or make an unsupported
version selectable. Historical decoders and offline tools remain outside this
limit.

The server does not promise to support every defined version. Offline tools may
retain more decoders when replay, archives, or migrations require them.

## Compatibility and new major versions

A new major version is required when a peer conforming to the current version
could misdecode, misinterpret, or silently apply the new behavior, including:

- removing or renaming a message, field, tag, reason, or stable code;
- adding a required field or changing its type;
- changing the meaning of a field, cursor, acknowledgement, code, or terminal
  state;
- changing ordering, delivery, idempotency, replay, authority, or recovery
  guarantees;
- emitting a new message or enum variant that an existing peer cannot safely
  ignore;
- changing negotiation, authentication, or the admission boundary
  incompatibly;
- accepting the same representation as valid with different semantics.

The following remain compatible within one version:

- clarifications that change no observable behavior;
- fixes that reject only data already invalid under the published contract;
- adding a truly optional field with a safe default when old readers ignore it
  and new readers accept its absence;
- internal optimizations that preserve the observable contract exactly;
- configured quota or timeout value changes when units, contractual bounds,
  and protocol outcomes remain unchanged.

Without a finer-grained capability mechanism, a new message, reason, or variant
that may be emitted is considered incompatible even when its JSON
representation is additive.

## Lifecycle

1. **Define.** Document the contract and incompatibilities, add
   version-specific types, fixtures, and tests, then advance
   `LATEST_PROTOCOL_VERSION`. The activated set does not change.
2. **Support.** Integrate encoders, decoders, and handlers into the binary
   without making them selectable. Cross-version tests prove isolation.
3. **Activate without preferring.** Explicitly add the version to selected host
   configurations. The old version remains preferred; only clients offering
   solely the new version select it.
4. **Prefer.** After compatibility, load, recovery, and observability
   validation, explicitly change the preference order. This operation has a
   configuration rollback.
5. **Deprecate.** Publish migration notes, instrument remaining use, and
   announce a retirement date or criterion.
6. **Retire.** Remove the version from the activated set and disable its legacy
   mode. Preserve offline decoding while needed.
7. **Delete code.** Make a separate decision and migration if the historical
   decoder is no longer needed.

Every state change is explicit in reviewed configuration or code. A release may
define a version without operationally supporting it, and a binary may support
it without any server activating it.

## Required tests

Before any activation:

- table and property tests for intersection and preference order;
- `activated ⊆ supported` and `preferred ∈ activated` invariants;
- separate rejection of present but empty or malformed offers with
  `invalid_protocol_offer`;
- duplicate normalization and proof that duplicates do not change selection;
- selection of a common version despite valid unknown, unsupported, or inactive
  tokens in the same offer;
- `no_common_protocol` failure when no common version remains, without session
  creation or authoritative effect;
- proof that selection is independent of client offer order;
- proof that the server never selects a version not offered;
- explicit tests for absent/enabled/disabled legacy behavior;
- selected version immutability throughout the connection;
- cross-rejection of V1 by the V2 decoder and V2 by the V1 decoder;
- version-specific fixtures and round trips;
- rolling-upgrade tests with N and N-1 clients and servers;
- preference rollback tests from N to N-1;
- an architectural test proving that changing only
  `LATEST_PROTOCOL_VERSION` changes neither the activated set nor selection.

Each activated version runs the same conformance, reconnect, replay, and error
scenarios. A transition to V2 additionally runs the ADR 0008 overload and
resynchronization scenarios, without removing V1 scenarios while V1 remains
active.

## Required observability

At startup, the server logs supported, activated, preferred, and deprecated
versions and the V1 legacy-mode status.

Handshake metrics count:

- explicit offers, legacy offers, malformed offers, and offers containing valid
  unknown tokens;
- selected version;
- rejections by reason, including `invalid_protocol_offer` and
  `no_common_protocol`;
- connections using a deprecated version.

Structured logs record the selected version and rejection cause without
including application payloads or creating high-cardinality labels. Alerts and
dashboards must show remaining use before retirement and compare errors,
reconnections, and latency by version.

## Guards against implicit activation

- `LATEST_PROTOCOL_VERSION` is never a connection, configuration, or selection
  default.
- Neither client nor server generates an offer, supported set, activated set,
  or preference order as the `1..=LATEST_PROTOCOL_VERSION` range or from its
  maximum.
- Selection receives the activated set and preference order explicitly.
- Supported and activated sets use distinct concepts validated at startup.
- The server node does not depend on `LATEST_PROTOCOL_VERSION`.
- Adding `PROTOCOL_VERSION_VN` or advancing `LATEST` changes no network path
  without a separate activation-configuration change.
- CI includes a test where `LATEST` is greater than the preferred version and
  verifies that selection remains unchanged.
- Every activated-set or preference-order change is separately visible in the
  diff and release notes.

## Consequences

Negotiation is explicit before the application protocol, and one connection
never mixes V1 and V2. The same endpoint can support a migration without
multiplying routes. Existing clients remain compatible through a bounded and
observable V1 legacy mode.

The server will need to customize the WebSocket handshake and maintain an N/N-1
test matrix. Detailed handshake errors will not always be visible from a
browser; server telemetry and a generic client error remain necessary.

The policy favors controlled migration over automatic activation of the latest
version. It accepts the temporary cost of two active network paths but refuses
to maintain more than two.

## Proposed implementation slices

These slices are ordered, but none is started by this ADR:

1. **Negotiation vocabulary.** Fix tokens, HTTP rejection,
   `invalid_protocol_offer`, `no_common_protocol`, and configuration types
   without changing the handshake.
2. **Pure selector.** Implement and test intersection, configuration
   invariants, and preference order, still without network activation.
3. **V1-only server handshake.** Select `sytog.v1` when present in a valid
   offer, ignore other valid tokens for intersection, preserve configurable V1
   legacy mode, reject malformed offers or offers without a common version, and
   add telemetry.
4. **V1 client advertisement.** Make maintained clients explicitly offer
   `sytog.v1` without changing payload or behavior.
5. **Dormant V2 support.** Integrate V2 handlers and let the handshake recognize
   its token while keeping the activated set at `{V1}` and V1 preferred.
6. **Controlled V2 activation.** Activate V2 on test hosts, preserve V1, and run
   the ADR 0008 tests required by newly observable behavior.
7. **V2 preference.** Change preference separately after human validation and
   retain rollback to V1.
8. **V1 deprecation then retirement.** Two later, separate decisions guided by
   telemetry and client migrations.

ADR 0008 slice 2 must not be conflated with these steps. Its V2 behavior becomes
observable only after explicitly validated negotiation and activation.

## Deferred questions

- complete handshake-rejection JSON schema beyond the stable code;
- source and format of activated-version and preference configuration;
- minimum calendar duration of deprecation in addition to the release rule;
- a possible fine-grained capability mechanism within one version;
- offline-tool compatibility policy and retention duration for retired
  decoders;
- exact exception procedure for urgent security retirement.
