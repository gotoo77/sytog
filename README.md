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
  sytog-transport/     messages réseau et adaptateur WebSocket
  sytog-node/          hôte autoritaire et journal JSONL
  sytog-capabilities/  offres, politiques, disponibilité et matching
  sytog-cli/           démonstrations locales et opérations sur fichiers
  sytog-wasm/          façade navigateur sérialisée et étroite
fixtures/              contrats V0 stables : protocole, journaux, jobs et nœuds
docs/                  architecture, ADR, guides, menace et roadmap
```

### Carte des crates

![Architecture des crates SYTOG](docs/assets/sytog-crates-overview.png)

_Les flèches indiquent les dépendances entre les crates._

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

### Session réseau locale

Dans un premier terminal :

```bash
cargo build -p sytog-cli
./target/debug/sytog serve --bind 127.0.0.1:7878
```

Puis dans deux autres terminaux :

```bash
./target/debug/sytog connect ws://127.0.0.1:7878 --participant alice
./target/debug/sytog connect ws://127.0.0.1:7878 --participant bob
```

Commandes interactives : `open thé café`, `vote café`, `close`, `state`, `quit`.
Chaque client conserve son état local sous `data/clients/` et demande les
événements manquants lors d’une reconnexion.

## État

La V0.2 ajoute un hôte WebSocket à autorité unique, un journal JSONL durable et
le rattrapage à la reconnexion au cœur déterministe V0.1. L’identité
cryptographique, le multi-autorité, l’exécution distante, Media Sync et les
interfaces produit restent conceptuels. Voir
[`docs/implementation-status.md`](docs/implementation-status.md).

## Documentation

Le français est la langue par défaut. Toute nouvelle page doit proposer un
équivalent anglais et un sélecteur de langue réciproque. Voir la
[convention documentaire](docs/README.md).
