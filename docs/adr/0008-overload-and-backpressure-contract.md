Langue canonique : Français

English version: [English](0008-overload-and-backpressure-contract.en.md)

[Français](0008-overload-and-backpressure-contract.md) | [English](0008-overload-and-backpressure-contract.en.md)

# ADR 0008 : Contrat de surcharge et de backpressure

Statut : accepté

Note d’implémentation : la tranche protocolaire 1 définit le vocabulaire V2, sa
validation, le décodage versionné, les raisons stables de surcharge et le
contrat de fermeture observable. Le nœud V0.2 continue d’émettre et de traiter
uniquement V1 ; aucun comportement de file, timeout, admission, catch-up,
snapshot ou rétention décrit ici n’est encore actif.

## Contexte

SYTOG V0.2 possède un hôte autoritatif unique. Une commande est décidée,
validée, ajoutée et synchronisée dans le journal canonique, commitée en mémoire,
puis publiée pendant que le verrou canonique est détenu. Le replay exact et
l’ordre du journal font autorité ; la livraison des notifications ne le fait
pas.

Le nœud actuel ne possède aucun contrat explicite de surcharge :

- `serve` crée une tâche pour chaque connexion TCP acceptée sans quota ;
- `Host` conserve en mémoire tout le journal et l’index des commandes acceptées ;
- les lots acceptés utilisent un canal broadcast Tokio de 256 emplacements ;
- la publication n’attend pas les abonnés ;
- un abonné en retard perd les anciens emplacements, reçoit `Lagged`, puis
  clone tout le suffixe manquant depuis le journal mémoire ;
- `Hello` et `CatchUpRequest` renvoient tout le suffixe dans un `EventBatch` ;
- la sérialisation et les écritures WebSocket n’ont aucune limite de taille ou
  de durée ;
- le client conserve tous les événements reçus après sa révision de base.

Les expériences de rupture V0.2 ont montré que 300 commits s’achèvent pendant
qu’un abonné reste inactif. Celui-ci observe ensuite `Lagged` et récupère en
clonant les 300 événements. La vérité canonique est préservée, mais la mémoire,
les connexions, le catch-up et les écritures réseau restent non bornés.

SYTOG privilégie la stabilité, le comportement explicite, le replay exact et la
récupération déterministe plutôt que le débit maximal. Un client lent ou
hostile ne doit ni perdre silencieusement des faits, ni bloquer indéfiniment la
progression autoritative.

## Facteurs de décision

- Préserver le journal canonique, le replay exact et l’ordre total mono-hôte.
- Ne jamais présenter un événement comme livré parce qu’il a été publié ou mis
  en file.
- Empêcher une connexion lente de consommer une mémoire illimitée ou de bloquer
  les autres clients.
- Borner l’admission globale ainsi que le travail par connexion.
- Rendre surcharge, retard, déconnexion et resynchronisation observables.
- Garder un catch-up déterministe depuis un curseur durable appartenant au
  client.
- Préférer rejet ou déconnexion explicites à la perte silencieuse ou l’attente
  indéfinie.
- Séparer autorité durable, notifications et rétention de catch-up.
- Introduire le contrat par petites tranches testables.

## Recommandation

Adopter une stratégie combinée : files bornées par connexion, détection
explicite du retard, délais d’écriture, déconnexion des consommateurs lents et
reconnexion/catch-up déterministes depuis un curseur durable du client. Ajouter
des limites globales d’admission afin que l’hôte puisse refuser du travail avant
commit sans coupler la progression autoritative à la vitesse des consommateurs.

Conserver l’archive durable complète pendant les premières tranches. Paginer et
borner le catch-up avant d’introduire une fenêtre chaude. Ajouter ensuite le
resync snapshot-plus-suffixe, puis séparer la fenêtre chaude de l’archive
exacte. Ne pas utiliser le compactage destructif comme premier mécanisme de
backpressure.

## Quatre domaines de pression distincts

1. **Pression de production autoritative** : admission, séquencement, append
   durable et commit ; elle protège l’hôte entier.
2. **Pression par connexion** : notifications live en attente pour un client ;
   elle isole les clients.
