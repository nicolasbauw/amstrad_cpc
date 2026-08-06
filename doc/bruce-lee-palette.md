# Bruce Lee : décor incomplet après le lancement de la partie (ouvert)

Note d'enquête, point de départ pour une reprise ultérieure. **Le symptôme
n'est pas résolu.**

## Le symptôme

```
cargo run -- -d Bruce_Lee.dsk -a 'run"bruce'
```

Au menu (deux ENTRÉE : un pour l'afficher, un pour lancer la partie), seule
une fraction du décor s'affiche, ni les personnages ni le reste du décor —
absent sur Caprice32 avec la même disquette et la même commande.

## Reproduction automatisée

Boot via `--autocmd`, deux appuis simulés sur ENTRÉE (matrice clavier ligne
2 bit 2, comme `\n` dans `autotype.rs`), captures d'écran périodiques. Les
captures après le 1er ENTRÉE, le 2e ENTRÉE et six trames suivantes sont
**strictement identiques** (`cmp` confirme un contenu VRAM octet pour octet
égal) : rien n'anime à l'écran sur la fenêtre observée, mais rien
n'indique que le jeu soit bloqué — le CPU visite une large plage de PC
distincts (contrairement au blocage de TMHT).

## Cause identifiée : la table d'encres du firmware reste incomplète

Mode vidéo 0 confirmé (16 encres). La VRAM (0xC000-0xFFFF) est correctement
remplie à ~45 % sur toute sa plage — le décor est bien dessiné en mémoire,
ce n'est donc pas un problème de dessin de tuiles ni de banking RAM
(`ram_config` reste à 0 tout du long, ce n'est pas un jeu qui utilise les
64 Ko supplémentaires du 6128).

Le vrai problème est la **palette** : la table d'encres que le firmware
recopie vers le Gate Array à chaque trame (routine ROM standard en
`0x0790`-`0x07AA`, appelée depuis l'interruption ; table source en RAM à
**0xB7D5-0xB7E5**, 17 octets = 16 stylos + bordure) ne contient que 4
valeurs réelles, le reste à zéro (gris, indiscernable du fond) :

```
[00, 04, 0C, 17, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00]
```

Puisque le mode 0 a 16 stylos et que le décor utilise vraisemblablement des
tuiles de plusieurs couleurs, tout ce qui est peint avec un stylo ≥ 4 est
invisible (même couleur que le fond), d'où l'illusion d'un décor "en
partie manquant" alors que les octets sont bien là.

### Chronologie précise (traçage trame par trame pendant le chargement)

Pendant l'écran de chargement (~10 s après le lancement), la table est
**complète et variée** :

```
[14, 18, 07, 17, 0C, 0B, 0A, 00, 1E, 12, 04, 0E, 0E, 0E, 0E, 0E, 14]
```

Vers 10,8 s, en deux temps :
- **PC=0x08A5** : le stylo 0 et la bordure (dernière entrée) sont remis à 0.
- **PC=0x08AB** (juste après) : la table est intégralement remise à 0.
- **PC=0x08AB** (encore, ~400 ms plus tard) : seuls les stylos 1, 2 et 3
  sont réécrits (`04, 0C, 17`) — plus aucune écriture ensuite, y compris
  après le lancement effectif de la partie (deux ENTRÉE, six trames de
  jeu observées).

### Le mécanisme d'écriture (confirmé par traçage instruction par instruction)

Un désassemblage **linéaire** à partir d'une adresse arbitraire se
désynchronise dès le premier branchement (déjà rencontré pendant l'enquête
TMHT) — ne pas s'y fier. Un traçage pas à pas de l'exécution réelle a
localisé une fonction utilitaire générique en ROM, `0x0CFD`-`0x0D19` :

```
0D02  LD C,(HL)      ; C = valeur a ecrire (fournie par l'appelant via HL)
0D03  LD A,E         ; A = index de stylo (fourni par l'appelant via E)
0D04  CALL $0D35
0D35  LD E,A
0D36  LD D,$00
0D38  LD HL,$B7E5    ; derniere entree de la table (bordure)
0D3B  ADD HL,DE
0D3C  EX DE,HL
0D3D  LD HL,$FFEF    ; -17
0D40  ADD HL,DE      ; HL = 0xB7D4 + index
0D41  RET
0D07  LD (HL),C      ; ecrit table_encres[index] = valeur
```

Cette fonction "écrit une entrée de la table d'encres" est réutilisée par
plusieurs endroits du jeu pour des raisons probablement variées (elle a
été vue appelée une seule fois pour un seul stylo, ce n'est pas
spécifiquement une routine "charge la palette du niveau"). La vraie
question ouverte est **quel appelant** échoue à parcourir les index 4 à 15
juste après le lancement de la partie, et pourquoi il s'arrête à 3.

## Ce qui reste à faire

Retrouver cet appelant (la boucle ou séquence qui devrait invoquer la
fonction ci-dessus pour les stylos 4 à 15 et ne le fait pas) demande de
poursuivre le traçage pas à pas au-delà du point où ce document s'arrête,
ou de désassembler la zone RAM du jeu autour de l'appel à `0x0D07` pour
retrouver la boucle appelante et son critère d'arrêt — un travail de
reverse engineering comparable à `doc/barbarian-demo.md`, non terminé ici.

Deux hypothèses restent ouvertes, à départager par cette suite d'enquête :
- un vrai bug du jeu (peu probable si Caprice32 affiche le décor complet
  avec la même disquette) ;
- un bug d'émulation qui fait dévier ce compteur/cette boucle plus tôt que
  prévu — auquel cas ça rejoindrait la même famille de pistes que le
  résidu de clignotement BMX Simulator (voir `doc/sprite-flicker.md`) :
  une déviation de timing/exécution qui nous fait sortir plus tôt d'une
  boucle par ailleurs correcte.

## Harnais de diagnostic

Méthode utilisée, réutilisable pour la suite :
- boot via `--autocmd`, appuis clavier simulés directement sur la matrice
  PSG (ligne 2 bit 2 pour ENTRÉE) plutôt que via `AutoTyper` (qui ne gère
  qu'une seule commande tapée au démarrage) ;
- échantillonnage périodique d'une zone mémoire (ici la table d'encres) à
  la trame près pour localiser une transition dans le temps, avant de
  descendre au pas à pas ;
- traçage pas à pas de l'exécution réelle (`Vec<u16>` de PC visités,
  dédupliqué consécutivement, désassemblé après coup) plutôt qu'un
  désassemblage linéaire à partir d'une adresse arbitraire, qui se
  désynchronise sur le premier branchement rencontré.

Le code de diagnostic (tests `investigate_bruce_lee*` dans
`src/machine.rs`, `GateArray::debug_log`) a été retiré après cette
session ; à recréer si besoin en suivant la méthode ci-dessus.
