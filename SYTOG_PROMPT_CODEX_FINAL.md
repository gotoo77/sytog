# Mission Codex — Concevoir et initialiser SYTOG

## 0. Rôle et méthode

Tu interviens comme architecte logiciel senior, ingénieur Rust et concepteur de systèmes distribués sur un nouveau projet nommé **SYTOG**.

Ta mission est de transformer une vision ambitieuse en une architecture cohérente, durable et incrémentale, puis d’en implémenter une première tranche verticale fonctionnelle.

Le but n’est pas de construire immédiatement une plateforme complète ni d’accumuler des abstractions. Il faut :

1. comprendre et formaliser l’identité du projet ;
2. définir clairement ses frontières ;
3. préserver les ambitions de long terme ;
4. éviter le monolithe et la sur-conception ;
5. construire un noyau minimal réellement exécutable ;
6. documenter les décisions structurantes ;
7. produire une trajectoire d’évolution crédible.

Lorsqu’une ambiguïté ne bloque pas la première tranche, prends une décision raisonnable, explicite-la dans un ADR et poursuis.

Ne demande pas confirmation pour chaque détail. Inspecte, décide, documente et implémente.

---

# 1. Vision générale

## 1.1 Identité de SYTOG

SYTOG est un **runtime distribué portable pour sessions, activités et capacités synchronisées**.

Il doit permettre à des participants hétérogènes de :

- se découvrir ;
- rejoindre une session ;
- annoncer leur présence ;
- publier des capacités ;
- échanger des commandes et des événements ;
- partager ou reconstruire un état ;
- coordonner une activité collective ;
- négocier l’utilisation de ressources ;
- exécuter des tâches ;
- observer les résultats ;
- se reconnecter ;
- transférer une autorité logique.

Un participant peut être :

- un humain dans un navigateur ;
- une application mobile ;
- un serveur ;
- une machine personnelle ;
- un worker de calcul ;
- un agent logiciel ;
- un moteur LLM ;
- un GPU exposant une capacité d’inférence ;
- un service spécialisé ;
- un dispositif physique.

SYTOG ne doit pas confondre ces catégories, mais doit permettre leur coordination au moyen de concepts communs.

Formulation de travail :

> **SYTOG is a portable distributed runtime for synchronized sessions, activities and capabilities.**

Cette formulation peut évoluer. Son intention doit rester stable.

---

## 1.2 Ce que SYTOG n’est pas

SYTOG n’est pas :

- un jeu ;
- une plateforme de jeux ;
- un framework frontend ;
- un moteur LLM ;
- un gestionnaire de conteneurs ;
- un clone de Kubernetes ;
- un protocole de chat ;
- un lecteur multimédia ;
- une blockchain ;
- un produit cloud centralisé ;
- un monolithe regroupant tous les projets connexes.

SYTOG fournit un socle générique. Les produits et modules spécialisés s’appuient sur lui.

---

# 2. Écosystème de projets

Les projets associés doivent rester distincts tout en pouvant se composer.

## 2.1 Friends Fun Factory

**Friends Fun Factory**, ou FFF, est une plateforme sociale de jeux et d’expériences collectives construite au-dessus de SYTOG.

FFF doit pouvoir fournir :

- création de salle ;
- invitations ;
- code court ou QR code ;
- lobby ;
- présence des joueurs ;
- chat ;
- sélection d’une activité ;
- écran principal partagé ;
- interfaces privées sur smartphone ;
- lancement et arrêt d’une partie ;
- enchaînement de plusieurs jeux ;
- retour au lobby.

FFF est une application de SYTOG, pas son identité complète.

---

## 2.2 GOTUS et PuzzleGuess

GOTUS et PuzzleGuess sont des jeux existants ou indépendants pouvant être intégrés à FFF.

Ils ne doivent pas être réécrits en Rust par principe.

SYTOG impose un protocole d’intégration, pas un langage.

Un jeu doit pouvoir rester en :