3. **Rétention du journal** : faits historiques disponibles pour replay, audit
   et catch-up.
4. **Pression de catch-up et réseau** : construction des pages, sérialisation,
   durée d’écriture et travail demandé par les clients reconnectés ou hostiles.

## Stratégies étudiées

### A. Ralentir globalement les producteurs autoritatifs

- **Sûreté :** préserve l’ordre et peut éviter la perte de notifications.
- **Disponibilité :** un consommateur bloqué peut arrêter tout travail accepté.
- **Isolation :** aucune ; le client le plus lent contrôle la session.
- **Mémoire :** bornable si les producteurs attendent.
- **Risque de blocage global :** critique.
- **Observation client :** commandes en attente sans borne sans timeout séparé.
- **Replay, catch-up, redémarrage :** compatibles, mais inutilement couplés.
- **Complexité :** mécanique simple, exploitation dangereuse.
- **Invariants :** peut borner les files mais viole l’exigence de progression
  autoritative indépendante des clients.

Rejetée comme contrat principal. L’admission globale peut être bornée et
refuser explicitement, mais ne doit jamais attendre les consommateurs live.

### B. Donner une file de sortie bornée à chaque connexion

- **Sûreté :** sûre si le dépassement ne change pas la vérité canonique et
  n’affirme jamais que des notifications abandonnées ont été livrées.
- **Disponibilité :** l’autorité et les autres clients continuent.
- **Isolation :** forte, sous réserve de quotas globaux.
- **Mémoire :** bornée par connexion seulement avec limites en nombre et octets.
- **Risque global :** faible ; la mémoire agrégée exige un quota de connexions.
- **Observation client :** le dépassement exige une transition explicite.
- **Replay, catch-up, redémarrage :** compatibles après reconnexion au curseur
  durable appliqué.
- **Complexité :** moyenne : writer, comptage, annulation et fermeture.
- **Invariants :** borne le travail live, pas le journal ou le catch-up.

Retenue comme composant, pas comme contrat complet.

### C. Déconnecter les consommateurs trop lents

- **Sûreté :** les événements restent durables ; le motif de fermeture peut se
  perdre, donc toute perte de transport doit imposer la même reconnexion.
- **Disponibilité :** élevée pour l’hôte et les clients sains.
- **Isolation :** forte avec quotas et délais d’écriture.
- **Mémoire :** bornée si la déconnexion suit une file ou un timeout borné.
- **Risque global :** faible.
- **Observation client :** raison best effort puis fermeture ; toute fermeture
  rend la livraison inconnue au-delà du curseur durable du client.
- **Replay, catch-up, redémarrage :** compatibles si la reconnexion est exigée.
- **Complexité :** faible à moyenne ; la sémantique du curseur est essentielle.
- **Invariants :** borne la durée d’une écriture bloquée, sans garantir la
  réception du motif de fermeture.

Retenue avec files bornées et reconnexion déterministe.

### D. Abandonner des notifications intermédiaires et exiger un resync

- **Sûreté :** sûre car le journal, non les notifications, fait autorité.
- **Disponibilité :** élevée.
- **Isolation :** bonne.
- **Mémoire :** bornée pour les files live.
- **Risque global :** faible.
- **Observation client :** dangereuse si invisible ; sûre seulement avec état
  explicite de retard/resync ou fermeture.
- **Replay, catch-up, redémarrage :** naturellement compatibles.
- **Complexité :** moyenne.
- **Invariants :** historique exact, continuité live limitée et jamais présentée
  comme une livraison garantie.

Retenue seulement avec une transition explicite. La perte silencieuse est
rejetée.

### E. File bornée, retard, déconnexion et reconnexion/catch-up

- **Sûreté :** préserve la vérité canonique et explicite la frontière de reprise.
- **Disponibilité :** élevée ; les clients surchargés se reconnectent.
- **Isolation :** forte lorsque toutes les limites sont appliquées.
- **Mémoire :** bornée pour le live et le catch-up concurrent ; le journal est
  séparé.
- **Risque global :** faible hors frontière d’admission autoritative.
- **Observation client :** raison explicite si livrable, sinon fermeture avec
  la même règle de reconnexion.
