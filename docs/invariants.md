[Français](invariants.md) | [English](invariants.en.md)

# Invariants et propriétés de SYTOG

Ce document est la carte vérifiable des propriétés de SYTOG depuis la baseline
`v0.2.0`, mise à jour au fil des expériences de hardening. Il décrit ce que le
système impose, ce que les tests ont seulement démontré, ce qui reste une cible
et les limites déjà connues. Il ne constitue pas une promesse générale au-delà
des périmètres et hypothèses indiqués.

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
| INV-006 | Traitement sûr des événements dupliqués | Garanti |
| INV-007 | Déduplication durable des commandes acceptées | Garanti |
| INV-008 | Linéarisation par l’hôte autoritaire | Garanti |
| INV-009 | Récupération d’une dernière ligne JSONL partielle | Garanti |
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
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L643-L710).

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
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L643-L694) avant écriture.

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

**Statut : Garanti**

**Énoncé exact.** Un préfixe entièrement valide suivi d’un suffixe final non
terminé par `\n` est récupéré en tronquant physiquement le fichier à l’octet
suivant le dernier `\n`. Aucun événement ni reçu du suffixe n’est appliqué.

**Hypothèses et périmètre.** Le caractère `\n` est la frontière de commit
logique d’une ligne. Tout suffixe final non vide et non terminé est considéré
comme non commis, même s’il forme accidentellement un JSON complet. Le préfixe
doit être syntaxiquement valide, satisfaire les invariants de journal et être
rejouable intégralement avant toute troncature. La règle s’applique aux
événements V0 bruts et aux reçus V1.

**Point d’application.**