- TypeScript ;
- JavaScript ;
- Rust ;
- Python ;
- C# ;
- Godot ;
- Unity ;
- ou toute technologie appropriée.

L’intégration peut être progressive :

1. lancement ou embarquement du jeu existant ;
2. adaptateur FFF traduisant commandes, événements et snapshots ;
3. extraction éventuelle d’un moteur métier indépendant ;
4. intégration profonde seulement si elle apporte une valeur réelle.

---

## 2.3 Noema

Noema est une façade commune d’accès aux modèles et moteurs IA.

Il peut abstraire :

- Ollama ;
- llama.cpp ;
- vLLM ;
- API OpenAI-compatible ;
- fournisseurs cloud ;
- moteurs locaux spécialisés ;
- modèles de texte, image, audio ou embeddings.

SYTOG ne doit pas connaître tous les détails des runtimes IA.

Un nœud peut publier une capacité fonctionnelle telle que `llm.inference`, tandis que Noema gère le moteur concret, le modèle et ses paramètres.

---

## 2.4 Delibra

Delibra est un runtime de délibération structurée et de production d’artefacts avec provenance durable.

Il peut :

- définir une activité cognitive ;
- orchestrer plusieurs rôles ou étapes ;
- produire des artefacts ;
- appeler des capacités IA par Noema ;
- demander à SYTOG de découvrir ou sélectionner une ressource disponible ;
- restituer les résultats à une application telle que FFF.

Delibra ne doit pas devenir un orchestrateur réseau.

SYTOG ne doit pas devenir un moteur de délibération.

---

## 2.5 Observatory

Observatory représente la couche d’observation empirique :

- exécutions ;
- traces ;
- latences ;
- erreurs ;
- consommation ;
- comportements réels ;
- performances observées ;
- dérives entre promesses et résultats.

La distinction suivante est essentielle :

```text
déclaré
≠ observé
≠ disponible maintenant
```

Observatory ou un module équivalent pourra alimenter les décisions d’orchestration sans contaminer le cœur déterministe.

---

# 3. Architecture d’ensemble

```text
Produits et expériences
├── Friends Fun Factory
├── outils collaboratifs
├── ateliers
├── applications éducatives
└── autres produits futurs
          │
          ▼
Activités et modules spécialisés
├── GOTUS
├── PuzzleGuess
├── Media Sync
├── Game Runtime
├── Delibra
└── autres activités
          │
          ▼
Services de capacités
├── Noema
├── workers CPU/GPU
├── transcodage
├── solveurs
├── moteurs IA
└── services spécialisés
          │
          ▼
SYTOG
├── sessions
├── identité
├── présence
├── commandes
├── événements
├── état
├── autorité
├── capacités
├── négociation
├── orchestration
└── observation
          │
          ▼
Adaptateurs remplaçables
├── mémoire
├── WebSocket
├── WebRTC
├── libp2p
├── HTTP
├── SQLite
├── PostgreSQL
├── IndexedDB
├── navigateur
├── desktop
└── mobile
```

---

# 4. Principe architectural fondamental

> **Cœur simple et durable, capacités spécialisées composables, adaptateurs techniques remplaçables.**

Distinguer trois catégories.

## 4.1 Cœur durable

Le cœur contient uniquement les concepts qui doivent survivre aux frameworks et aux choix de plateformes :

- session ;
- participant ;
- identité logique ;
- rôle ;
- permission ;
- autorité ;
- commande ;
- événement ;
- état ;
- révision ;
- snapshot ;
- journal ;
- activité ;
- capability ;
- offre ;
- besoin ;
- protocole versionné ;
- validation ;
- réduction ;
- reconnexion.

## 4.2 Modules spécialisés

Exemples :

- présence ;
- chat ;
- voix ;
- synchronisation média ;
- runtime de jeu ;
- vote ;
- score ;
- manches ;
- chronomètres ;
- affichage partagé ;
- découverte de ressources ;
- inventaire matériel ;
- capacités IA ;
- orchestration de jobs ;
- exécution distribuée ;
- mesure des ressources ;
- politique d’exposition.

