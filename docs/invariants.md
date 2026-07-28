[Français](invariants.md) | [English](invariants.en.md)

# Invariants et propriétés de SYTOG

Ce document est la carte vérifiable des propriétés de SYTOG à la baseline
`v0.2.0`. Il décrit ce que le système impose, ce que les tests ont seulement
démontré, ce qui reste une cible et les limites déjà connues. Il ne constitue
pas une promesse générale au-delà des périmètres et hypothèses indiqués.

## Statuts

| Statut | Signification |
| --- | --- |
| **Garanti** | Tous les chemins indiqués imposent actuellement la propriété par le code. |
| **Démontré** | Un test ou une expérience l’a observée sous les hypothèses indiquées, sans preuve générale. |
| **Cible** | La propriété est souhaitée mais n’est pas encore assurée. |
| **Réfuté / limité** | Un contre-exemple ou une limite précise est déjà connu. |

Un statut porte toujours sur le périmètre écrit dans la même section. Par
exemple, une propriété garantie par `EventLogV0::validate` ne l’est pas
nécessairement pour un événement réseau que le client ignore avant validation.

## Vue d’ensemble

| Identifiant | Propriété | Statut actuel |
| --- | --- | --- |
| INV-001 | Continuité des séquences | Garanti |
| INV-002 | Unicité de `event_id` dans le journal canonique | Garanti |
| INV-003 | Répétabilité de `causation_id` | Garanti |
| INV-004 | Replay déterministe avec la même implémentation | Démontré |
| INV-005 | Convergence sémantique des réplicas clients | Démontré |
| INV-006 | Traitement sûr des événements dupliqués | Réfuté / limité |
| INV-007 | Déduplication durable des commandes | Cible |
| INV-008 | Linéarisation par l’hôte autoritaire | Garanti |
| INV-009 | Récupération d’une dernière ligne JSONL partielle | Réfuté / limité |
| INV-010 | Refus d’une corruption JSONL intermédiaire | Garanti |
| INV-011 | Reconnexion et rattrapage par séquence | Démontré |
| INV-012 | Mémoire et backpressure bornées | Réfuté / limité |
| INV-013 | Persistance avant commit mémoire et diffusion | Garanti |

## Journal et replay

### INV-001 — Continuité des séquences

**Statut : Garanti**

**Énoncé exact.** Dans tout `EventLogV0` accepté, l’événement d’index `i`
porte la séquence `base_revision + i + 1`, sans trou, doublon ni retour en
arrière. Le journal canonique V0.2 utilise `base_revision = 0`. Un
`SessionState` n’applique également que l’événement suivant sa révision.

**Hypothèses et périmètre.** La garantie s’applique aux journaux passés par
`EventLogV0::validate` et aux événements passés par `SessionState::apply`. Elle
ne dit rien d’un fichier JSONL qui n’a pas encore été chargé et validé.

**Point d’application.**

