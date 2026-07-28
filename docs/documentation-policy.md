Langue canonique : Français

English version: [English](documentation-policy.en.md)

[Français](documentation-policy.md) | [English](documentation-policy.en.md)

# Politique linguistique de la documentation

## Règle

Le français est la source de vérité de la documentation SYTOG. Le fichier
canonique utilise le nom `page.md`. Lorsqu’un fichier `page.en.md` existe, il
constitue une traduction anglaise maintenue, jamais une seconde source
normative.

Toute modification normative du document français implique la mise à jour de
sa traduction anglaise avant fusion. La structure, la numérotation, les titres,
les tableaux, les liens internes et les ancres doivent rester isomorphes autant
que le permet la langue. Une traduction peut adapter la formulation, mais ne
doit ajouter, retirer ou affaiblir aucune décision.

Chaque revue d’un document bilingue vérifie explicitement :

- la présence des métadonnées réciproques de langue ;
- la correspondance des sections, listes, tableaux et décisions ;
- la cohérence des liens et ancres ;
- l’absence de modification normative dans une seule langue.

Les ADR, documents d’architecture, protocoles et spécifications ont vocation à
être bilingues. Les guides, README et documents développeur migrent séparément.
Un document opérationnel, temporaire ou historique peut rester uniquement en
français si sa justification est inscrite dans le plan de migration.

## Métadonnées

Un document français bilingue commence par :

```text
Langue canonique : Français

English version: [English](page.en.md)
```

Sa traduction anglaise commence par :

```text
Canonical source: French

Source document: [Français](page.md)
```

## État et plan de migration

Le tableau décrit l’état après l’application de cette politique. « À traduire »
désigne une migration ultérieure ; aucune traduction hors ADR n’est réalisée
dans cette tâche.

| Document canonique ou paire | État | Action proposée |
|---|---|---|
| `README.md` / `README.en.md` | Bilingue | Maintenir ; migration README séparée. |
| `SYTOG_PROMPT_CODEX_FINAL.md` | Français uniquement | Peut rester en français : prompt historique interne, non normatif. |
| `docs/README.md` / `docs/README.en.md` | Bilingue | Maintenir ; ne pas modifier dans cette tâche. |
| `docs/documentation-policy.md` / `.en.md` | Bilingue | Maintenir comme règle de référence. |
| `docs/implementation-status.md` / `.en.md` | Bilingue | Maintenir la paire. |
| `docs/invariants.md` / `.en.md` | Bilingue | Maintenir la paire ; document normatif prioritaire. |
| ADR 0001 | Bilingue | Migration achevée ; français canonique. |
| ADR 0002 | Bilingue | Migration achevée ; français canonique. |
| ADR 0003 | Bilingue | Migration achevée ; français canonique. |
| ADR 0004 | Bilingue | Migration achevée ; français canonique. |
| ADR 0005 | Bilingue | Migration achevée ; français canonique. |
| ADR 0006 | Bilingue | Migration achevée ; français canonique. |
| ADR 0007 | Bilingue | Migration achevée ; français canonique. |
| ADR 0008 | Bilingue | Migration achevée ; français canonique. |
| ADR 0009 | Bilingue | Maintenir ; proposer séparément son passage à `accepted`. |
| `docs/architecture/overview.md` | Anglais uniquement | Traduire en français en priorité 1 ; conserver l’anglais sous `.en.md`. |
| `docs/architecture/current-answers.md` | Anglais uniquement | Traduire en français en priorité 1 ; conserver l’anglais sous `.en.md`. |
| `docs/protocol/v0.md` | Anglais uniquement | Traduire en français en priorité 1 ; protocole normatif. |
| `docs/protocol/v2.md` | Anglais uniquement | Traduire en français en priorité 1 ; protocole normatif. |
| `docs/network/v0.2.md` / `.en.md` | Bilingue | Maintenir la paire. |
| `docs/security/threat-model-v0.md` | Anglais uniquement | Traduire en français en priorité 2 ; document de conception sécurité. |
| `docs/roadmap/roadmap.md` | Anglais uniquement | Traduire en français en priorité 2. |
| `docs/scenarios/vertical-slice.md` | Anglais uniquement | Traduire en français en priorité 2 ; scénario de conception. |
| `docs/guides/add-activity.md` | Anglais uniquement | Traduire lors de la migration séparée des guides. |
| `docs/guides/add-capability.md` | Anglais uniquement | Traduire lors de la migration séparée des guides. |
| `docs/guides/typescript-game-integration.md` | Anglais uniquement | Traduire lors de la migration séparée des guides. |

## Ordre proposé

1. Maintenir dès maintenant toutes les paires existantes et les ADR.
2. Migrer ensemble architecture et protocoles, avec une revue de symétrie.
3. Migrer menace, roadmap et scénarios.
4. Migrer séparément README, guides utilisateur et documentation développeur.
5. Reclasser les documents temporaires à leur création avec une justification
   explicite s’ils restent monolingues.
