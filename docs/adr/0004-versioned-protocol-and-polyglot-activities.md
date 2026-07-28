Langue canonique : Français

English version: [English](0004-versioned-protocol-and-polyglot-activities.en.md)

[Français](0004-versioned-protocol-and-polyglot-activities.md) | [English](0004-versioned-protocol-and-polyglot-activities.en.md)

# ADR 0004 : Protocole versionné et activités polyglottes

Statut : accepté

Tous les messages de frontière portent une famille et une version de protocole.
Les versions inconnues échouent explicitement.
 JSON est le format de frontière
de la V0 et les fixtures sont des contrats de compatibilité ; les types Rust
internes ne sont pas conçus autour de JSON arbitraire.

Les activités utilisent des identifiants et des versions stables. Les jeux
existants s’intègrent par des adaptateurs et n’ont pas à être réécrits en Rust.
