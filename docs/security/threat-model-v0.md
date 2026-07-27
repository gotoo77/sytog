# Threat model V0

## Assets and trust boundaries

Assets are session authority, event-log integrity, participant-private data,
machine resources, prompts/results, and capability truthfulness. Network input,
remote identities, declarations, availability, observations, and job payloads
are untrusted.

## Addressed now

- strict typed deserialization and explicit unknown protocol rejection;
- optimistic revision checks and ordered event application;
- explicit permission and lifecycle failures;
- local exposure policy, requester/locality/memory limits, and consent gate;
- no domain I/O or arbitrary code execution;
- rejected commands leave state unchanged;
- matching reasons make policy decisions auditable.

## Open risks before real networking/execution

- opaque ids are forgeable: bind identities to authenticated keys;
- message ids are not deduplicated: retain a bounded causation index;
- logs are unsigned: authenticate events and snapshots;
- authority may be compromised: define recovery and revocation;
- declarations and observations may lie: attach provenance and trust;
- payload sizes are unbounded: enforce boundary quotas;
- TOCTOU between match and launch: re-check policy and reserve atomically;
- hostile execution: sandbox, isolate filesystem/network, and enforce budgets;
- private state may leak through snapshots/logs: separate projections and encrypt;
- replay/flood attacks: nonce/sequence windows, quotas, rate limits, and expiry.

No V0 output should be exposed directly to an untrusted network or used to launch
jobs.

