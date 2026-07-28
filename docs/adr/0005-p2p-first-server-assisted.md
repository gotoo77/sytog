Langue canonique : Français

English version: [English](0005-p2p-first-server-assisted.en.md)

[Français](0005-p2p-first-server-assisted.md) | [English](0005-p2p-first-server-assisted.en.md)

# ADR 0005 : P2P d’abord, assisté par serveur

Statut : accepté

L’architecture doit permettre les pairs directs et l’usage LAN ou auto-hébergé,
tout en autorisant la signalisation, le relais et un repli WebSocket. Aucun
transport n’est privilégié dans le domaine, et la V0 n’en implémente aucun car
une tranche déterministe en mémoire suffit à tester la sémantique.