## 4.3 Adaptateurs

Exemples :

- WebRTC ;
- WebSocket ;
- libp2p ;
- réseau local ;
- serveur de signalisation ;
- TURN ;
- HTTP ;
- filesystem ;
- SQLite ;
- PostgreSQL ;
- navigateur ;
- Tauri ;
- API de runtime IA ;
- API système ;
- outils de détection GPU.

Les dépendances doivent pointer vers le cœur, jamais l’inverse.

```text
adaptateurs → modules → runtime → domaine
```

---

# 5. Choix technologiques

## 5.1 Rust

Utiliser Rust pour :

- domaine ;
- protocole ;
- runtime ;
- validation ;
- reducers ;
- replay ;
- snapshots ;
- simulation ;
- CLI ;
- algorithmes de synchronisation ;
- matching de capabilities ;
- orchestration ;
- services natifs pertinents ;
- façade WebAssembly.

Rust est choisi pour :

- ses types ;
- ses invariants explicites ;
- sa sûreté mémoire ;
- ses performances ;
- sa portabilité ;
- sa compatibilité WebAssembly ;
- son adéquation aux protocoles et systèmes distribués.

Ne pas imposer Rust à toutes les couches.

## 5.2 TypeScript et web

Utiliser TypeScript pour :

- UI web ;
- DOM ;
- composants ;
- animations ;
- players HTML ;
- permissions navigateur ;
- WebRTC côté navigateur ;
- adaptateurs de jeux existants ;
- intégration avec le shell FFF.

Architecture cible :

```text
UI TypeScript
    ↓
FFF Activity API
    ↓
façade WebAssembly
    ↓
SYTOG Core en Rust
```

## 5.3 Cœur compatible WebAssembly

Le domaine ne doit dépendre directement :

- ni de Tokio ;
- ni d’Axum ;
- ni de sockets ;
- ni du filesystem ;
- ni de threads système ;
- ni du DOM ;
- ni de WebRTC ;
- ni de l’horloge globale ;
- ni d’un générateur aléatoire implicite.

Les entrées non déterministes doivent être explicites.

Modèle privilégié :

```text
état + commande + contexte
    → décision
    → événements + effets demandés
```

Puis :

```text
état + événement
    → nouvel état
```

---

# 6. Modèle de session

Une session représente un contexte de coordination durable ou temporaire.

Modèle conceptuel :

```text
Session
├── session_id
├── protocol_version
├── lifecycle
├── participants
├── roles
├── authority
├── activity
├── shared_state
├── revision
├── event_log
├── snapshot
└── enabled_capabilities
```

Cycle de vie possible :

- created ;
- open ;
- starting ;
- active ;
- paused ;
- completed ;
- closed.

Éviter de multiplier les états sans besoin.

Les transitions doivent être explicites et validées.

---

# 7. Activités

Une activité est un module exécuté dans une session.

Elle définit :

- identifiant stable ;
- version ;
- métadonnées ;
- état initial ;
- commandes ;
- événements ;
- règles de validation ;
- reducer ;
- permissions ;
- snapshots ;
- état public ;
- état privé ;
- capabilities requises ;
- compatibilité.

Exemple :

```text
ActivityDefinition
├── activity_id
├── version
├── metadata
├── initial_state
├── command_schema
├── event_schema
├── reducer
├── permissions
├── snapshot_policy
├── required_capabilities
└── compatibility
```

Compositions possibles :

```text
BlindTest =
    Session
  + Presence
  + MediaSync
  + Answers
  + Scoring
  + Rounds
```

```text
GOTUS =
    Session
  + TurnManagement
  + GuessSubmission
  + WordValidation
  + SharedBoard
```

```text
DistributedInference =
    Session
  + CapabilityDiscovery
  + Scheduling
  + Execution
  + Observation
```

---

# 8. Commandes, événements et effets

## 8.1 Commande

Une commande représente une intention.

Elle peut être refusée.

Exemples :

