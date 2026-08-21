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

## Écriture ciblée d'un secteur (Plan V3.md, point 1)

Chaque écriture de secteur réécrivait **tout** le fichier `.dsk` (160 Ko
pour `Discology.dsk`, quelques centaines pour une disquette standard), pas
seulement le secteur modifié. Négligeable pour une sauvegarde BASIC
ponctuelle, mais un formatage piste par piste ou un jeu qui journalise sa
progression réécrivait l'image entière à chaque secteur.

`Fdc::persist_sector` écrit désormais les 512 octets du secteur à son offset
exact (`seek` + `write` ciblés), plus l'octet ST2 de son descripteur — soit
**~320 fois moins d'octets écrits** par secteur sur une image de 160 Ko.
`persist_drive_dsk` (réécriture complète) reste utilisée par Format Track,
qui change la structure même des pistes.

### Deux précautions

**L'offset se calcule depuis le fichier, jamais depuis l'image en mémoire.**
Les deux formats rangent les pistes différemment : taille uniforme annoncée
en 0x32 pour le Standard, table de tailles à partir de 0x34 pour l'Extended,
où une piste non formatée n'occupe carrément aucun octet du fichier. Et une
image chargée en Extended le reste tant que personne ne l'a réécrite en
entier — `write_dsk_file`, lui, produit toujours du Standard.

**Repli systématique sur la réécriture complète** dès que la géométrie ne
correspond pas exactement à ce qu'on croit écrire : secteur absent de la
piste, taille différente (elle déplacerait tout ce qui suit), piste absente
d'une image Extended, en-tête inattendu. Mieux vaut réécrire trop que
corrompre une image ; c'est ce repli qui rend le correctif sans risque.

Les autres bits de ST2 sont préservés à l'écriture : ils peuvent porter des
indicateurs d'erreur venus d'un dump réel, qu'on n'a aucune raison
d'effacer.

### Vérification

Un test dédié (`a_targeted_sector_write_lands_at_the_right_offset`) écrit un
secteur au MILIEU d'une piste — un offset faux passerait inaperçu sur le
premier — puis compare le fichier octet à octet avec son état précédent :
exactement 512 octets, contigus, doivent avoir changé, et les secteurs
voisins rester intacts. Il vérifie aussi que `persist_sector` renvoie bien
`true`, sans quoi le test ne validerait que le repli et masquerait un calcul
d'offset faux.
