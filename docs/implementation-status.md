[Français](implementation-status.md) | [English](implementation-status.en.md)

# État de l’implémentation

## Implémenté et exécutable

- session, participants, cycle de vie, autorité, enveloppes d’activité et
  révision typés ;
- commandes de création, jonction, activité et transfert avec refus structurés ;
- `demo.counter` et `demo.vote` isolés derrière `ActivityEngine` ;
- événements immuables et séquencés, réduction pure, application atomique et
  replay déterministe ;
- snapshots, journaux et enveloppes de protocole versionnés ;
- contrats LLM et CPU, inventaire, politiques, disponibilité, observations par
  offre et score de matching explicable ;
- hôte WebSocket à autorité unique et clients multi-processus ;
- diffusion des événements acceptés et rattrapage depuis une séquence connue ;
- journal canonique JSON Lines synchronisé avant commit mémoire et diffusion ;
- reconstruction de l’hôte depuis le journal après redémarrage ;
- façade CLI pour les démonstrations, la validation, le replay, le matching,
  `serve` et `connect` ;
- façade Wasm étroite pour le matching ;
- CI pour le formatage, Clippy, les tests et la compilation Wasm.

## Conçu mais non implémenté

- déduplication durable des commandes et reprise automatique d’une dernière
  ligne JSONL partiellement écrite ;
- snapshots réseau et compaction du journal ;
- TLS applicatif, identité cryptographique, signatures et projections privées ;
- découverte LAN, WebRTC, NAT traversal, consensus et multi-autorité ;
- réservation, exécution et annulation distantes des jobs ;
- persistance et provenance de l’Observatory ;
- Noema, Delibra, FFF, jeux, Media Sync et implémentations TypeScript ;
- paquet Wasm/TypeScript généré.

Le matcher V0 fait confiance aux déclarations, politiques, disponibilités et
observations fournies. Il démontre le modèle mais ne constitue pas une
autorisation sûre pour l’exécution réelle de ressources.