- [`JournalStore::load`](../crates/sytog-node/src/lib.rs#L739-L838) calcule
  `safe_offset` et `original_length`, puis ne charge que les lignes terminées ;
- [`Host::load_or_create`](../crates/sytog-node/src/lib.rs#L479-L538) valide et rejoue le
  préfixe avant d’autoriser la réparation ;
- [`JournalStore::apply_recovery`](../crates/sytog-node/src/lib.rs#L840-L859) vérifie que
  la longueur n’a pas changé, appelle `set_len(safe_offset)`, synchronise le
  fichier et émet le diagnostic avec les deux offsets.

**Comportement en cas de récupération ou violation.** Une récupération réussie
écrit sur stderr :
`journal recovery: ... from byte <original_length> to safe offset <safe_offset>`.
Un second redémarrage ne détecte plus de suffixe et ne réécrit rien. Si le
fichier change entre inspection et troncature, ou si une opération d’E/S
échoue, l’hôte échoue sans masquer l’erreur.

**Tests existants.**

- `truncated_legacy_final_line_recovers_valid_prefix` ;
- `invalid_final_bytes_recover_valid_prefix` ;
- `truncated_v1_receipt_recovers_once` ;
- `truncated_v1_event_preserves_prior_command_deduplication` ;
- `final_empty_line_is_valid_and_unchanged`.

**Tentative de rupture reproductible.** Copier un journal, noter sa taille,
ajouter un fragment sans `\n`, puis redémarrer. Vérifier le diagnostic,
l’offset physique, la révision reconstruite et l’absence de seconde
modification au redémarrage suivant.

### INV-010 — Refus d’une corruption JSONL intermédiaire

**Statut : Garanti**

**Énoncé exact.** Toute ligne terminée non vide qui ne peut pas être lue ou
désérialisée comme événement V0 brut ou lot V1 reconnu provoque un échec fermé.
Toute entrée JSON syntaxiquement valide mais incohérente avec le schéma, les
séquences, les identifiants ou le réducteur provoque également un échec fermé.

**Hypothèses et périmètre.** La garantie porte sur les erreurs visibles au
lecteur, à `serde_json`, au schéma du reçu, à `EventLogV0::validate` et au
replay. Une ligne invalide terminée, y compris la dernière, n’est jamais
assimilée à une écriture partielle. Si un suffixe incomplet suit une corruption
du préfixe, la validation échoue avant toute troncature.

**Point d’application.**

- le chargement strict et la reconstruction de l’index dans
  [`JournalStore::load`](../crates/sytog-node/src/lib.rs#L739-L838) ;
- la validation et le replay complets dans
  [`Host::load_or_create`](../crates/sytog-node/src/lib.rs#L479-L538) avant
  `apply_recovery`.

**Comportement en cas de violation.** L’hôte refuse de démarrer. Il ne produit
pas un état issu du seul préfixe valide et ne réécrit pas le journal.

**Tests existants.**

- `syntactic_corruption_in_the_middle_fails_without_repair` ;
- `semantic_corruption_fails_without_repair` ;
- `terminated_invalid_final_line_fails_without_repair`.

**Tentative de rupture reproductible.** Sur une copie d’un journal contenant au
moins trois lignes, remplacer la deuxième par `not-json` et redémarrer l’hôte.
Le démarrage doit échouer sans modifier la copie.

## Commandes, concurrence et durabilité

### INV-007 — Déduplication durable des commandes acceptées

**Statut : Garanti**

**Énoncé exact.** Pour une paire stable `(session_id, message_id)` déjà
acceptée, toute nouvelle soumission de la même commande doit retourner le
résultat accepté antérieurement sans décider, persister ni diffuser de nouveaux
événements. Le même identifiant avec un contenu différent doit être une
collision fatale ou un rejet structuré.

**Hypothèses et périmètre.** La garantie s’applique aux commandes acceptées
écrites sous forme de lot versionné V1. Les lignes d’événements brutes produites
par `v0.2.0` restent lisibles, mais ne contiennent pas la requête nécessaire
pour dédupliquer rétrospectivement leurs commandes. L’identité compare tout le
`CommandRequest`, y compris acteur, révision attendue et payload.

**Point d’application.**

- `SubmitCommand` transporte le
  [`CommandRequest`](../crates/sytog-transport/src/lib.rs#L14-L41) ;
- [`Host::submit`](../crates/sytog-node/src/lib.rs#L559-L619) consulte l’index durable
  avant le contrôle de révision ;
- `AcceptedBatchV1` persiste dans une même ligne versionnée la requête acceptée
  et la liste exacte des événements retournés ;
- [`JournalStore::load`](../crates/sytog-node/src/lib.rs#L739-L838) reconstruit l’index
  des commandes au redémarrage.

**Comportement en cas de répétition ou collision.**

- même `message_id` et même requête acceptée : les événements antérieurs sont
  retournés, sans nouvelle décision, écriture, révision ou diffusion globale ;
- même `message_id` accepté avec une requête différente : rejet structuré
  `command_id_collision` avant toute décision ;
- une commande refusée n’est pas enregistrée : son identifiant peut être
  réévalué, y compris après redémarrage. Ce choix est explicite et correspond à
  la politique « seuls les faits acceptés appartiennent au journal canonique ».

**Tests existants.**

- `accepted_command_is_deduplicated_without_new_events` ;
- `accepted_command_id_with_different_content_is_rejected_explicitly` ;
- `accepted_command_deduplication_survives_restart` ;
- `rejected_command_id_can_be_reevaluated` ;
- `host_loads_legacy_events_and_appends_versioned_acceptances` ;
- `concurrent_identical_command_is_appended_once_and_replayed_to_both_callers` ;
- `concurrent_command_id_collision_has_one_winner_and_one_explicit_rejection`.

**Tentative de rupture reproductible.** Soumettre une commande, conserver sa
requête et ses événements, redémarrer l’hôte, puis resoumettre exactement la
même requête. La révision et le journal doivent rester inchangés et la réponse
doit contenir les mêmes événements. Modifier ensuite un seul champ de la
requête en gardant `message_id` : l’hôte doit répondre
`command_id_collision`.

### INV-008 — Linéarisation par l’hôte autoritaire

**Statut : Garanti**

**Énoncé exact.** Dans un processus hôte V0.2, les commandes de session et
d’activité qui atteignent `Host::join` ou `Host::submit` sont traitées une à la
fois sous le même verrou canonique. Décision, validation prospective, append
durable, commit mémoire et émission du lot ne peuvent pas s’entrelacer avec une
autre commande. Les événements d’un même reçu restent atomiques et contigus.
L’ordre des commandes acceptées dans le journal est l’unique ordre canonique.

**Hypothèses et périmètre.** Il existe un seul processus autoritaire et une
seule instance `Host`. L’ordre d’acquisition du verrou entre commandes
concurrentes n’est pas prédéterminé ni nécessairement identique entre deux
exécutions. La garantie ne porte ni sur l’ordre d’arrivée réseau, ni sur l’ordre
temporel global, ni sur l’équité ou l’absence de famine. Une décision
d’activité lente retarde les commandes suivantes. Seul l’ordre finalement
synchronisé dans le journal est canonique et reproductible par replay.

**Point d’application.**

- verrou dans [`Host::join`](../crates/sytog-node/src/lib.rs#L540-L557) ;
- verrou et contrôle de révision dans
  [`Host::submit`](../crates/sytog-node/src/lib.rs#L559-L586) ;
- validation, persistance et commit restent sous cette garde jusqu’à la fin de
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L643-L710).

**Comportement en cas de concurrence.**

- deux commandes distinctes à la même révision : la première acceptée avance
  la révision ; l’autre est refusée avec `revision_conflict` ;
- même commande acceptée soumise deux fois : un append unique et le même
  résultat pour les deux appelants selon INV-007 ;
- même `message_id` et contenus différents : une acceptation éventuelle puis
  `command_id_collision` ;
- une connexion fermée n’annule pas une commande dont l’append durable a
  réussi ;
- une rafale ne perd aucune issue interne : chaque soumission observée par
  l’hôte devient succès, doublon ou rejet structuré. Un client déconnecté peut
  ne pas recevoir cette issue et doit reprendre par déduplication ou catch-up.

Il n’existe ni fusion ni ordre distribué entre plusieurs autorités.

**Tests existants.**

- `concurrent_distinct_commands_at_one_revision_are_linearized` ;
- `concurrent_identical_command_is_appended_once_and_replayed_to_both_callers` ;
- `concurrent_command_id_collision_has_one_winner_and_one_explicit_rejection` ;
- `slow_command_holds_its_place_without_partial_interleaving` ;
- `separate_connections_share_one_canonical_order_and_catch_up_state` ;
- `disconnect_during_acceptance_does_not_erase_a_durable_command` ;
- `concurrent_burst_accounts_for_every_command_without_history_gaps` ;
- `concurrent_order_and_receipts_survive_restart_and_replay`.

**Tentative de rupture reproductible.** Ouvrir plusieurs connexions, les
libérer simultanément avec une même révision, retarder artificiellement une
décision, puis fermer une connexion pendant son traitement. Vérifier les
séquences, `event_id`, reçus physiques V1, contiguïté multi-événements,
réponses, diffusion et catch-up. Redémarrer et comparer exactement état,
événements et index des reçus.

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

- ordre de [`Host::commit`](../crates/sytog-node/src/lib.rs#L643-L710) ;
- écriture et synchronisation dans
  [`JournalStore::append_accepted`](../crates/sytog-node/src/lib.rs#L861-L876).

**Comportement en cas de violation.** Une erreur d’append devient
`persistence_failed` et empêche le commit mémoire et la diffusion. Si l’écriture
a laissé un suffixe final non terminé, le prochain redémarrage applique INV-009
après validation du préfixe.

**Tests existants.** Les tests de déduplication après redémarrage et de journal
mixte V0/V1 couvrent le chemin de succès.
`disconnect_during_acceptance_does_not_erase_a_durable_command` démontre
qu’une perte de connexion ne retire pas un fait accepté.
`concurrent_order_and_receipts_survive_restart_and_replay` compare l’histoire
avant et après redémarrage. Aucun test n’injecte encore un crash ou une erreur à
chaque point de l’append.

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
  [`connect_client`](../crates/sytog-node/src/lib.rs#L253-L293) ;
- flux canonique produit après commit dans
  [`Host::commit`](../crates/sytog-node/src/lib.rs#L643-L710).

**Comportement en cas de violation.** Les trous détectés déclenchent un
catch-up. Aucune comparaison automatique de hash ou d’état avec l’hôte ne
détecte actuellement une divergence silencieuse.

**Tests et expériences existants.**

- `sytog_node::tests::two_participants_converge_and_catch_up_from_the_journal`
  exerce deux participants et le suffixe d’événements ;
- `separate_connections_share_one_canonical_order_and_catch_up_state` exerce
  deux soumissions WebSocket concurrentes puis deux réplicas neufs rattrapés
  depuis la séquence zéro ;
- le parcours manuel V0.2 a produit des snapshots hôte, Alice et Bob
  sémantiquement et octet pour octet identiques avec la même implémentation.

**Tentative de rupture reproductible.** Capturer un journal, le livrer par lots
de tailles et délais différents à deux réducteurs neufs, puis comparer leurs
états. Répéter en supprimant, dupliquant et altérant un événement afin de
vérifier que chaque écart est détecté plutôt que silencieusement réduit.

### INV-006 — Traitement sûr des événements dupliqués

**Statut : Garanti**

**Énoncé exact.** Un événement déjà appliqué doit être ignoré uniquement
si son `event_id`, sa séquence et son contenu sont strictement identiques au
fait canonique connu. Le même `event_id` ou la même séquence avec un contenu
différent doit déclencher une violation d’invariant.

**Hypothèses et périmètre.** La garantie s’applique aux événements dont
l’identité complète est présente dans l’historique V1 du client. Un ancien
snapshot V0 ne contient que l’état et sa révision : un événement antérieur à
son historique disponible est donc refusé avec
`EventHistoryUnavailable`, jamais supposé identique. Le journal canonique reste
en plus protégé par INV-001 et INV-002.

**Point d’application.**

- `ClientReplicaV1` persiste le snapshot, la révision de base de l’historique et
  les événements connus ;
- [`ClientReplica::apply_received_event`](../crates/sytog-node/src/lib.rs#L965-L1008)
  compare l’événement complet par séquence et recherche toute réutilisation de
  `event_id` avant réduction ;
- [`load_client_replica`](../crates/sytog-node/src/lib.rs#L1019-L1034) valide l’historique
  versionné au redémarrage.

**Comportement en cas de doublon ou collision.**

- événement strictement égal déjà connu : `AlreadySeen`, sans modification ;
- même `event_id` avec contenu ou séquence différents :
  `EventIdCollision` et arrêt fermé ;
- même séquence avec un autre événement : `EventSequenceCollision` et arrêt
  fermé ;
- séquence ancienne non vérifiable faute d’historique :
  `EventHistoryUnavailable` ;
- séquence future non contiguë : demande de catch-up.

**Tests existants.**

- `identical_received_event_is_a_safe_noop` ;
- `reused_event_id_with_different_content_is_rejected` ;
- `old_sequence_with_different_content_is_rejected` ;
- `received_event_identity_survives_client_restart`.

**Tentative de rupture reproductible.** Amener un client V1 à la révision N,
puis lui envoyer successivement l’événement canonique N, le même `event_id`
avec un payload différent, puis un autre `event_id` à la séquence N. Seul le
premier doit être un no-op ; les deux suivants doivent échouer explicitement.

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
  [`handle_connection`](../crates/sytog-node/src/lib.rs#L350-L430) ;
- détection d’un trou et nouvelle demande dans
  [`connect_client`](../crates/sytog-node/src/lib.rs#L257-L285).

**Comportement en cas de violation.** Un trou visible provoque une nouvelle
demande depuis la révision locale. Il n’existe ni timeout de convergence, ni
snapshot réseau effectivement envoyé, ni stratégie si le suffixe n’est plus
disponible.

**Tests et expériences existants.**

- `two_participants_converge_and_catch_up_from_the_journal` vérifie
  `events_after(3)` ;
- `separate_connections_share_one_canonical_order_and_catch_up_state` vérifie
  un `Hello` WebSocket depuis zéro et la convergence de deux réplicas neufs ;
- `persisted_old_replica_catches_up_large_suffix_after_host_restart` persiste
  un réplica à la révision 25, produit un suffixe de 276 événements — supérieur
  à la capacité de 256 lots du canal broadcast —, redémarre l’hôte, puis
  vérifie le lot `26..301`, la convergence et une nouvelle relecture locale ;
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
L’historique d’identité du client V1 conserve également tous les événements
reçus après sa révision de base. Un récepteur en retard retombe sur ce même
rattrapage complet. Chaque connexion acceptée crée en outre une tâche sans
quota global ou par adresse. Il n’existe ni pagination, fenêtre maximale,
compaction, limite de connexion, timeout d’écriture, quota ni refus de
surcharge.

**Point d’application actuel.**

- création sans limite des tâches de connexion dans
  [`serve`](../crates/sytog-node/src/lib.rs#L137-L169) ;
- capacité du canal dans
  [`Host::load_or_create`](../crates/sytog-node/src/lib.rs#L526-L537) ;
- récupération après `Lagged` dans
  [`handle_connection`](../crates/sytog-node/src/lib.rs#L440-L451) ;
- clone non borné du suffixe dans
  [`Host::events_after`](../crates/sytog-node/src/lib.rs#L712-L721).

**Comportement actuel sous pression.** L’émission dans le canal n’attend pas les
abonnés : les commits continuent et les lots les plus anciens sont évincés pour
un récepteur lent. À son prochain `recv`, celui-ci observe `Lagged`, clone tout
le suffixe manquant sous le verrou canonique, puis tente de le transmettre dans
un unique `EventBatch`. Le journal mémoire et l’historique client croissent avec
la session ; le clone et la sérialisation du catch-up croissent avec le suffixe.
Une écriture réseau lente bloque sa tâche de connexion, mais pas explicitement
le producteur canonique. Aucune borne de mémoire, de connexion, de taille de
lot, de délai d’écriture ou de service n’est garantie.

**Contrat proposé, partiellement typé mais non appliqué.** La tranche
protocolaire de l’[ADR 0008](adr/0008-overload-and-backpressure-contract.md)
définit des enveloppes V2 distinctes, `ResyncRequired`, des raisons stables, un
rejet avant admission et les métadonnées validées d’un catch-up paginé. Le nœud
continue toutefois d’émettre et traiter V1 : aucune limite d’exécution décrite
ci-dessous n’est encore active. L’ADR sépare quatre domaines de pression :

- admission autoritative globale bornée, avec rejet `server_overloaded` avant
  tout fait canonique, mais sans attente d’un consommateur après admission ;
- file de sortie par connexion bornée en nombre **et** en octets, écrite par
  une tâche dédiée avec timeout ;
- dépassement de file ou timeout transformé en `ResyncRequired`, puis fermeture
  explicite ; toute fermeture impose une reconnexion depuis la dernière
  séquence contiguë appliquée et persistée par le client ;
- catch-up paginé, borné en événements, octets, durée et concurrence, sans
  entrelacement invisible avec le flux live ;
- distinction entre archive canonique durable, fenêtre chaude de rattrapage et
  snapshots ; aucun compactage destructif avant validation du replay
  snapshot-plus-suffixe ;
- resync complet explicite lorsqu’un curseur précède la rétention disponible.

Ce contrat ne promet pas la livraison de bout en bout. Il promet qu’aucune
perte de notification ou reprise ne sera présentée comme une continuité
livrée : le curseur de récupération appartient au client et représente
uniquement les faits appliqués contigus qu’il a persistés localement.

**Critère de reclassement.** INV-012 reste `Réfuté / limité` tant que les bornes
de connexion, admission, file, octets, écriture et catch-up ne sont pas
implémentées et testées. Il restera au moins `Limité` tant que le journal
complet, l’index des commandes acceptées ou l’état canonique peuvent croître
sans borne. Rétention locale finie, rattrapage historique arbitraire et absence
d’archive externe ne peuvent pas être garantis simultanément.

**Tests existants.**

- `persisted_old_replica_catches_up_large_suffix_after_host_restart` observe un
  lot unique de 276 événements ;
- `saturated_broadcast_drops_old_batches_without_blocking_commits_and_recovers`
  laisse un abonné inactif pendant 300 commits, observe `Lagged`, puis vérifie
  la récupération contiguë des 300 événements et la convergence.

**Tentative de rupture reproductible.** Produire un grand journal, maintenir un
client qui ne lit pas sa socket, puis demander depuis la séquence zéro avec un
second client. Mesurer mémoire, taille du lot, latence des commandes et
comportement lors du dépassement des 256 lots.

## Ordre proposé des expériences de rupture

1. **Doublons et collisions — terminée** : l’identité et l’idempotence durable
   des commandes acceptées sont désormais définies.
2. **Corruption JSONL — terminée** : la frontière de commit et la récupération
   d’un suffixe incomplet sont désormais définies.
3. **Concurrence — terminée** : la linéarisation mono-hôte, la contiguïté et la
   durabilité de l’ordre sont caractérisées.
4. **Reconnexion ancienne — terminée** : un réplica persisté rattrape après
   redémarrage un suffixe plus grand que la capacité du canal broadcast.
5. **Pression et backpressure — caractérisée** : le producteur ne ralentit pas,
   le canal évince les anciens lots et le rattrapage complet reste non borné ;
   INV-012 demeure réfuté / limité.

Les cinq expériences de rupture sont maintenant caractérisées. La prochaine
étape n’est plus une mesure exploratoire : elle exige la validation humaine des
limites et choix protocolaires de l’ADR 0008 avant toute correction de
production.
