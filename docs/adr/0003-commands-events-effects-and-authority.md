Langue canonique : Français

English version: [English](0003-commands-events-effects-and-authority.en.md)

[Français](0003-commands-events-effects-and-authority.md) | [English](0003-commands-events-effects-and-authority.en.md)

# ADR 0003 : Commandes, événements, effets et autorité initiale

Statut : accepté

Les commandes sont des intentions faillibles. Les faits acceptés sont des
événements immuables et ordonnés. Seuls les reducers modifient l’état. Le
travail externe est décrit sous forme d’effets. Le créateur de la session est
l’autorité logique initiale et peut transférer cette autorité manuellement.

La V0 exclut délibérément l’élection de leader et le consensus. L’autorité
logique n’est ni un pair réseau, ni un serveur, ni un écran, ni le propriétaire
d’une machine.
