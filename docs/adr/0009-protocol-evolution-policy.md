Langue canonique : Français

English version: [English](0009-protocol-evolution-policy.en.md)

[Français](0009-protocol-evolution-policy.md) | [English](0009-protocol-evolution-policy.en.md)

# ADR 0009 : Politique d’évolution du protocole

Statut : proposé

Cette décision définit la négociation, l’activation, la compatibilité et le
cycle de vie des versions du protocole réseau SYTOG. Elle ne modifie pas le
serveur : V1 reste la seule version active et V2 reste définie mais inactive.

## Contexte

[L’ADR 0004](0004-versioned-protocol-and-polyglot-activities.md) impose une
famille et une version explicites à chaque enveloppe. [L’ADR
0008](0008-overload-and-backpressure-contract.md) a introduit le vocabulaire V2
sans l’activer. Le code distingue désormais :

- `LATEST_PROTOCOL_VERSION = 2`, la version la plus récente définie ;
- `ACTIVE_SERVER_PROTOCOL_VERSION = 1`, la version actuellement émise par le
  serveur et le client existants.

Cette séparation empêche aujourd’hui une activation implicite, mais ne suffit
pas lorsque plusieurs versions devront coexister. Il manque encore un contrat
pour annoncer les capacités, choisir une version commune, faire évoluer la
préférence du serveur et retirer une ancienne version.

Le premier message applicatif ne peut pas servir de format neutre de
négociation : `Hello` est déjà versionné et sa forme diffère entre V1 et V2.
Décoder un `Hello` avant d’avoir choisi son protocole recréerait précisément
l’ambiguïté que le versionnement cherche à supprimer.

Les versions du protocole réseau sont indépendantes des versions de schéma du
journal, des snapshots et des activités. Une évolution de l’un de ces formats
ne change pas automatiquement la version du protocole réseau.

## Objectifs

- sélectionner exactement une version avant tout message applicatif ;
- permettre à V1 et V2 de coexister sur le même endpoint WebSocket ;
- préserver les clients V1 pendant une migration explicitement bornée ;
- rendre les incompatibilités et les transitions observables ;
- empêcher `LATEST_PROTOCOL_VERSION` de participer à l’activation ;
- limiter le nombre de chemins réseau actifs et testés simultanément ;
- rendre toute dépréciation et tout retrait intentionnels et réversibles avant
  suppression du code de compatibilité.

## Stratégies étudiées

### A. Négociation par sous-protocole WebSocket

Le client offre des jetons `Sec-WebSocket-Protocol`, par exemple `sytog.v2` et
`sytog.v1`. Le serveur choisit un jeton dans l’intersection avec ses versions
activées et le retourne dans la réponse HTTP `101`.

- la version est fixée avant `Hello` ;
- le mécanisme est standard et disponible dans les clients WebSocket natifs et
  navigateurs ;
- un seul endpoint peut héberger plusieurs versions ;
- aucun format applicatif « V0 » supplémentaire n’est nécessaire ;
- l’échec se produit avant l’admission d’une session applicative ;
- les navigateurs exposent parfois seulement un échec de handshake, et non le
  détail du corps HTTP ;
- les clients V1 actuels, qui n’offrent aucun sous-protocole, exigent une règle
  de transition explicite.

### B. Message de négociation neutre avant `Hello`

La connexion WebSocket est d’abord acceptée, puis un nouveau message non
versionné échange les listes de versions.

- le serveur peut retourner une erreur structurée dans le canal WebSocket ;
- il faut créer, versionner et sécuriser un mini-protocole d’amorçage ;
- il faut ajouter un état, un parseur, une limite de taille et un timeout avant
  le protocole normal ;
- les anciens clients envoient immédiatement un `Hello` V1 et nécessitent
  malgré tout une détection spéciale.

### C. Liste de versions dans `Hello`

- le transport ne change pas ;
- le serveur doit savoir quelle forme de `Hello` décoder avant de connaître la
  version ;
- V1 et V2 ne partagent déjà plus le même champ de curseur ;
- une erreur ou une extension de `Hello` pourrait être réinterprétée
  silencieusement.