- créer une session ;
- rejoindre ;
- quitter ;
- démarrer une activité ;
- soumettre une réponse ;
- publier une capability ;
- demander une exécution ;
- réserver une ressource ;
- transférer l’autorité ;
- lancer un média ;
- annuler un job.

## 8.2 Événement

Un événement représente un fait accepté.

Il est :

- immuable ;
- sérialisable ;
- journalisable ;
- rejouable ;
- versionné.

Exemples :

- participant ajouté ;
- activité démarrée ;
- capability publiée ;
- job proposé ;
- job accepté ;
- exécution commencée ;
- résultat produit ;
- média chargé ;
- autorité transférée.

## 8.3 Effet

Un effet décrit une action externe demandée :

- diffuser ;
- persister ;
- programmer un timer ;
- appeler un worker ;
- contrôler un player ;
- écrire un fichier ;
- ouvrir une connexion ;
- demander une mesure système.

Le cœur décrit l’effet mais ne l’exécute pas directement.

---

# 9. Autorité et distribution

SYTOG vise une architecture :

> **P2P-first, server-assisted.**

Cela signifie :

- pair à pair lorsque raisonnable ;
- serveur de signalisation ou relais possible ;
- fonctionnement sur LAN possible ;
- auto-hébergement possible ;
- fallback WebSocket possible ;
- absence d’obligation de cloud central pour la logique métier.

Première version :

- créateur de session = autorité initiale ;
- l’autorité ordonne les événements faisant foi ;
- les autres participants soumettent des commandes ;
- l’autorité accepte ou refuse ;
- séquence monotone ;
- transfert manuel d’autorité possible.

Ne pas implémenter immédiatement :

- Paxos ;
- Raft ;
- blockchain ;
- CRDT universel ;
- consensus byzantin ;
- élection distribuée complète.

Distinguer :

```text
autorité logique
≠ pair réseau
≠ serveur
≠ interface principale
≠ propriétaire de la machine
```

---

# 10. Module Media Sync

La synchronisation média est un module majeur, mais pas l’identité entière de SYTOG.

Responsabilités possibles :

- identification d’un média ;
- chargement ;
- lecture ;
- pause ;
- seek ;
- vitesse ;
- autorité de contrôle ;
- intention temporelle ;
- position estimée ;
- dérive ;
- correction douce ;
- resynchronisation ;
- buffering ;
- reprise après reconnexion.

Modèle conceptuel :

```text
MediaTimeline
├── media_id
├── reference_position
├── reference_time
├── playback_state
├── playback_rate
└── revision
```

Le système doit synchroniser une intention temporelle, pas diffuser continuellement une position brute.

Prévoir un port abstrait :

```text
MediaPlayerPort
```

Adaptateurs futurs :

- BrowserMediaPlayer ;
- DesktopMediaPlayer ;
- MobileMediaPlayer ;
- TestMediaPlayer.

---

# 11. Capabilities et calcul distribué

## 11.1 Objectif

Créer un module permettant à une machine ou un service de décrire :

1. ce qu’il possède ;
2. ce qu’il sait faire ;
3. ce qu’il accepte d’exposer ;
4. ce qui est disponible maintenant ;
5. ce qui a été réellement observé.

L’objectif n’est pas uniquement de recenser du matériel.

Il faut découvrir et composer des **capacités fonctionnelles distribuées et hétérogènes**.

## 11.2 Inventaire matériel

Modèle conceptuel :

```text
HardwareInventory
├── cpu
│   ├── architecture
│   ├── physical_cores
│   ├── logical_cores
│   └── instruction_sets
├── memory
│   ├── total
│   └── available
├── accelerators
│   ├── kind
│   ├── vendor
│   ├── model
│   ├── memory
│   ├── compute_backends
│   └── supported_precisions
├── storage
├── network
└── platform
```

L’inventaire est informatif, mais ne constitue pas à lui seul une capability utilisable.

## 11.3 Capabilities logicielles

Exemples :

