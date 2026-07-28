Langue canonique : Français

English version: [English](0007-v01-consolidation-boundaries.en.md)

[Français](0007-v01-consolidation-boundaries.md) | [English](0007-v01-consolidation-boundaries.en.md)

# ADR 0007 : Frontières des activités et capacités en V0.1

Statut : accepté

La première tranche verticale a révélé deux spécialisations accidentelles. La
V0.1 :

- conserve des enums fermées pour les règles génériques de session ;
- route les commandes et événements d’activité par une enveloppe opaque
  versionnée ;
- implémente `demo.counter` hors du cœur au moyen d’un `ActivityEngine`
  minimal ;
- identifie et évalue des offres concrètes de capacité plutôt que les seuls
  nœuds ;
- utilise des familles de contrats typées pour le LLM et le CPU ;
- rattache observations et disponibilité aux identifiants d’offre ;
- publie une décomposition V1 du score ;
- versionne journaux et snapshots indépendamment de l’état du domaine.

Il ne s’agit pas d’un système dynamique de plugins. Les nouveaux variants
d’enum de contrat restent des modifications volontaires à la compilation tant
que plusieurs familles réelles ne justifient pas un registre de schémas.