Cette stratégie est circulaire et ne fournit pas de frontière de version nette.

### D. Un endpoint par version

Des chemins tels que `/v1` et `/v2` sélectionnent la version avant le
handshake.

- la sélection est simple et explicite ;
- aucun protocole d’amorçage n’est nécessaire ;
- chaque nouvelle version ajoute du routage, de la configuration et de la
  documentation opérationnelle ;
- la découverte et la migration imposent des URL différentes ;
- les endpoints peuvent diverger en quotas, sécurité ou déploiement.

Cette stratégie reste une solution de secours si une future infrastructure ne
peut pas préserver les en-têtes de sous-protocole.

### E. Essai de la version la plus haute puis reconnexion avec repli

- le serveur n’a pas besoin d’un mécanisme de sélection ;
- chaque incompatibilité coûte au moins une connexion supplémentaire ;
- le repli dépend de l’ordre des erreurs et peut masquer une mauvaise
  configuration ;
- un intermédiaire peut provoquer un downgrade ;
- le comportement devient moins explicable sous surcharge.

Le repli automatique implicite est incompatible avec l’objectif de
déterminisme de SYTOG.

## Matrice de décision

| Stratégie | Avant `Hello` | Même endpoint | Compatibilité V1 | Déterminisme | Complexité | Décision |
|---|---:|---:|---:|---:|---:|---|
| Sous-protocole WebSocket | Oui | Oui | Transition explicite | Fort | Faible à moyenne | Retenir |
| Message neutre | Oui, après upgrade | Oui | Détection spéciale | Fort | Moyenne à forte | Rejeter |
| Liste dans `Hello` | Non | Oui | Ambiguë | Faible | Faible en apparence | Rejeter |
| Endpoint par version | Oui | Non | Simple | Fort | Moyenne opérationnelle | Secours |
| Reconnexion avec repli | Après échec | Oui | Possible | Faible | Moyenne côté client | Rejeter |

## Décision

### Négociation

La négociation aura lieu pendant le handshake HTTP WebSocket, avant toute
enveloppe SYTOG et avant `Hello`.

Le client offre l’ensemble de ses versions réseau supportées à l’aide des
jetons de sous-protocole :

- `sytog.v1` pour V1 ;
- `sytog.v2` pour V2 ;
- les futures versions utilisent `sytog.vN`.

Un jeton de version SYTOG est canonique s’il correspond exactement à
`sytog.vN`, où `N` est un entier décimal strictement positif sans zéro initial.
Le traitement de l’offre sépare trois cas :

1. un en-tête WebSocket invalide ou un jeton qui revendique le préfixe
   `sytog.v` sans respecter cette grammaire rend toute l’offre mal formée ; le
   handshake est rejeté avec le code stable `invalid_protocol_offer` et aucune
   sélection partielle n’a lieu ;
2. un jeton syntaxiquement valide mais inconnu, non supporté ou non activé ne
   participe pas à l’intersection ; il ne rend pas invalide une autre version
   commune ;
3. après normalisation, une intersection vide produit
   `no_common_protocol`.

Les doublons syntaxiquement valides sont normalisés en un ensemble. Ils ne sont
pas rejetés, ne modifient pas la préférence et ne donnent aucun poids
supplémentaire à une version.

Ainsi, l’offre `sytog.v3, sytog.v2` sélectionne V2 si V2 est supportée et
activée, même si V3 est inconnue, non supportée ou inactive. Si aucune des deux
versions n’est commune, le résultat est `no_common_protocol`, pas
`invalid_protocol_offer`.

Un en-tête absent n’est pas un en-tête mal formé : il suit exclusivement la
règle legacy V1 décrite ci-dessous. Un en-tête présent mais vide ou
syntaxiquement invalide produit `invalid_protocol_offer`.

L’ordre fourni par le client n’est pas une préférence. Un client qui refuse une
version ne l’offre pas. Le serveur calcule l’intersection entre :

1. les versions offertes par le client ;
2. les versions supportées par le binaire ;
3. les versions activées dans la configuration de l’hôte.