- `llm.inference` ;
- `embedding.compute` ;
- `image.generate` ;
- `speech.transcribe` ;
- `speech.synthesize` ;
- `media.transcode` ;
- `compute.wasm` ;
- `compute.container` ;
- `solver.execute` ;
- `code.run` ;
- `storage.provide` ;
- `relay.network`.

Une capability doit décrire un contrat fonctionnel.

Exemple conceptuel :

```json
{
  "capability": "llm.inference",
  "implementation": "noema",
  "models": ["qwen3:4b"],
  "context_limit": 32768,
  "languages": ["fr", "en"],
  "supports_streaming": true,
  "supports_tools": false,
  "concurrency": 1
}
```

Le hardware explique les contraintes.

La capability décrit ce que le système peut demander.

## 11.4 Politique d’exposition

Une machine ne doit jamais exposer automatiquement tout ce qu’elle possède.

Modèle conceptuel :

```text
ExposurePolicy
├── allowed_capabilities
├── max_cpu_share
├── max_memory
├── max_vram
├── time_windows
├── thermal_limits
├── energy_budget
├── allowed_requesters
├── local_only
├── internet_access
├── data_retention
└── consent_mode
```

Exemples :

- uniquement sur le LAN ;
- uniquement entre 20 h et minuit ;
- maximum 50 % CPU ;
- maximum 8 Go de VRAM ;
- aucune conservation des prompts ;
- approbation manuelle pour chaque job ;
- uniquement pour des identités de confiance.

## 11.5 Distinctions obligatoires

Ne jamais confondre :

```text
HardwareInventory
DeclaredCapability
ExposurePolicy
ObservedCapability
CurrentAvailability
HistoricalPerformance
```

Un nœud peut déclarer une capability, mais être momentanément indisponible.

Une performance observée peut contredire une promesse déclarative.

## 11.6 Besoin fonctionnel

Un job doit exprimer ce dont il a besoin, pas le matériel précis à utiliser.

```text
JobRequirement
├── capability
├── model_or_family
├── minimum_context
├── language
├── latency_constraint
├── privacy
├── locality
├── estimated_memory
├── priority
├── energy_preference
└── budget_dimensions
```

Le système résout :

```text
besoin abstrait → nœuds compatibles
```

## 11.7 Orchestration

Séparer :

```text
sytog-capabilities
    décrit les offres

sytog-orchestration
    trouve et classe les candidats

sytog-execution
    négocie et suit l’exécution

sytog-observation
    mesure le comportement réel

sytog-resource-policy
    applique consentement et limites
```

Ne pas créer toutes ces crates immédiatement. Maintenir ces frontières conceptuelles.

Cycle cible :

1. publication d’une capability ;
2. description d’un job ;
3. matching ;
4. explication des acceptations et rejets ;
5. sélection ;
6. proposition au nœud ;
7. acceptation ou refus ;
8. réservation ;
9. exécution ;
10. progression ;
11. résultat ;
12. observation ;
13. libération des ressources.

## 11.8 Comptabilité multidimensionnelle

Ne pas réduire immédiatement le coût à une somme monétaire.

Mesurer séparément lorsque possible :

- temps CPU ;
- temps GPU ;
- RAM ;
- VRAM ;
- énergie ;
- bande passante ;
- stockage ;
- durée d’occupation ;
- confidentialité ;
- coût financier ;
- résultat utile ;
- échec.

Le système pourra ultérieurement décider comment agréger ou compenser ces dimensions.

---

# 12. Sécurité et confiance

Considérer :

- usurpation d’identité ;
- commande injectée ;
- rejeu ;
- duplication ;
- événement hors ordre ;
- nœud malveillant ;
- autorité compromise ;
- payload invalide ;
- modèle mensonger ;
- fuite de données privées ;
- exécution de code hostile ;
- job excessif ;
- consommation non consentie ;
- résultat falsifié ;
- capability mensongère.

Règles :

- toute entrée réseau est non fiable ;
- validation stricte ;
- refus explicite ;
- aucune correction silencieuse ;
- aucun accès arbitraire au système ;
- sandboxing futur pour l’exécution ;
- politiques locales souveraines ;
- consentement explicite pour les ressources sensibles.

