Langue canonique : Français

English version: [English](0002-pure-rust-core-and-web-boundary.en.md)

[Français](0002-pure-rust-core-and-web-boundary.md) | [English](0002-pure-rust-core-and-web-boundary.en.md)

# ADR 0002 : Cœur Rust pur et frontière web sérialisée

Statut : accepté

Les règles durables utilisent Rust sans I/O, runtime asynchrone, horloge globale ni aléa implicite.
TypeScript porte l’interface navigateur et les API de plateforme.
Wasm expose une façade sérialisée étroite plutôt que les représentations internes de Rust.

Les entrées susceptibles de varier doivent être fournies explicitement, ce qui
maintient l’alignement du replay et des comportements natif et navigateur.