- [`EventLogV0::validate`](../crates/sytog-protocol/src/lib.rs#L37-L73)
  calcule la séquence attendue ;
- [`SessionState::apply`](../crates/sytog-domain/src/lib.rs#L107-L131) refuse
  toute autre séquence ;
- le nœud valide le journal prospectif avant persistance dans
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L520-L557).

**Comportement en cas de violation.** Le validateur retourne
`UnexpectedEventSequence`, ou le réducteur retourne `UnexpectedSequence`.
`replay_log` s’arrête sans produire un état partiellement accepté.

**Tests existants.**

- `sytog_protocol::tests::log_rejects_sequence_gaps` ;
- `sytog_runtime::tests::multi_event_application_is_atomic`.

**Tentative de rupture reproductible.** Copier un journal, supprimer sa
deuxième ligne ou changer une séquence de `2` à `3`, puis essayer de reconstruire
un hôte depuis cette copie. Le démarrage doit échouer avant le replay complet.

### INV-002 — Unicité de `event_id` dans le journal canonique

**Statut : Garanti**

**Énoncé exact.** Deux événements d’un même `EventLogV0` validé ne peuvent pas
partager le même `event_id`, que leurs autres champs soient identiques ou non.

**Hypothèses et périmètre.** La garantie porte sur le journal complet validé au
chargement et sur le journal prospectif construit par l’hôte avant chaque
append. Elle ne couvre pas le chemin client décrit par INV-006.

**Point d’application.**

- l’ensemble `event_ids` de
  [`EventLogV0::validate`](../crates/sytog-protocol/src/lib.rs#L51-L70) ;
- la validation prospective de
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L526-L543) avant écriture.

**Comportement en cas de violation.** Le journal est refusé avec
`ProtocolError::DuplicateEventId`. L’hôte transforme une collision prospective
en rejet `journal_invariant_failed` et ne commit pas la commande.

**Tests existants.**

- `sytog_protocol::tests::log_rejects_duplicate_event_ids`.

**Tentative de rupture reproductible.** Copier une ligne JSONL, lui attribuer
la séquence suivante sans changer `event_id`, puis redémarrer l’hôte sur cette
copie. Il doit refuser le journal avec un identifiant dupliqué.

### INV-003 — Répétabilité de `causation_id`

**Statut : Garanti**

**Énoncé exact.** `causation_id` n’est pas une clé unique. Plusieurs événements
valides peuvent porter le même `causation_id` si leurs `event_id` et séquences
sont distincts. `EventId::from_causation` permet de les distinguer par ordinal.

**Hypothèses et périmètre.** Le validateur garantit que le champ est non vide,
mais ne vérifie ni l’existence de la commande causale ni que tous ses événements
partagent effectivement cet identifiant. Cette propriété exprime une permission
du modèle, pas une preuve de traçabilité causale complète.

**Point d’application.**

- [`EventId::from_causation`](../crates/sytog-domain/src/lib.rs#L32-L36) ;
- [`SessionEvent`](../crates/sytog-domain/src/lib.rs#L294-L302) sépare
  `event_id` et `causation_id` ;
- [`EventLogV0::validate`](../crates/sytog-protocol/src/lib.rs#L51-L70)
  n’impose l’unicité qu’à `event_id`.

**Comportement en cas de violation.** Répéter `causation_id` n’est pas une
violation. Un identifiant vide est refusé ; une collision de `event_id` est
refusée selon INV-002.

**Tests existants.**

- `sytog_protocol::tests::log_allows_shared_causation_with_unique_event_ids`.

**Tentative de rupture reproductible.** Construire deux événements contigus
avec le même `causation_id` et les identifiants `<cause>:0` et `<cause>:1`.
Le journal doit être accepté. Réutiliser ensuite `<cause>:0` doit déclencher
INV-002.

### INV-004 — Replay déterministe avec la même implémentation

**Statut : Démontré**

**Énoncé exact.** Pour un état initial, un journal valide, la même version du
réducteur Rust et les mêmes dépendances, deux replays appliquent les événements
dans le même ordre et produisent des `SessionState` sémantiquement égaux.

**Hypothèses et périmètre.** Le journal, la session et la révision de base sont
valides. La propriété ne garantit pas encore des octets ou un hash identiques
entre implémentations, sérialiseurs ou versions différentes : aucune
sérialisation canonique portable n’est spécifiée.

**Point d’application.**

- [`replay`](../crates/sytog-runtime/src/lib.rs#L234-L244) est une réduction
  ordonnée sans effet externe ;
- [`replay_log`](../crates/sytog-runtime/src/lib.rs#L246-L264) valide identité,
  base et journal avant réduction.

**Comportement en cas de violation.** Une erreur de protocole, de session, de
révision de base ou d’application arrête le replay. Une divergence silencieuse
entre deux replays valides serait une défaillance critique actuellement non
détectée automatiquement par un hash canonique.

**Tests existants.**

- `sytog_runtime::tests::replay_reconstructs_exact_state` ;
- `sytog_runtime::tests::replay_log_rejects_the_wrong_session` ;
- `sytog_node::tests::host_restarts_from_its_durable_journal`.

**Tentative de rupture reproductible.** Rejouer deux fois le même fixture
depuis `SessionState::uninitialized`, sérialiser les deux états avec la même
configuration et comparer leur égalité sémantique. Répéter avec une séquence
manquante pour vérifier un refus plutôt qu’une divergence.

### INV-009 — Récupération d’une dernière ligne JSONL partielle

**Statut : Réfuté / limité**

**Énoncé exact visé.** Après un crash ayant laissé uniquement la dernière ligne
JSONL incomplète, l’hôte devrait identifier avec certitude ce suffixe non commis,
le retirer et reconstruire le dernier préfixe durable valide.

**État actuel, hypothèses et périmètre.** Cette récupération n’existe pas.
`load_events` désérialise toutes les lignes non vides et propage la première
erreur JSON. Une dernière ligne tronquée bloque donc actuellement le
redémarrage, comme toute autre corruption.

**Point d’application.**

- lecture stricte dans
  [`JournalStore::load_events`](../crates/sytog-node/src/lib.rs#L587-L601) ;
- l’append écrit un lot puis appelle `sync_data` dans
  [`JournalStore::append_events`](../crates/sytog-node/src/lib.rs#L603-L616),
  sans framing ni marqueur de commit.

**Comportement en cas de violation.** Le chargement retourne `NodeError::Json`
ou une erreur d’entrée/sortie. L’hôte ne démarre pas et ne tronque rien
automatiquement.

**Tests existants.** Aucun test ne couvre une écriture finale partielle.

**Tentative de rupture reproductible.** Copier un répertoire de session,
tronquer les derniers octets de `events.jsonl` au milieu du dernier objet JSON,
puis démarrer l’hôte sur la copie. En V0.2.0, le démarrage doit échouer : ce
contre-exemple confirme la limite.

### INV-010 — Refus d’une corruption JSONL intermédiaire

**Statut : Garanti**

**Énoncé exact.** Si une ligne non vide quelconque du journal JSONL ne peut pas
être lue ou désérialisée comme `SessionEvent`, le chargement échoue. Aucun
suffixe suivant cette ligne n’est rejoué silencieusement.

**Hypothèses et périmètre.** La garantie porte sur les erreurs visibles au
lecteur de lignes et à `serde_json`. Une modification qui reste un JSON
structurellement valide est ensuite soumise aux invariants du protocole et du
réducteur.

**Point d’application.**

- la collecte en `Result<Vec<SessionEvent>, NodeError>` de
  [`JournalStore::load_events`](../crates/sytog-node/src/lib.rs#L587-L601) ;
- la validation et le replay complets dans
  [`Host::load_or_create`](../crates/sytog-node/src/lib.rs#L397-L430).

**Comportement en cas de violation.** L’hôte refuse de démarrer. Il ne produit
pas un état issu du seul préfixe valide et ne réécrit pas le journal.

**Tests existants.** Aucun test dédié à une corruption intermédiaire ; la
garantie est imposée par le chemin de chargement mais doit encore recevoir un
test de régression.

**Tentative de rupture reproductible.** Sur une copie d’un journal contenant au
moins trois lignes, remplacer la deuxième par `not-json` et redémarrer l’hôte.
Le démarrage doit échouer sans modifier la copie.

## Commandes, concurrence et durabilité

### INV-007 — Déduplication durable des commandes

**Statut : Cible**

**Énoncé exact visé.** Pour une paire stable `(session_id, message_id)` déjà
acceptée, toute nouvelle soumission de la même commande doit retourner le
résultat accepté antérieurement sans décider, persister ni diffuser de nouveaux
événements. Le même identifiant avec un contenu différent doit être une
collision fatale ou un rejet structuré.

**État actuel, hypothèses et périmètre.** Aucun registre durable des commandes
et de leurs réponses n’existe. Une répétition après acceptation est souvent
refusée indirectement par `expected_revision`, mais le système ne sait pas
répondre « commande déjà connue » ni restituer sa réponse initiale.

**Point d’application actuel.**

- `SubmitCommand` transporte le
  [`CommandRequest`](../crates/sytog-transport/src/lib.rs#L14-L41) ;
- [`Host::submit`](../crates/sytog-node/src/lib.rs#L460-L503) vérifie la
  révision puis redécide, sans index de `message_id` ;
- les fichiers persistés ne contiennent que les événements, pas les réponses
  de commandes.

**Comportement actuel en cas de répétition.** Selon la révision et l’état, la
commande peut être refusée comme obsolète ou réévaluée. Aucune sémantique
exactly-once durable n’est garantie.

**Tests existants.** Aucun test de resoumission du même `message_id` après
acceptation ou redémarrage.

**Tentative de rupture reproductible.** Soumettre une commande acceptée,
interrompre la connexion avant réception de sa réponse, redémarrer le client
avec son ancienne révision et resoumettre exactement le même `message_id`.
Observer qu’aucune réponse acceptée historique n’est disponible.

### INV-008 — Linéarisation par l’hôte autoritaire

**Statut : Garanti**

**Énoncé exact.** Dans un processus hôte V0.2, les commandes de session et
d’activité qui atteignent `Host::join` ou `Host::submit` sont traitées une à la
fois sous le même verrou canonique. Chaque commande acceptée observe une
révision et inscrit ses événements dans un ordre total unique.

**Hypothèses et périmètre.** Il existe un seul processus autoritaire et une
seule instance `Host`. L’ordre d’acquisition du verrou entre commandes
concurrentes n’est pas prédéterminé ni nécessairement identique entre deux
exécutions. Seul l’ordre finalement inscrit au journal est canonique et
reproductible par replay.

**Point d’application.**

- verrou dans [`Host::join`](../crates/sytog-node/src/lib.rs#L441-L458) ;
- verrou et contrôle de révision dans
  [`Host::submit`](../crates/sytog-node/src/lib.rs#L460-L471) ;
- validation, persistance et commit restent sous cette garde jusqu’à la fin de
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L520-L557).

**Comportement en cas de concurrence.** Une commande gagne le verrou et peut
être acceptée. Une autre commande portant la même `expected_revision` est
ensuite refusée avec `revision_conflict`. Il n’existe ni fusion ni ordre
distribué entre plusieurs autorités.

**Tests existants.**

- `sytog_node::tests::two_participants_converge_and_catch_up_from_the_journal`
  vérifie le refus d’une révision obsolète ;
- aucun test ne déclenche encore réellement plusieurs soumissions simultanées.

**Tentative de rupture reproductible.** Ouvrir dix connexions, leur donner la
même dernière révision, puis libérer simultanément dix commandes valides.
Vérifier qu’un seul ordre contigu apparaît au journal, que les perdants
obsolètes reçoivent un refus structuré et que le replay reproduit cet ordre.

### INV-013 — Persistance avant commit mémoire et diffusion

**Statut : Garanti**

**Énoncé exact.** Lorsqu’un append retourne avec succès, l’hôte a validé le
journal prospectif, écrit le lot et appelé `sync_data` avant de remplacer son
état canonique en mémoire et avant de diffuser les événements.

**Hypothèses et périmètre.** La garantie suppose que le système de fichiers et
`sync_data` respectent leur contrat, et que l’append retourne normalement. Elle
ne garantit pas l’atomicité physique du lot : une erreur ou un crash pendant
`write_all` peut laisser un suffixe partiel tout en empêchant le commit mémoire.

**Point d’application.**

- ordre de [`Host::commit`](../crates/sytog-node/src/lib.rs#L520-L557) ;
- écriture et synchronisation dans
  [`JournalStore::append_events`](../crates/sytog-node/src/lib.rs#L603-L616).

**Comportement en cas de violation.** Une erreur d’append devient
`persistence_failed` et empêche le commit mémoire et la diffusion. Si l’écriture
a été partielle, le prochain redémarrage rencontre actuellement INV-009.

**Tests existants.** Aucun test injectant un crash ou une erreur à chaque point
de l’append. Le test de redémarrage ne couvre que le chemin de succès.

**Tentative de rupture reproductible.** Utiliser un stockage instrumenté qui
échoue après N octets pour chaque valeur de N dans un lot multi-événements.
Après chaque échec, vérifier qu’aucun événement n’a été diffusé et mesurer si le
journal peut redémarrer sans intervention.

## Réseau et convergence

### INV-005 — Convergence sémantique des réplicas clients

**Statut : Démontré**

**Énoncé exact.** Des clients partant du même état et réduisant le même flux
canonique, complet et ordonné avec la même version du code aboutissent à des
`SessionState` sémantiquement égaux.

**Hypothèses et périmètre.** L’hôte est unique, les événements ne sont pas
altérés, tous les événements manquants finissent par arriver et clients comme
hôte utilisent le même schéma et le même réducteur. L’égalité de hash observée
en V0.2 ne constitue pas une garantie inter-implémentations : l’ordre des
champs, Unicode, nombres, options, whitespace et algorithme de hash ne sont pas
spécifiés comme sérialisation canonique.

**Point d’application.**

- réduction locale dans
  [`connect_client`](../crates/sytog-node/src/lib.rs#L201-L240) ;
- flux canonique produit après commit dans
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L520-L557).

**Comportement en cas de violation.** Les trous détectés déclenchent un
catch-up. Aucune comparaison automatique de hash ou d’état avec l’hôte ne
détecte actuellement une divergence silencieuse.

**Tests et expériences existants.**

- `sytog_node::tests::two_participants_converge_and_catch_up_from_the_journal`
  exerce deux participants et le suffixe d’événements ;
- le parcours manuel V0.2 a produit des snapshots hôte, Alice et Bob
  sémantiquement et octet pour octet identiques avec la même implémentation.

**Tentative de rupture reproductible.** Capturer un journal, le livrer par lots
de tailles et délais différents à deux réducteurs neufs, puis comparer leurs
états. Répéter en supprimant, dupliquant et altérant un événement afin de
vérifier que chaque écart est détecté plutôt que silencieusement réduit.

### INV-006 — Traitement sûr des événements dupliqués

**Statut : Réfuté / limité**

**Énoncé exact visé.** Un événement déjà appliqué doit être ignoré uniquement
si son `event_id`, sa séquence et son contenu sont strictement identiques au
fait canonique connu. Le même `event_id` ou la même séquence avec un contenu
différent doit déclencher une violation d’invariant.

**État actuel, hypothèses et périmètre.** Sur le chemin réseau client, tout
événement dont `sequence <= local.revision` est ignoré sans comparer
`event_id`, `causation_id`, acteur, portée ou payload. Un faux événement ancien
avec un contenu différent peut donc être silencieusement ignoré. Le journal
canonique complet reste protégé par INV-001 et INV-002.

**Point d’application actuel.**

- branche d’ignorance dans
  [`connect_client`](../crates/sytog-node/src/lib.rs#L205-L215) ;
- aucune table locale des événements déjà appliqués n’est conservée dans le
  snapshot client.

**Comportement actuel en cas de doublon.** Une séquence ancienne est ignorée,
qu’elle soit identique ou contradictoire. Une séquence future non contiguë
déclenche un catch-up.

**Tests existants.** Aucun test n’envoie au client un ancien événement modifié
ou une collision d’identifiant via WebSocket.

**Tentative de rupture reproductible.** Amener un client à la révision N, puis
lui envoyer un `EventBatch` contenant un événement de séquence N avec un
`event_id` ou payload différent. En V0.2.0, le client l’ignore sans erreur : ce
contre-exemple confirme la limite.

### INV-011 — Reconnexion et rattrapage par séquence

**Statut : Démontré**

**Énoncé exact.** Un client disposant d’un snapshot local à la révision N peut
annoncer N, demander les événements strictement postérieurs et réduire le
suffixe contigu jusqu’à la révision courante de l’hôte.

**Hypothèses et périmètre.** L’hôte possède encore tout le journal en mémoire,
la session et le snapshot local sont valides, la connexion finit par livrer le
suffixe et aucune compaction n’a supprimé les événements requis.

**Point d’application.**

- `Hello.last_sequence` et `CatchUpRequest.after_sequence` dans
  [`NetworkMessage`](../crates/sytog-transport/src/lib.rs#L14-L41) ;
- réponse au hello et au catch-up dans
  [`handle_connection`](../crates/sytog-node/src/lib.rs#L297-L357) ;
- détection d’un trou et nouvelle demande dans
  [`connect_client`](../crates/sytog-node/src/lib.rs#L205-L233).

**Comportement en cas de violation.** Un trou visible provoque une nouvelle
demande depuis la révision locale. Il n’existe ni timeout de convergence, ni
snapshot réseau effectivement envoyé, ni stratégie si le suffixe n’est plus
disponible.

**Tests et expériences existants.**

- le test `two_participants_converge_and_catch_up_from_the_journal` vérifie
  `events_after(3)` mais pas une reconnexion WebSocket complète ;
- la reconnexion d’un client en retard et le redémarrage de l’hôte ont été
  vérifiés manuellement pendant la V0.2.

**Tentative de rupture reproductible.** Déconnecter Bob à N, produire plusieurs
événements avec Alice, reconnecter Bob avec son ancien snapshot, puis vérifier
qu’il reçoit exactement `N+1..courant`. Refaire avec un snapshot très ancien et
un retard artificiel entre les lots.

### INV-012 — Mémoire et backpressure bornées

**Statut : Réfuté / limité**

**Énoncé exact visé.** La mémoire utilisée par l’hôte, la quantité clonée pour
un catch-up et le travail en attente pour un client lent doivent avoir des
bornes explicites et un comportement de surcharge documenté.

**État actuel, hypothèses et périmètre.** Le canal broadcast est borné à 256
lots, mais le journal canonique reste intégralement dans un `Vec`. Chaque
`events_after` filtre et clone tout le suffixe demandé dans un nouveau `Vec`.
Un récepteur en retard retombe sur ce même rattrapage complet. Il n’existe ni
pagination, fenêtre maximale, compaction, quota ni refus de surcharge.

**Point d’application actuel.**

- capacité du canal dans
  [`Host::load_or_create`](../crates/sytog-node/src/lib.rs#L431-L438) ;
- récupération après `Lagged` dans
  [`handle_connection`](../crates/sytog-node/src/lib.rs#L366-L377) ;
- clone non borné du suffixe dans
  [`Host::events_after`](../crates/sytog-node/src/lib.rs#L560-L569).

**Comportement actuel sous pression.** Le journal mémoire croît avec la session.
Un catch-up ancien alloue proportionnellement au suffixe. Un client lent peut
prendre du retard ; sa tâche tente ensuite de cloner et transmettre tous les
événements manquants. Aucune borne de service n’est garantie.

**Tests existants.** Aucun test de charge, de client lent, de canal saturé ou de
client très ancien.

**Tentative de rupture reproductible.** Produire un grand journal, maintenir un
client qui ne lit pas sa socket, puis demander depuis la séquence zéro avec un
second client. Mesurer mémoire, taille du lot, latence des commandes et
comportement lors du dépassement des 256 lots.

## Ordre proposé des expériences de rupture

1. **Doublons et collisions** — préciser l’identité de l’histoire avant toute
   autre mesure.
2. **Corruption JSONL** — établir ce qui est durablement commis et récupérable.
3. **Concurrence** — éprouver l’ordre canonique une fois l’histoire fiable.
4. **Reconnexion ancienne** — vérifier la convergence sur un suffixe important.
5. **Pression et backpressure** — mesurer les bornes après stabilisation des
   sémantiques précédentes.

Les deux premières familles protègent l’intégrité du journal canonique. Les
tests de charge ou de nombreux clients produiraient surtout du bruit tant que
l’identité, l’idempotence et la récupérabilité de cette histoire ne sont pas
définies.