Ne pas implémenter une cryptographie complète en V0, mais produire un premier modèle de menace.

---

# 13. Protocole

Le protocole doit être versionné dès le départ.

Enveloppe minimale :

- famille de protocole ;
- version ;
- message_id ;
- session_id ;
- sender_id ;
- message_type ;
- payload ;
- revision ou sequence ;
- métadonnées utiles.

JSON est acceptable aux frontières en V0.

Le modèle interne Rust ne doit pas être asservi à JSON.

Conserver des fixtures protocolaires stables.

Toute rupture de compatibilité doit être documentée.

---

# 14. Durabilité et observabilité

Le projet doit conserver :

- intention architecturale ;
- invariants ;
- ADR ;
- contrats ;
- scénarios ;
- fixtures ;
- journaux ;
- snapshots ;
- traces ;
- mesures ;
- hypothèses ;
- limites.

ADR initiaux suggérés :

1. identité de SYTOG ;
2. frontières SYTOG / FFF / jeux ;
3. relation avec Noema, Delibra et Observatory ;
4. cœur Rust portable ;
5. UI web TypeScript ;
6. commandes, événements et effets ;
7. autorité initiale ;
8. P2P-first, server-assisted ;
9. protocole versionné ;
10. jeux polyglottes par adaptateurs ;
11. Media Sync comme module ;
12. capabilities fonctionnelles ;
13. politique d’exposition ;
14. déclaré vs observé vs disponible ;
15. déterminisme et absence d’I/O dans le domaine.

---

# 15. Tests

Prévoir :

## Tests unitaires

- validation ;
- transitions ;
- permissions ;
- reducers ;
- erreurs ;
- sérialisation ;
- matching de capabilities ;
- politique d’exposition.

## Tests de replay

Un même journal reconstruit exactement le même état.

## Tests de propriétés

Exemples :

- une révision ne régresse jamais ;
- un replay complet et un snapshot suivi du suffixe donnent le même état ;
- une commande refusée ne modifie pas l’état ;
- une capability interdite par la politique n’est jamais sélectionnée ;
- un nœud indisponible n’est pas classé comme exécutable ;
- un même ensemble d’entrées produit le même classement déterministe.

## Simulations

- plusieurs participants ;
- déconnexion ;
- reconnexion ;
- autorité absente ;
- doublon ;
- ordre incorrect ;
- version inconnue ;
- capability retirée ;
- saturation ;
- job refusé ;
- timeout ;
- résultat invalide.

---

# 16. Structure cible du dépôt

Proposer une structure proche de :

```text
sytog/
├── Cargo.toml
├── README.md
├── LICENSE
├── rust-toolchain.toml
├── crates/
│   ├── sytog-domain/
│   ├── sytog-protocol/
│   ├── sytog-runtime/
│   ├── sytog-simulation/
│   ├── sytog-cli/
│   ├── sytog-wasm/
│   ├── sytog-capabilities/
│   ├── sytog-orchestration/
│   └── sytog-media/
├── adapters/
│   ├── transport-memory/
│   ├── storage-memory/
│   └── inventory-local/
├── web/
│   └── fff-shell/
├── fixtures/
├── examples/
├── docs/
│   ├── architecture/
│   ├── adr/
│   ├── protocol/
│   ├── scenarios/
│   └── roadmap/
└── scripts/
```

Cette structure est indicative.

Ne crée pas de crates vides pour simuler une architecture complète.

Implémente uniquement ce qui sert la première tranche.

---

# 17. Première tranche verticale

Construire une démonstration locale, sans vrai réseau, couvrant deux dimensions :

## 17.1 Session générique

- création d’une session ;
- ajout de participants ;
- autorité initiale ;
- activité de démonstration ;
- commande acceptée ;
- commande refusée ;
- événement ;
- reducer ;
- révision ;
- snapshot ;
- export du journal ;
- replay ;
- reconstruction déterministe.

## 17.2 Capability Registry