Le serveur parcourt ensuite son ordre de préférence configuré et choisit la
première version commune. Cette fonction est pure, déterministe et indépendante
de l’ordre des jetons du client. Le serveur ne sélectionne jamais une version
absente de l’offre, non supportée par le binaire ou non activée.

Pour une offre explicite, la réponse `101 Switching Protocols` retourne
exactement le jeton sélectionné. Cette version reste immuable pendant toute la
connexion. Chaque enveloppe ultérieure doit porter cette même version et est
décodée uniquement par son décodeur dédié. Une divergence ferme la connexion
comme erreur de protocole ; elle ne déclenche aucun repli.

Ignorer une capacité offerte mais non sélectionnée ne contredit pas l’ADR
0004 : aucune enveloppe de cette version n’est acceptée. Une version inconnue
sans version commune échoue explicitement au handshake, et toute enveloppe dont
la version diffère de la version sélectionnée échoue ensuite explicitement.

### Compatibilité transitoire de V1

Pendant la migration initiale uniquement, l’absence de
`Sec-WebSocket-Protocol` peut être configurée comme l’offre implicite du seul
jeton `sytog.v1`. Ce mode :

- n’est valide que tant que V1 est activée ;
- ne permet jamais de sélectionner V2 ;
- retourne une réponse `101` sans en-tête de sous-protocole, conformément au
  fait que le client legacy n’en a offert aucun, tout en fixant V1 en interne ;
- est exposé dans les métriques et les journaux comme `legacy_v1`;
- possède un interrupteur de configuration distinct ;
- est supprimé lorsque V1 est retirée.

Ainsi, un ancien client ne peut pas activer V2 par accident et une future
absence d’offre ne signifie jamais « choisir la dernière version ».

### Aucune version commune

Si l’intersection est vide, le serveur rejette le handshake avant de créer une
session applicative. Il ne produit ni enveloppe SYTOG, ni close frame dépendante
d’une version, ni effet autoritatif.

La réponse est `400 Bad Request`, avec
`Content-Type: application/problem+json`, l’en-tête
`SYTOG-Supported-Protocols` contenant les jetons activés dans l’ordre de
préférence du serveur, et un corps dont le code machine stable est
`no_common_protocol`. Une offre mal formée utilise la même frontière HTTP avec
le code `invalid_protocol_offer`. La réponse ne reflète pas les valeurs mal
formées fournies par le client.

Le statut HTTP, l’en-tête et le corps `application/problem+json` sont des
enrichissements diagnostiques. La seule dépendance fonctionnelle du client est
le résultat du handshake : une réponse `101` avec le jeton qu’il a offert, ou
un échec. Un client, une politique de retry ou une décision de sûreté ne doit
pas dépendre de la possibilité de lire ou parser le corps, car une API
navigateur peut seulement exposer l’échec de connexion. Les détails complètent
les journaux et la télémétrie, mais ne déclenchent aucun repli automatique. Le
schéma JSON complet du problème est figé dans la tranche de vocabulaire de
négociation avant toute implémentation réseau.

Aucun client ne tente automatiquement une version qu’il n’avait pas offerte.
Une nouvelle tentative avec une offre différente relève d’une politique
explicite du client.

## États formels d’une version

| État | Définition |
|---|---|
| Définie | Schéma, sémantique, documentation, fixtures et validations existent. `LATEST_PROTOCOL_VERSION` désigne seulement la plus grande version définie. |
| Supportée par le binaire | Le binaire contient les encodeurs, décodeurs, handlers et tests de conformité nécessaires à son fonctionnement de bout en bout. Elle peut rester désactivée. |
| Activée par le serveur | La configuration autorise cette version pour de nouvelles connexions. L’ensemble activé est un sous-ensemble de l’ensemble supporté. |
| Préférée | Première version de l’ordre de sélection du serveur parmi les versions communes activées. Elle doit être activée et supportée. |
| Dépréciée | Toujours supportée et éventuellement activée, mais son retrait est annoncé et observé. Elle n’est jamais retirée dans la même livraison que sa première dépréciation normale. |
| Retirée | Refusée pour toute nouvelle connexion et absente de l’ensemble activé. Son décodeur peut rester dans le binaire pour les fixtures, diagnostics ou migrations historiques. |