- **Replay, catch-up, redémarrage :** alignés sur les curseurs durables.
- **Complexité :** moyenne à forte, mais décomposable.
- **Invariants :** travail live borné et reprise visible ; disponibilité du
  catch-up limitée par la rétention.

Retenue comme contrat V0.2.

### F. Borner ou compacter le journal avec snapshots et rétention

- **Sûreté :** dangereuse si le compactage supprime les seuls faits
  autoritatifs ; sûre après validation d’un snapshot comme base de replay et
  conservation d’une archive immuable.
- **Disponibilité :** améliore redémarrage et catch-up ; les clients sous le
  plancher de rétention exigent un resync complet.
- **Isolation :** réduit le coût imposable par un ancien client.
- **Mémoire :** borne le suffixe chaud ; l’archive durable exige sa politique.
- **Risque global :** le compactage doit rester incrémental.
- **Observation client :** expose la première séquence disponible et exige un
  snapshot lorsque le curseur est plus ancien.
- **Replay, catch-up, redémarrage :** compatibles seulement après validation du
  replay snapshot-plus-suffixe.
- **Complexité :** forte ; persistance, reprise et audit changent.
- **Invariants :** rétention chaude bornée, mais historique arbitraire
  impossible sans archive.

Différée jusqu’à validation du replay snapshot-plus-suffixe. Les premières
tranches ne compactent pas destructivement l’archive canonique.

## Matrice de décision

| Stratégie | Sûreté | Disponibilité / isolation | Mémoire | Reprise observable | Replay | Complexité | Décision |
|---|---|---|---|---|---|---|---|
| Ralentissement global | Ordre fort, livraison couplée | Faible / aucune | Potentiellement bornée | Commandes en attente | Compatible | Faible | Rejeter |
| File bornée par connexion | Forte si dépassement explicite | Élevée / forte | Bornée par client | Transition requise | Compatible | Moyenne | Retenir |
| Déconnexion lente | Vérité canonique préservée | Élevée / forte | Bornée avec délais | Fermeture puis reconnexion | Compatible | Moyenne | Retenir |
| Abandon live | Sûr seulement si visible | Élevée / bonne | Bornée | Resync explicite | Compatible | Moyenne | Retenir sous condition |
| File + retard + reconnexion | Forte et explicite | Élevée / forte | Bornée hors journal | Catch-up déterministe | Compatible | Moyenne-forte | Recommander |
| Snapshot + rétention | Forte après replay validé | Élevée / bonne | Données chaudes bornées | Snapshot puis suffixe | Conditionnel | Forte | Différer puis retenir |

## Contrat V0.2 proposé

### 1. Admission des commandes autoritatives

- Les consommateurs lents ne participent jamais au chemin critique du commit.
- L’hôte borne connexions, commandes admises et catch-ups concurrents.
- `server_overloaded` avant admission ne crée aucun fait ni reçu canonique.
- Une commande admise atteint succès durable ou rejet structuré
  indépendamment de la connexion.
- Un commit durable reste accepté après fermeture ; la déduplication par
  `message_id` récupère le résultat.
- Épuisement disque et échec de persistance restent `persistence_failed`.
- Limites et délais sont une configuration explicite.

### 2. Sortie par connexion

- Chaque connexion possède une file de données bornée et un writer.
- La borne porte sur le nombre et les octets sérialisés.
- La publication canonique effectue un enqueue non bloquant.
- Un chemin de contrôle réservé peut annuler le writer et tenter une fermeture.
- Écritures et fermeture ont une échéance.
- Dépassement ou timeout produit exactement une transition
  `live -> resync_required -> closed`.

### 3. Livraison et curseurs

- Publication, mise en file, écriture socket, réception, application et
  persistance client sont des états distincts.
- Le serveur ne prétend pas livrer de bout en bout et n’avance aucun curseur
  confirmé lors d’une simple écriture.
- Le curseur appartient au client : plus haute séquence contiguë appliquée et
  persistée durablement.
- Toute fermeture impose une reconnexion depuis ce curseur ; les doublons sont
  traités selon INV-006.
- `sent_sequence` reste un curseur local d’ordonnancement, pas un accusé.

### 4. Comportement observable d’un consommateur lent

