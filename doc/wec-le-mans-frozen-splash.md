# WEC Le Mans : reste figé sur l'écran de démarrage (ouvert)

Note d'enquête, point de départ pour une reprise ultérieure. **Le symptôme
n'est pas résolu**, mais la boucle figée est désormais entièrement
identifiée, et l'hypothèse « attente clavier » de la session précédente
est réfutée.

## Le symptôme

```
cargo run -- -d WEC_Le_Mans.dsk -a 'run"wec'
```

L'image de démarrage (splash "WEC Le Mans") s'affiche puis reste figée
indéfiniment. Sur Caprice32 avec la même disquette et la même commande, le
menu du jeu devrait s'afficher après quelques secondes.

## Contenu de la disquette

Pas de `WEC.BAS` : `RUN"WEC"` lance `WEC.BIN`. La disquette contient
`WEC.BIN`, `WEC.SCR` (l'écran de démarrage) et `TRACK0F.BIN`. La commande
utilisée est donc bien la bonne.

## Ce qui a été écarté

- **Pas un blocage type TMHT** (DI jamais suivi d'un EI) : le compteur
  d'interruptions acceptées avance normalement.
- **Pas une simple attente de touche/tir manette** : simuler un appui
  ESPACE puis un tir manette après le figement ne change rien.
- **Pas une attente clavier du tout** (hypothèse de la session
  précédente, désormais réfutée — voir ci-dessous : la boucle n'a
  strictement aucune lecture d'I/O).
- **Pas un échec disque** : le chargement se termine normalement à 4,80 s
  (phase de résultat sans erreur, puis `OUT &FA7E,00` qui coupe le
  moteur). Aucun accès FDC ensuite.
- **Pas un défaut du registre R** : la routine de temporisation utilise
  `LD A,R`, mais la crate `../ZilogZ80` l'émule correctement (incrément
  7 bits avec bit 7 préservé, recopie de IFF2 dans P/V, et le cas
  « interruption pendant l'instruction »), tests à l'appui.
- **Pas la direction du port A du PSG** : R7 vaut 0x3F au moment décisif,
  donc bit 6 = 0, port A en entrée — lire R14 doit bien rendre le
  clavier, ce que fait notre émulateur.
- Une adresse mémoire (`0xB831`) qui semblait être le drapeau attendu
  s'est révélée être un octet à usage multiple — fausse piste.

## Ce que fait réellement la boucle figée

Ce n'est pas une attente : c'est une **écriture cassette**. Le jeu pousse
un flux d'octets sur le bit 5 du port C du PPI (« cassette write data »),
en basculant ce bit via le mode Bit Set/Reset du 8255 (`OUT &F7xx` avec
0x0A/0x0B). La temporisation entre fronts est calibrée sur `LD A,R`.

Chemin d'appel complet, relevé par traçage des branchements :

```
29B3  CALL $2AD4          ; entrée « écriture cassette »
2AD4  CALL $2BF9          ; temporisation
2AD7  LD HL,$0801         ; 2049 impulsions de tonalité d'amorce
2ADA  CALL $2AEC
...
2A67  CALL $BAD7          ; lit (IX) ROMs désactivées (helper Gate Array)
2A6A  CALL $2B68          ;   -> émet les 8 bits de l'octet
2A6E  INC IX              ;   -> octet suivant, indéfiniment
```

`0xBAD7` est le classique « lire sous la ROM » : `OUT &7F88` avec 0x8C
(ROMs coupées), `LD A,(IX+0)`, puis 0x88 pour rétablir — d'où les seules
écritures Gate Array observées.

Mesure sur 115 s émulées : IX progresse **de façon monotone** de 0x0C4F à
0x53AC, soit ~160 octets/s (débit d'une vraie cassette), sans jamais
s'arrêter ni reboucler. Le jeu déverse donc la mémoire entière sur le port
cassette. Sur toute la fenêtre observée, **aucune lecture d'I/O** : que
des écritures (Gate Array + PPI).

## Le point de décision exact

Tout se joue sur une seule instruction, en `0x2AEC` :

```
2AEC  LD B,$F4      ; port &F4xx = port A du PPI
2AEE  IN A,(C)
2AF0  AND $04       ; bit 2
2AF2  RET Z         ; bit 2 à 0 -> on saute toute l'écriture cassette
```

État relevé à cet instant : PPI `control=0x92` (port A en entrée),
`port_c=0x58` (bits 7-6 = 01 → PSG en lecture ; bits 3-0 = 8 → ligne
clavier 8 ; bit 4 = 1 → moteur cassette), PSG `selected_register=14`.

La lecture rend donc **la ligne 8 de la matrice clavier**, dont le
**bit 2 est la touche ESC** (0 = enfoncée sur CPC). Autrement dit : ESC
maintenue ⇒ le jeu saute l'écriture cassette ; ESC relâchée ⇒ il l'exécute.
ESC est la touche d'**abandon** d'une sauvegarde cassette volontaire.

Notre émulateur rend 0xFF (aucune touche), ce qui est correct au vu de
l'état matériel — le problème n'est donc pas cette lecture elle-même.

## Pourquoi ce n'est pas la cause racine

Expérience décisive : en maintenant artificiellement ESC juste avant le
test, le jeu franchit bien la barrière et repart pour de bon (17 795
adresses distinctes visitées, contre ~50 en figement). Mais il se fige
alors dans une **seconde** boucle, celle-ci franchement anormale :

```
42BD  30 30    JR NC,$42EF
42EF  20 CC    JR NZ,$42BD
```

Deux sauts qui se renvoient la balle, **sans aucun accès I/O** et sans
aucune instruction susceptible de modifier les indicateurs qu'ils
testent : rien ne peut en sortir. C'est un plantage, pas une attente.

Conclusion : le saut vers l'écriture cassette **et** ce plantage sont tous
deux des conséquences d'une divergence plus en amont, entre la fin du
chargement disque (4,80 s) et 8,80 s. Forcer ESC ne fait que déplacer le
symptôme.

## Ce qui reste à faire

Trouver ce qui déraille entre 4,80 s (fin du chargement, moteur coupé) et
8,80 s (appel de l'écriture cassette). Pistes :

- remonter au décideur : qui appelle la routine contenant `0x29AF` ?
  Il existe deux entrées voisines, `0x29A6` (qui charge : `LD HL,$2A28`)
  et `0x29AF` (qui écrit : `CALL $2AD4`) — c'est le choix entre les deux
  qu'il faut expliquer ;
- surveiller (watchpoints) les variables `0xB1E5`/`0xB1E8`-`0xB1EB` qu'utilise
  le système cassette, pour voir qui les arme ;
- vérifier le décodage de `TRACK0F.BIN` : la disquette a un format de
  piste 0 particulier, et le jeu charge ensuite des pistes brutes ;
- comparer avec Caprice32 si un moyen fiable de trace est disponible
  (tenté sans succès : ni capture X11 ni injection clavier ne
  fonctionnent dans cet environnement).

## Harnais de diagnostic

Tests `investigate_wec*` dans `src/machine.rs` et journal d'I/O temporaire
`CpcBus::io_trace` dans `src/bus.rs`, tous deux retirés après la session.
Méthode réutilisable, éprouvée ici :

1. journal d'I/O armable (`RefCell<Option<Vec<(bool, u16, u8)>>>`) autour
   de `read_io`/`write_io`, vidé et filtré par tranches pour ne pas
   saturer la mémoire sur de longues fenêtres ;
2. `Tracer` en mode `Branches` + déduplication des adresses consécutives,
   indispensable : les boucles de temporisation serrées noient sinon le
   tampon circulaire (65 536 entrées) ;
3. carte de bits des PC visités sur plusieurs dizaines de millions de pas,
   affichée en plages — c'est ce qui a montré que la « boucle figée »
   progressait en réalité (IX croissant) ;
4. désassemblage à partir de points d'entrée sûrs (cibles de `CALL`
   relevées dans la trace), jamais linéairement depuis une adresse
   arbitraire.

Note : la console de l'émulateur réimprime son prompt `> ` sur stdout
pendant les tests ; filtrer la sortie (`sed 's/> //g'`) et préfixer les
lignes utiles (ici `WEC#`) rend les relevés lisibles.