La suppression du code d’une version retirée est une étape distincte. Elle
exige la preuve qu’aucun format persistant ou outil de migration n’en dépend.
La présence de **code historique conservé** est une propriété orthogonale, pas
un état réseau supplémentaire : un décodeur seul ne rend pas une version
supportée, activée ou sélectionnable. Ce code ne figure pas dans l’ensemble
supporté de bout en bout et ne compte pas dans la fenêtre des versions réseau
actives.

À l’adoption de cet ADR :

- V1 est définie, supportée, activée et préférée ; elle n’est pas dépréciée ;
- V2 est définie et ses types de frontière sont disponibles dans la
  bibliothèque de transport, mais elle n’est pas encore supportée de bout en
  bout par le serveur, activée ou préférée ;
- aucune version n’est dépréciée ou retirée.

`ACTIVE_SERVER_PROTOCOL_VERSION` reste, dans l’implémentation actuelle, le
scalaire V1 utilisé par le chemin historique. L’implémentation future de la
négociation devra introduire un ensemble activé et un ordre de préférence
explicites ; elle ne devra pas redéfinir ce scalaire comme alias de
`LATEST_PROTOCOL_VERSION`.

## Fenêtre de compatibilité

Un serveur active au plus deux versions majeures réseau consécutives. Cette
borne s’applique uniquement aux versions acceptées pour de nouvelles connexions
sur un hôte ; elle ne compte ni les versions seulement définies, ni les
décodeurs conservés dans les outils hors ligne. Lors de l’activation de V`N`,
V`N-1` reste activée pendant au moins une livraison normale complète. Une
livraison ultérieure peut annoncer sa dépréciation ; son retrait n’intervient
que dans une livraison encore ultérieure.

V1 n’est pas dépréciée automatiquement par l’existence ou l’activation future
de V2. Déprécier puis retirer V1 exige des décisions humaines distinctes,
étayées par la télémétrie d’usage et des notes de migration.

Une correction urgente de sécurité ou d’intégrité peut raccourcir cette
fenêtre. L’exception exige une décision documentée, une erreur explicite pour
les clients affectés et une procédure de retour arrière lorsque celle-ci reste
sûre. Elle peut réduire immédiatement l’ensemble activé, mais ne peut ni
dépasser la borne de deux versions, ni rendre sélectionnable une version non
supportée. Les décodeurs historiques et outils hors ligne restent hors de cette
borne.

Le serveur ne promet pas de supporter toutes les versions définies. Les outils
hors ligne peuvent conserver davantage de décodeurs lorsque le replay,
l’archive ou la migration l’exigent.

## Compatibilité et nouvelle version majeure

Une nouvelle version majeure est requise lorsqu’un pair conforme à la version
existante pourrait mal décoder, mal interpréter ou appliquer silencieusement le
nouveau comportement, notamment pour :

- retirer ou renommer un message, un champ, un tag, une raison ou un code
  stable ;
- ajouter un champ obligatoire ou changer son type ;
- changer le sens d’un champ, d’un curseur, d’un accusé, d’un code ou d’un état
  terminal ;
- changer les garanties d’ordre, de livraison, d’idempotence, de replay,
  d’autorité ou de reprise ;
- envoyer un nouveau variant de message ou d’énumération qu’un pair existant ne
  sait pas ignorer de façon sûre ;
- modifier la négociation, l’authentification ou la frontière d’admission d’une
  manière incompatible ;
- accepter comme valide une même représentation avec une sémantique différente.

Restent compatibles dans une même version :

- les clarifications qui ne changent aucune conduite observable ;
- les corrections qui rejettent seulement des données déjà invalides selon le
  contrat publié ;
- l’ajout d’un champ réellement optionnel, doté d’un défaut sûr, lorsque les
  anciens lecteurs l’ignorent et les nouveaux lecteurs acceptent son absence ;
