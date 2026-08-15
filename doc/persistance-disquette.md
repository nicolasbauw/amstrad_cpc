# Les écritures sur disquette ne survivaient pas à un power cycle

## Le symptôme

`SAVE` en BASIC (ou tout logiciel qui écrit sur la disquette insérée)
semblait fonctionner : le fichier apparaissait bien via `CAT`, relisible
dans la même session. Mais après un power cycle (`pc` en console, ou
redémarrage de l'émulateur), la sauvegarde avait disparu — comme si elle
n'avait jamais eu lieu.

## La cause

Le contrôleur de disquette (`Fdc`, dans `fdc.rs`) tient une image `.dsk`
complète **en mémoire** (`Drive::dsk`), construite une fois au chargement
du fichier (`load_disk_into`) et relue par le CPU à chaque instruction
`IN`/`OUT` touchant le FDC. Deux commandes la modifient :

- **Write Data** (0x05) — écriture d'un secteur, close par
  `finish_write_command`.
- **Format Track** (0x0D) — formatage d'une piste, close par
  `finish_format_command`.

Les deux mutaient bien `Drive::dsk`, la copie en mémoire — mais jamais le
fichier réellement sur disque. Rien ne clochait tant que l'émulateur restait
allumé : toute lecture ultérieure de la disquette passait par la même copie
en mémoire, à jour. Le décalage n'apparaissait qu'au moment où quelque chose
force à **relire le fichier depuis le disque** : `power_on()`/
`power_cycle()` (`machine.rs`) reconstruit `Fdc` de zéro et recharge chaque
lecteur via son `current_filename`, ou tout simplement fermer puis rouvrir
l'émulateur. À ce moment-là, la copie en mémoire (à jour) est perdue et
remplacée par le fichier sur disque (périmé).

## Le correctif

`Fdc::persist_drive_dsk()` (`fdc.rs`) réécrit intégralement le fichier
`.dsk` du lecteur concerné, via `Fdc::write_dsk_file` — la même fonction déjà
utilisée pour créer une disquette vierge avec la commande console `blank`.
Appelée à la fin de `finish_write_command` (si l'écriture a réellement
trouvé le secteur visé) et de `finish_format_command`.

Comme l'appel se fait secteur par secteur (`finish_write_command` ne
traite jamais plus d'un secteur à la fois, tout comme le matériel réel dans
l'usage courant — voir le commentaire existant sur `write_data`), un `SAVE`
qui écrit plusieurs secteurs d'affilée les rend tous durables au fur et à
mesure, pas seulement une fois la commande BASIC terminée. Un power cycle
survenant *pendant* une sauvegarde perdrait donc seulement la fin du
fichier, pas la totalité — comportement cohérent avec un vrai lecteur de
disquette, qui écrit physiquement piste par piste.

Deux garde-fous :
- `current_filename == "None"` (aucun fichier réel derrière l'image — cas
  théorique d'un `Format Track` envoyé à un lecteur sans disquette insérée)
  : rien n'est écrit, pas de fichier fantôme créé.
- Une erreur d'écriture (permissions, disque plein, périphérique retiré...)
  est signalée en console plutôt que de faire paniquer l'émulateur : une
  disquette USB qu'on retire par erreur ne doit pas planter une partie en
  cours.

## Vérifié

`fdc::tests::writing_a_sector_persists_to_the_dsk_file_on_disk` : écrit un
secteur sur un fichier `.dsk` temporaire réel, puis **relit ce fichier
depuis le disque** (pas la structure en mémoire) pour confirmer que
l'écriture y est bien présente. Confirmé en pratique que ce test échoue
sans le correctif et passe avec.

## Optimisation non faite, à reprendre si besoin

Chaque écriture de secteur réécrit **tout** le fichier `.dsk` (quelques
centaines de Ko pour une disquette standard), pas seulement le secteur
modifié. Pour une sauvegarde BASIC ponctuelle, le coût est négligeable
(quelques écritures, fichier petit). Mais un logiciel qui écrit très
fréquemment — un formatage complet piste par piste, un jeu qui journalise
sa progression sur disque en continu — réécrirait le fichier entier à
chaque secteur, ce qui pourrait devenir sensible.

Inscrit au chantier de finition V3 (voir TODO.txt, point 1), à traiter après
la V2.

Piste si ça devient nécessaire un jour : écrire seulement l'octet modifié à
son offset exact dans le fichier (`std::fs::File` ouvert en lecture-écriture,
`seek` + `write` ciblés), plutôt que de reconstruire et réécrire l'image
complète à chaque fois. Ça suppose de calculer l'offset exact du secteur
dans le fichier `.dsk` (dépend du format Standard vs Extended DSK, de la
taille de piste, et de la position du secteur dans sa piste) — plus
complexe que l'écriture complète actuelle, d'où le choix de ne pas le faire
tant qu'aucun cas d'usage réel ne le justifie.