- plusieurs nœuds simulés ;
- inventaires matériels minimaux ;
- capabilities déclarées ;
- politiques d’exposition ;
- disponibilités courantes ;
- quelques observations historiques ;
- description d’un job ;
- matching ;
- classement déterministe ;
- explication détaillée des acceptations et rejets.

Exemple de job :

```text
capability: llm.inference
model: qwen3:4b
minimum_context: 16000
language: fr
streaming_required: true
local_network_only: true
```

Exemple de résultat :

```text
node-a: compatible
node-b: rejected — required model unavailable
node-c: rejected — policy forbids remote inference
node-d: compatible but currently saturated
```

La première tranche ne doit pas encore exécuter réellement un job distribué.

Elle doit prouver :

- le modèle ;
- la distinction inventaire/capability/politique/disponibilité ;
- le matching ;
- l’explicabilité ;
- le replay ;
- l’indépendance du transport.

---

# 18. CLI

Créer un CLI minimal.

Commandes possibles :

```text
sytog demo session
sytog demo capabilities
sytog scenario run <file>
sytog replay <event-log>
sytog inspect <snapshot-or-log>
sytog capability match <job-file> <nodes-file>
sytog validate <file>
```

Implémenter un sous-ensemble cohérent.

Le CLI doit afficher :

- commandes ;
- acceptations ou refus ;
- événements ;
- révision ;
- état ;
- candidats ;
- scores ;
- motifs d’acceptation ;
- motifs de rejet.

Favoriser une sortie humaine claire et une option JSON pour l’automatisation.

---

# 19. WebAssembly

Ne pas rendre la première tranche dépendante d’une UI.

Préparer néanmoins :

- compilation `wasm32-unknown-unknown` ;
- façade réduite ;
- API sérialisée minimale ;
- documentation de la frontière Rust/TypeScript ;
- vérification CI si raisonnable.

Ne pas exposer directement tous les types internes.

---

# 20. Qualité

Utiliser :

- workspace Rust moderne ;
- `cargo fmt` ;
- `cargo clippy` ;
- `cargo test` ;
- documentation ;
- erreurs structurées ;
- peu de dépendances ;
- visibilité minimale ;
- API publique réduite ;
- pas de `unsafe` sans justification ;
- pas d’`unwrap` dans le code de production sauf invariant local incontestable et documenté.

Commentaires : expliquer le pourquoi, pas le trivial.

---

# 21. Non-objectifs de la V0

Ne pas construire immédiatement :

- FFF complet ;
- GOTUS réécrit ;
- PuzzleGuess réécrit ;
- chat vocal ;
- synchronisation vidéo complète ;
- WebRTC complet ;
- infrastructure cloud ;
- authentification globale ;
- cryptographie complète ;
- sandbox universelle ;
- scheduler distribué sophistiqué ;
- migration de jobs ;
- fédération publique ;
- monnaie ou marché de calcul ;
- CRDT générique ;
- consensus complet ;
- SDK multi-langages complet ;
- application mobile ;
- marketplace de plugins.

---

# 22. Roadmap attendue

## Phase 0 — Fondation

- vision ;
- frontières ;
- domaine ;
- protocole ;
- runtime pur ;
- replay ;
- simulation ;
- capability registry ;
- matching explicable.

## Phase 1 — Session locale

- transport mémoire ;
- plusieurs clients simulés ;
- snapshots ;
- reconnexion ;
- transfert d’autorité.

## Phase 2 — Première interface web

- Wasm ;
- shell FFF ;
- lobby ;
- activité de démonstration.

## Phase 3 — Réseau réel simple

- WebSocket ;
- signalisation ;
- présence ;
- reprise.

## Phase 4 — P2P

- WebRTC DataChannel ;
- fallback ;
- LAN ;
- identité et confiance initiales.

## Phase 5 — Premier jeu

- intégration de GOTUS ou PuzzleGuess par adaptateur ;
- vue principale ;
- vues joueurs ;
- snapshots.

## Phase 6 — Exécution distribuée