- les optimisations internes qui préservent exactement le contrat observable ;
- les changements de valeurs configurées de quotas ou délais lorsque leurs
  unités, bornes contractuelles et résultats protocolaires restent inchangés.

En l’absence d’un mécanisme de capacités plus fin, un nouveau message, une
nouvelle raison ou un nouveau variant susceptible d’être émis est considéré
comme incompatible, même si sa représentation JSON est additive.

## Cycle de vie

1. **Définir.** Documenter le contrat et les incompatibilités, ajouter les types,
   fixtures et tests spécifiques, puis avancer `LATEST_PROTOCOL_VERSION`.
   L’ensemble activé ne change pas.
2. **Supporter.** Intégrer les encodeurs, décodeurs et handlers dans le binaire
   sans les rendre sélectionnables. Les tests croisés prouvent l’isolation des
   versions.
3. **Activer sans préférer.** Ajouter explicitement la version à la
   configuration de certains hôtes. L’ancienne version reste préférée ; seuls
   les clients qui n’offrent que la nouvelle la sélectionnent.
4. **Préférer.** Après validation de compatibilité, charge, reprise et
   observabilité, modifier explicitement l’ordre de préférence. Cette opération
   possède un retour arrière de configuration.
5. **Déprécier.** Publier les notes de migration, instrumenter l’usage restant
   et annoncer une échéance ou un critère de retrait.
6. **Retirer.** Supprimer la version de l’ensemble activé et désactiver son mode
   legacy. Conserver le décodage hors ligne tant que nécessaire.
7. **Supprimer le code.** Effectuer une décision et une migration séparées si le
   décodeur historique n’est plus nécessaire.

Chaque changement d’état est explicite dans la configuration ou le code revu.
Une livraison peut définir une version sans la supporter opérationnellement, et
un binaire peut la supporter sans qu’aucun serveur ne l’active.

## Tests exigés

Avant toute activation :

- tests de table et de propriété sur l’intersection et l’ordre de préférence ;
- invariants `activées ⊆ supportées` et `préférée ∈ activées` ;
- rejet séparé des offres présentes mais vides ou mal formées avec
  `invalid_protocol_offer` ;
- normalisation des doublons et preuve qu’ils ne changent pas la sélection ;
- sélection d’une version commune malgré des jetons valides inconnus, non
  supportés ou inactifs dans la même offre ;
- échec `no_common_protocol` lorsqu’aucune version commune ne subsiste, sans
  création de session ni effet autoritatif ;
- preuve que la sélection est indépendante de l’ordre offert par le client ;
- preuve que le serveur ne sélectionne jamais une version non offerte ;
- règle legacy absente/activée/désactivée testée explicitement ;
- version sélectionnée immuable pendant la connexion ;
- rejet croisé V1 par le décodeur V2 et V2 par le décodeur V1 ;
- fixtures et round-trips propres à chaque version ;
- tests de rolling upgrade avec clients et serveurs N et N-1 ;
- tests de retour de préférence de N vers N-1 ;
- test architectural prouvant qu’une modification isolée de
  `LATEST_PROTOCOL_VERSION` ne change ni l’ensemble activé ni la sélection.

Chaque version activée exécute les mêmes scénarios de conformité, de
reconnexion, de replay et d’erreur. Une transition vers V2 ajoute en plus les
scénarios de surcharge et de resynchronisation de l’ADR 0008, sans retirer les
scénarios V1 tant que V1 reste activée.

## Observabilité exigée

Au démarrage, le serveur journalise les versions supportées, activées,
préférées, dépréciées et le statut du mode legacy V1.

Les métriques de handshake comptent :

- offres explicites, offres legacy, offres mal formées et offres contenant des
  jetons valides inconnus ;
- version sélectionnée ;
- refus par raison, dont `invalid_protocol_offer` et `no_common_protocol` ;
- connexions utilisant une version dépréciée.

Les journaux structurés enregistrent la version choisie et la cause d’un refus,
sans inclure de payload applicatif ni créer de labels à forte cardinalité.
Alertes et tableaux de bord doivent permettre de mesurer l’usage restant avant
un retrait et de comparer erreurs, reconnexions et latence par version.