- Signal préféré :
  `ResyncRequired { reason, current_sequence, earliest_available_sequence,
  snapshot_revision }`, puis fermeture WebSocket stable.
- Raisons : `outbound_queue_full`, `outbound_byte_budget_exceeded`,
  `write_timeout`, `catch_up_limit_exceeded`, `cursor_before_retention`.
- Le signal final est best effort ; toute fermeture applique la même règle :
  persister le réplica, reconnecter, annoncer le curseur durable.
- Le serveur ne saute jamais une plage avant de reprendre le live sur la même
  connexion comme si la continuité était préservée.

### 5. Catch-up

- Le catch-up est paginé et chaque page bornée en événements et octets expose
  `from_sequence`, `through_sequence`, `current_sequence` et sa terminalité.
- Le serveur borne concurrence, pages, événements, octets et durées.
- Chaque page est une plage canonique contiguë.
- Le live ne dépasse pas un catch-up inachevé ; la connexion atteint un high
  water mark puis réconcilie le suffixe avant `live`.
- Dépasser un budget ferme avec `catch_up_limit_exceeded`.
- Le client peut reprendre depuis la dernière page appliquée durablement.

### 6. Rétention, snapshots et replay

- Archive canonique durable et fenêtre chaude sont distinctes.
- La première implémentation conserve l’archive append-only complète.
- Avant toute rétention chaude bornée, valider `snapshot N + suffixe
  N+1..courant`.
- Ensuite, annoncer `earliest_available_sequence` ; servir les pages au-dessus
  du plancher, sinon snapshot compatible plus suffixe ou `ResyncRequired`.
- La suppression destructive de l’unique source exacte reste hors de cet ADR.

### 7. Clients volontairement lents ou hostiles

- L’admission utilise un sémaphore global ; les quotas par adresse attendent
  une identité et une confiance proxy définies.
- Handshake, lectures inactives, écritures et catch-up ont des échéances.
- Messages, octets, fréquence et concurrence sont bornés.
- Un client ne peut réserver mémoire, tâche ou clonage sans borne.
- La limitation répétée ne change jamais l’historique canonique ni ne masque
  le résultat d’une commande acceptée.

## Machine à états observable par le client

| État | Comportement serveur | Résultat client | Action requise |
|---|---|---|---|
| `connecting` | Valide limites et curseur | `Hello` accepté, plan ou rejet | Garder le curseur |
| `catching_up` | Pages bornées contiguës | Métadonnées et événements | Valider, appliquer, persister |
| `live` | Met en file les lots live | Événements ordonnés | Appliquer et persister |
| `resync_required` | Arrête le live, tente signal et fermeture | Signal, motif ou fermeture inexpliquée | Reconnecter au curseur durable |
| `full_resync` | Snapshot compatible puis suffixe | Snapshot et plan | Remplacer la base puis appliquer |
| `rejected` | N’admet aucun travail | Code stable | Retry borné, ne pas supposer le commit |

Après fermeture pendant une soumission, le résultat est inconnu. Le client
réessaie avec le même `message_id` ; la déduplication durable retrouve le
résultat accepté ou évalue une requête jamais admise.

## Impact sur les invariants après implémentation complète

### Garantis

- Une connexion lente ne bloque pas indéfiniment le commit autoritatif.
- Chaque connexion possède des bornes de file, octets et temps d’écriture.
- Dépassement et timeout ne sont jamais masqués comme livraison continue.
- Les pages sont contiguës et bornées.
- Le client reprend seulement depuis son curseur durable contigu.
- Une commande acceptée reste récupérable par `message_id`.
- Un rejet avant admission ne crée aucun fait canonique.

### Limités

- Le catch-up dépend du plancher retenu ou d’un snapshot compatible.
- Sous surcharge, l’hôte peut refuser explicitement.
- Sans accusé client, la livraison de bout en bout n’est pas garantie.
- Le stockage durable total reste non borné avant politique d’archive.

### Impossibles sous exigences contradictoires

- Catch-up historique arbitraire, rétention locale finie et absence d’archive
  externe ne peuvent coexister.
- Un motif final ne peut être garanti sur un transport déjà bloqué ou rompu.
- La mémoire totale ne peut être strictement bornée tant que journal, index et
  état canonique restent intégralement résidents.