- proposition de job ;
- acceptation ;
- réservation ;
- progression ;
- résultat ;
- annulation ;
- observations.

## Phase 7 — Noema

- publication de capacités IA ;
- découverte de modèles ;
- exécution locale ;
- métriques ;
- politiques de confidentialité.

## Phase 8 — Media Sync

- timeline ;
- player abstrait ;
- dérive ;
- blind test ou quiz média.

## Phase 9 — Enrichissement social

- chat ;
- voix ;
- invitations ;
- équipes ;
- modération.

---

# 23. Questions auxquelles répondre

Documenter explicitement les réponses actuelles :

- Qu’est-ce qu’une session ?
- Qu’est-ce qu’un participant ?
- Qu’est-ce qu’une activité ?
- Qu’est-ce qu’une capability ?
- Qui possède l’état ?
- Qui valide une commande ?
- Qui ordonne les événements ?
- Comment rejouer ?
- Comment reconnecter ?
- Comment transférer l’autorité ?
- Comment versionner le protocole ?
- Comment versionner une activité ?
- Comment intégrer un jeu existant sans réécriture ?
- Quelle frontière entre FFF et SYTOG ?
- Quelle frontière entre Noema et SYTOG ?
- Quelle frontière entre Delibra et SYTOG ?
- Quelle différence entre inventaire et capability ?
- Quelle différence entre déclaré, observé et disponible ?
- Comment appliquer une politique d’exposition ?
- Comment expliquer un matching ?
- Comment empêcher une ressource non consentie d’être utilisée ?
- Comment tester sans réseau réel ?

---

# 24. Livrables

Fournir :

1. diagnostic du dépôt ;
2. vision ;
3. architecture ;
4. frontières entre projets ;
5. ADR initiaux ;
6. workspace Rust ;
7. domaine minimal ;
8. protocole V0 ;
9. runtime commande → événements → réduction ;
10. activité de démonstration ;
11. capability registry ;
12. matcher explicable ;
13. CLI ;
14. fixtures ;
15. tests unitaires ;
16. tests de replay ;
17. tests de propriétés pertinents ;
18. documentation d’ajout d’une activité ;
19. documentation d’intégration d’un jeu TypeScript ;
20. documentation d’ajout d’une capability ;
21. roadmap ;
22. commandes exactes de développement ;
23. bilan honnête entre implémenté et conceptuel.

---

# 25. Critères de réussite

La mission est réussie si :

- SYTOG possède une identité claire ;
- les frontières entre SYTOG, FFF, jeux, Noema, Delibra et Observatory sont explicites ;
- le cœur est indépendant du réseau et de l’UI ;
- les commandes sont validées ;
- les événements reconstruisent l’état ;
- le replay est déterministe ;
- les capabilities sont fonctionnelles et non réduites au hardware ;
- les politiques d’exposition sont séparées des inventaires ;
- le matching est explicable ;
- un jeu existant peut s’intégrer sans changer de langage ;
- Rust sert de cœur portable sans devenir un dogme ;
- WebAssembly reste possible ;
- le projet reste compréhensible ;
- la première tranche fonctionne réellement ;
- la vision distribuée reste ouverte sans sur-conception.

---

# 26. Principe final

Ne construis pas un framework abstrait à la recherche d’usages.

Construis le noyau minimal d’un écosystème réel, destiné à coordonner des humains, des logiciels, des modèles et des machines autour d’activités communes.

La priorité est :

> **Une fondation sobre, démontrable, durable, explicable et extensible, capable d’accueillir progressivement Friends Fun Factory, GOTUS, PuzzleGuess, la synchronisation média, Noema, Delibra et un réseau coopératif de capacités de calcul, sans les enfermer dans un langage, un fournisseur, un transport ou une plateforme.**

Commence par présenter :

1. ton diagnostic ;
2. l’architecture proposée ;
3. les décisions à figer immédiatement ;
4. la première tranche verticale ;
5. les risques de sur-conception ;
6. les hypothèses prises.

Puis implémente la tranche proposée.
