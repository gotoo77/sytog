Canonical source: French

Source document: [Français](documentation-policy.md)

[Français](documentation-policy.md) | [English](documentation-policy.en.md)

# Documentation language policy

## Rule

French is the source of truth for SYTOG documentation. The canonical file uses
the `page.md` name. When a `page.en.md` file exists, it is a maintained English
translation, never a second normative source.

Every normative change to the French document requires updating its English
translation before merge. Structure, numbering, headings, tables, internal
links, and anchors must remain isomorphic as far as language permits. A
translation may adapt phrasing but must not add, remove, or weaken a decision.

Every review of a bilingual document explicitly checks:

- reciprocal language metadata is present;
- sections, lists, tables, and decisions correspond;
- links and anchors remain coherent;
- no normative change exists in only one language.

ADRs, architecture documents, protocols, and specifications are intended to be
bilingual. Guides, READMEs, and developer documentation migrate separately. An
operational, temporary, or historical document may remain French-only when its
justification is recorded in the migration plan.

## Metadata

A bilingual French document starts with:

```text
Langue canonique : Français

English version: [English](page.en.md)
```

Its English translation starts with:

```text
Canonical source: French

Source document: [Français](page.md)
```

## Migration status and plan

The table describes the state after applying this policy. “Translate” denotes
later migration; this task translates no non-ADR document.

| Canonical document or pair | State | Proposed action |
|---|---|---|
| `README.md` / `README.en.md` | Bilingual | Maintain; migrate READMEs separately. |
| `SYTOG_PROMPT_CODEX_FINAL.md` | French only | May remain French: internal historical prompt, not normative. |
| `docs/README.md` / `docs/README.en.md` | Bilingual | Maintain; do not modify in this task. |
| `docs/documentation-policy.md` / `.en.md` | Bilingual | Maintain as the reference rule. |
| `docs/implementation-status.md` / `.en.md` | Bilingual | Maintain the pair. |
| `docs/invariants.md` / `.en.md` | Bilingual | Maintain the pair; high-priority normative document. |
| ADR 0001 | Bilingual | Migration complete; French canonical. |
| ADR 0002 | Bilingual | Migration complete; French canonical. |
| ADR 0003 | Bilingual | Migration complete; French canonical. |
| ADR 0004 | Bilingual | Migration complete; French canonical. |
| ADR 0005 | Bilingual | Migration complete; French canonical. |
| ADR 0006 | Bilingual | Migration complete; French canonical. |
| ADR 0007 | Bilingual | Migration complete; French canonical. |
| ADR 0008 | Bilingual | Migration complete; French canonical. |
| ADR 0009 | Bilingual | Maintain; propose its move to `accepted` separately. |
| `docs/architecture/overview.md` | English only | Translate to French as priority 1; retain English under `.en.md`. |
| `docs/architecture/current-answers.md` | English only | Translate to French as priority 1; retain English under `.en.md`. |
| `docs/protocol/v0.md` | English only | Translate to French as priority 1; normative protocol. |
| `docs/protocol/v2.md` | English only | Translate to French as priority 1; normative protocol. |
| `docs/network/v0.2.md` / `.en.md` | Bilingual | Maintain the pair. |
| `docs/security/threat-model-v0.md` | English only | Translate to French as priority 2; security design document. |
| `docs/roadmap/roadmap.md` | English only | Translate to French as priority 2. |
| `docs/scenarios/vertical-slice.md` | English only | Translate to French as priority 2; design scenario. |
| `docs/guides/add-activity.md` | English only | Translate during the separate guide migration. |
| `docs/guides/add-capability.md` | English only | Translate during the separate guide migration. |
| `docs/guides/typescript-game-integration.md` | English only | Translate during the separate guide migration. |

## Proposed order

1. Maintain all existing pairs and ADRs from now on.
2. Migrate architecture and protocols together, with symmetry review.
3. Migrate threat model, roadmap, and scenarios.
4. Migrate READMEs, user guides, and developer documentation separately.
5. Classify temporary documents when created and record an explicit
   justification when they remain monolingual.
