[Français](README.md) | [English](README.en.md)

# SYTOG

> SYTOG est un runtime distribué portable pour des sessions, activités et
> capacités synchronisées.

Ce dépôt contient une V0 volontairement sobre qui démontre deux parcours
indépendants :

- un cœur de session déterministe : commandes validées → événements immuables →
  reducer, snapshot, journal et replay ;
- un registre de capacités fonctionnelles : inventaire, offres, politique
  d’exposition locale, disponibilité courante, observations et matching
  déterministe explicable.

SYTOG n’est ni FFF, ni un moteur de jeu, ni un moteur d’IA, ni un gestionnaire de
cluster. Friends Fun Factory est un produit construit au-dessus de SYTOG ; GOTUS
et PuzzleGuess s’intègrent par des adaptateurs polyglottes ; Noema implémente les
capacités IA ; Delibra définit les workflows cognitifs ; Observatory/Probe
conserve l’historique empirique.

## Dépôt

```text
crates/
  sytog-domain/        types durables de session et reducer
  sytog-protocol/      enveloppes de frontière versionnées
  sytog-runtime/       décision pure, effets, replay et snapshots
  sytog-demo-counter/  activité exemple hors du cœur générique
  sytog-demo-vote/     seconde activité validant la frontière d’extension
  sytog-capabilities/  offres, politiques, disponibilité et matching
  sytog-cli/           démonstrations locales et opérations sur fichiers
  sytog-wasm/          façade navigateur sérialisée et étroite
fixtures/              contrats V0 stables : protocole, journaux, jobs et nœuds
docs/                  architecture, ADR, guides, menace et roadmap
```

Le dépôt ne contient aucune crate vide de transport ou de stockage. Ces
adaptateurs seront ajoutés lorsqu’un scénario réel en aura besoin.

## Développement

Rust 1.85.1 est épinglé.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p sytog-wasm --target wasm32-unknown-unknown
cargo run -p sytog-cli -- demo session
cargo run -p sytog-cli -- demo capabilities
cargo run -p sytog-cli -- demo vote
cargo run -p sytog-cli -- --json capability match \
  fixtures/capabilities/job.json fixtures/capabilities/nodes.json
cargo run -p sytog-cli -- --json capability match \
  fixtures/capabilities/job-cpu.json fixtures/capabilities/nodes.json
cargo run -p sytog-cli -- replay fixtures/session/demo-event-log.json
cargo run -p sytog-cli -- validate fixtures/protocol/envelope-v1.json
```

## État

« Implémenté » signifie ici un comportement local, déterministe et en mémoire.
Le réseau, les adaptateurs de persistance, l’identité cryptographique, la
reconnexion, l’exécution distribuée, Media Sync et les interfaces produit
restent explicitement conceptuels. Voir
[`docs/implementation-status.md`](docs/implementation-status.md).

## Documentation

Le français est la langue par défaut. Toute nouvelle page doit proposer un
équivalent anglais et un sélecteur de langue réciproque. Voir la
[convention documentaire](docs/README.md).