## Garde-fous contre l’activation implicite

- `LATEST_PROTOCOL_VERSION` n’est jamais une valeur par défaut de connexion,
  de configuration ou de sélection.
- Ni le client ni le serveur ne génèrent une offre, un ensemble supporté, un
  ensemble activé ou un ordre de préférence comme l’intervalle
  `1..=LATEST_PROTOCOL_VERSION` ou à partir de son maximum.
- La sélection reçoit explicitement l’ensemble activé et l’ordre de préférence.
- Les ensembles supporté et activé utilisent des concepts distincts, validés au
  démarrage.
- Le nœud serveur ne dépend pas de `LATEST_PROTOCOL_VERSION`.
- Ajouter une constante `PROTOCOL_VERSION_VN` ou avancer `LATEST` ne modifie
  aucun chemin réseau sans un changement séparé de configuration d’activation.
- La CI contient un test où `LATEST` est supérieur à la version préférée et
  vérifie que la sélection reste inchangée.
- Toute modification de l’ensemble activé ou de l’ordre préféré est visible
  séparément dans le diff et les notes de livraison.

## Conséquences

La négociation est explicite avant le protocole applicatif et une seule
connexion ne mélange jamais V1 et V2. Le même endpoint peut accompagner une
migration sans multiplier les routes. Les clients existants restent
compatibles grâce à un mode legacy V1 borné et observable.

Le serveur devra personnaliser le handshake WebSocket et maintenir une matrice
de tests N/N-1. Les erreurs détaillées de handshake ne seront pas toujours
visibles depuis un navigateur ; la télémétrie serveur et une erreur client
générique restent nécessaires.

La politique privilégie une migration contrôlée plutôt que l’activation
automatique de la version la plus récente. Elle accepte le coût temporaire de
deux chemins réseau actifs, mais refuse d’en maintenir plus de deux.

## Plan d’implémentation proposé

Ces tranches sont ordonnées, mais aucune n’est commencée par cet ADR :

1. **Vocabulaire de négociation.** Figer les jetons, le refus HTTP, les codes
   `invalid_protocol_offer` et `no_common_protocol`, ainsi que les types de
   configuration, sans modifier le handshake.
2. **Sélecteur pur.** Implémenter et tester l’intersection, les invariants de
   configuration et l’ordre de préférence, toujours sans activation réseau.
3. **Handshake serveur en V1 seulement.** Sélectionner `sytog.v1` lorsqu’elle
   est présente dans une offre valide, ignorer les autres jetons valides pour
   l’intersection, conserver le mode legacy V1 configurable, rejeter les offres
   mal formées ou sans version commune et ajouter la télémétrie.
4. **Annonce client V1.** Faire offrir explicitement `sytog.v1` par les clients
   maintenus, sans changer leur payload ni leur comportement.
5. **Support dormant de V2.** Intégrer les handlers V2 et autoriser le handshake
   à reconnaître son jeton, tout en gardant l’ensemble activé à `{V1}` et V1
   préférée.
6. **Activation contrôlée de V2.** Activer V2 sur des hôtes de test, conserver
   V1, puis exécuter les tests de l’ADR 0008 requis par le comportement rendu
   observable.
7. **Préférence V2.** Modifier séparément la préférence après validation
   humaine et conserver un retour arrière vers V1.
8. **Dépréciation puis retrait de V1.** Deux décisions ultérieures distinctes,
   guidées par la télémétrie et les migrations clients.

La tranche 2 de l’ADR 0008 ne doit pas être confondue avec ces étapes. Son
comportement V2 ne devient observable qu’après une négociation et une activation
explicitement validées.

## Questions différées

- schéma JSON complet du refus de handshake au-delà du code stable ;
- source et format de la configuration des versions activées et préférées ;
- durée calendaire minimale d’une dépréciation en plus de la règle de livraison ;
- mécanisme éventuel de capacités fines à l’intérieur d’une version ;
- politique de compatibilité des outils hors ligne et durée de conservation des
  décodeurs retirés ;
- procédure d’exception exacte pour un retrait de sécurité urgent.