## Plan d’implémentation

1. **Vocabulaire protocolaire seulement.** Raisons stables,
   `ResyncRequired`, pages et tests.
2. **Isolation du writer par connexion.** File bornée en nombre et octets,
   timeout, chemin de fermeture réservé et tests de dépassement.
3. **Admission des connexions et du travail autoritatif.** Sémaphores globaux
   et codes de rejet, sans changer commit ni déduplication.
4. **Catch-up paginé sur l’archive existante.** High-water marks, limites de
   page et budgets totaux, sans cloner tout le suffixe.
5. **Resync par snapshot.** Rendre `StateSnapshot` opérationnel, valider
   snapshot-plus-suffixe et négocier `earliest_available_sequence`.
6. **Rétention de fenêtre chaude.** Segmenter le stockage et servir sans charger
   l’archive complète, tout en conservant le replay archivé exact.
7. **Politique de rétention.** Décider export, durée, quota et suppression avant
   tout compactage destructif.
8. **Durcissement opérationnel.** Métriques et logs sur files, retards,
   fermetures, pages, octets, rejets, timeouts et planchers.

Chaque tranche garde les anciennes versions lisibles ou augmente explicitement
la version. Aucun curseur ancien n’est réinterprété silencieusement.

## Tests exigés

### Tests de propriété et d’invariants

- Ordre accepté et journal indépendants d’une file bloquée.
- Occupation et octets n’excèdent jamais les bornes.
- Une seule transition `resync_required` par connexion surchargée.
- Aucun événement après une plage perdue n’est présenté comme continu.
- Pages non vides, contiguës, ordonnées et bornées.
- La concaténation des pages égale exactement le suffixe canonique.
- Taille et timing des pages ne changent pas l’état final.
- Le passage `catching_up -> live` ne perd ni ne duplique.
- Tout curseur converge ou exige explicitement snapshot/resync complet.
- Retry du même `message_id` après fermeture sans second append.
- Rejet d’admission sans événement, reçu, révision ou broadcast.
- Snapshot plus suffixe égal au replay de l’archive complète.

### Scénarios de charge et de panne

- Un client WebSocket qui ne lit pas pendant que les autres progressent.
- De nombreux clients lents jusqu’au-delà du quota.
- Événements et lots surdimensionnés.
- Writer bloqué au-delà de son délai.
- Retard broadcast pendant un catch-up concurrent.
- Catch-up au plancher, juste avant et depuis zéro.
- Déconnexion à chaque frontière d’enqueue, écriture, application et persistance.
- Redémarrage de l’hôte à chaque frontière de page.
- Disque plein et échec d’append sous surcharge.
- Tempêtes de reconnexion avec backoff et concurrence bornés.
- Sessions longues démontrant mémoire chaude et latence stables.

## Décisions humaines requises avant implémentation

1. Valeurs et configuration des limites de connexions, commandes, files, octets,
   délais, pages, budgets et catch-ups concurrents.
2. Compatibilité V1 ou nouvelle version du premier changement protocolaire.
3. Messages, code WebSocket et retry stables pour lenteur et surcharge.
4. Accusé durable serveur ou seul curseur de reconnexion appartenant au client.
5. Définition d’un snapshot compatible et traitement de l’historique local.
6. Distinction fenêtre chaude/archive, export, durée, quota et autorité de
   suppression.
7. Portée des limites : hôte, session, identité ou adresse réseau.

## Conséquences

Le chemin autoritatif reste stable lorsqu’un client ralentit, tandis que l’hôte
peut refuser explicitement du nouveau travail global. La règle client est
simple : ne persister que des faits contigus appliqués, ne jamais inférer la
livraison de la durée de connexion et reconnecter depuis ce curseur.

Le design ajoute états protocolaires, comptage de file, délais et limites. Il
accepte déconnexion et retry comme flux normal. Le replay exact reste
l’autorité ; le live est une optimisation bornée.

Le compactage n’est pas une correction immédiate de backpressure. Il ne devient
sûr qu’après validation du replay snapshot-plus-suffixe et d’une politique
d’archive séparée.
