# WEC Le Mans (2e bug) : redémarrage ~1 s après le lancement de la course (ouvert)

Note d'enquête, point de départ pour une reprise ultérieure. **Le
symptôme n'est pas résolu.** Contrairement au premier bug WEC Le Mans
(écran de démarrage figé, voir `doc/wec-le-mans-frozen-splash.md`,
résolu), celui-ci n'a pas encore de cause confirmée.

## Le symptôme

```
cargo run -- -d WEC_Le_Mans.dsk -a 'run"wec'
```

Une fois le menu principal atteint (« WEC LE MANS / 1. JOYSTICK /
2. KEYBOARD / 3. REDEFINE KEYS »), sélectionner « 2 » (clavier) lance la
course : l'écran de préparation s'affiche brièvement (visible : "PRE..."
et des panneaux d'interface en cours de dessin), puis la machine
redémarre à froid (retour à l'écran « Amstrad 128K Microcomputer... BASIC
1.1... Ready ») environ une seconde après.

## Confirmé : c'est un vrai redémarrage, pas un blocage

Capture d'écran ~1 s après avoir relâché la touche « 2 » : l'écran de
préparation de course a bien commencé à se dessiner (route/panneaux
visibles), puis les captures suivantes montrent l'écran de boot BASIC. Le
CPU exécute effectivement le code d'amorçage du firmware (`PC`
descendant sous `0x0100`, y compris `0x0000` lui-même, le vecteur de
reset) — ce n'est pas un CPU figé dans une boucle.

## Ce qui a été tracé

`PC=0x0000` est atteint à **+1,18 s** après le relâchement de la touche
« 2 », par un chemin d'exécution qui dérive dans de la mémoire vide en
haut de l'espace d'adressage (`0xFFB6`-`0xFFEA`, octets `0xFF` = `RST
$38`, le motif classique d'un PC parti dans de la mémoire non
initialisée). Cette dérive part d'un `RET` (en `0x3158`) qui dépile une
adresse de retour absurde (`0xFFB5`/`0xFFB2` selon l'essai) — signe d'une
pile corrompue.

### La pile de secours de l'interruption

Le gestionnaire d'interruption du jeu (`0x0038 → 0x309B`) utilise une
astuce classique en Z80 : basculer temporairement `SP` vers une petite
zone de code (`0x3143`), y empiler quelques registres (une écriture
rapide, plus rapide que des `LD (nn),A` répétés), lire clavier/manette,
agir sur les ports, puis restaurer `SP`. La restauration elle-même est du
code auto-modifiant : `0x309B LD ($310E),SP` écrit la valeur de `SP`
au moment de l'interruption directement dans les 2 octets opérandes de
`0x310D LD SP,nn` — donc désassembler statiquement `0x310D` ne montre
JAMAIS la vraie valeur restaurée à un instant donné, seule une trace
d'exécution réelle le peut (piège déjà rencontré plusieurs fois durant
l'enquête sur le premier bug WEC).

```
309B  LD ($310E),SP     ; sauvegarde le SP interrompu
309F  LD SP,$3143        ; bascule vers la pile de secours
30A2  PUSH AF / BC / HL   ; ...utilisation de la pile de secours...
...                        (lecture clavier/manette, écritures de ports)
310A  POP HL / BC / AF
310D  LD SP,nn            ; nn auto-modifié = ce que 0x309B a sauvegardé
3110  EI
3111  RET
```

Sur la quasi-totalité des occurrences observées (plus de 130 dans la
fenêtre tracée), cette paire bascule/restauration est parfaitement
équilibrée. **Juste avant le plantage (~1,178 s), une bascule
(`LD SP,$3143`) n'est jamais restaurée avant la bascule suivante** — et
les restaurations qui suivent ramènent `SP` à des valeurs voisines de
`0x3143` (`0x3141`, `0x3135`, `0x3139`...) au lieu de la valeur saine
habituelle (`0x023x`, observée en tout début de partie). `SP` a donc
déjà dérivé dans la zone `0x31xx` — la même zone que le code de la pile
de secours elle-même — avant même l'appel `0x08FD → 0x3143` qui finit
par produire le `RET` fatal.

## Ce qui a été écarté (mis à jour, définitivement cette fois)

- **Pas une imbrication d'interruptions.** Premier détecteur (surveiller
  un retour à `0x3111`) invalidé par un vrai piège méthodologique : le
  gestionnaire a DEUX chemins de sortie (un chemin court,
  `0x3090`-`0x309A`, sans bascule de pile ; un chemin long,
  `0x309B`-`0x3111`, avec la pile de secours), et un détecteur qui ne
  surveille que la sortie du chemin long se dérègle dès que le chemin
  court est emprunté une seule fois — ce qui explique les dizaines de
  « fausses imbrications » vues au premier essai. **Détecteur refait
  correctement** (en ne comptant comme entrée que les cas où
  `has_pending_int()` passe de vrai à faux — donc une vraie acceptation
  matérielle, pas un appel explicite du jeu vers `0x0038` comme
  sous-routine ordinaire, ce qui arrive aussi et fausse le comptage) :
  **1795 interruptions matérielles réelles observées sur la fenêtre,
  zéro imbrication.** Cette piste est refermée pour de bon.
- Le tout premier bug WEC (RST qui saute l'incrément du PC) est déjà
  corrigé et vérifié sans effet secondaire sur ce nouveau symptôme — ce
  n'est pas une résurgence du même bug, au moins pas directement (le
  mécanisme ici est un `RET` qui dépile une mauvaise adresse, pas un
  opérande de far-call mal aligné).
- **Piste « corruption de HL par une interruption mal placée »,
  explorée puis écartée après vérification directe.** Une trace pas à
  pas semblait montrer un simple `INC HL` sauter de `441A` à `EF1B` —
  ce qui aurait été un vrai bug de CPU si confirmé. Revérifié
  instruction par instruction avec `has_pending_int()`/`iff1` à chaque
  pas : **aucune interruption ne survient dans cette fenêtre, et
  `INC HL` se comporte normalement** (`441A → 441B`). La première trace
  était juste mal interprétée sous le coup de la précipitation (le `H`
  affiché après l'instruction SUIVANTE, `LD H,(HL)`, avait été confondu
  avec un HL corrompu par l'instruction en cours).

## La vraie piste : une table de saut avec une entrée invalide

En repartant de zéro sur l'écran de préparation, la toute première
dérive dans de la mémoire vide (`PC=0xEF06`, à 0,96 s, avant même que
`SP` ne soit affecté) vient d'un saut calculé parfaitement normal :

```
1EE9  EX AF,AF'        ; bascule vers le jeu de registres alternatif
1EEA  LD A,C
1EEB  EXX                ; HL/DE/BC alternatifs deviennent actifs
1EEC  EX AF,AF'           ; A reprend sa valeur alternative (index d'objet)
1EED  RLCA
1EEE  RLCA                ; index ×4 (table de 4 octets par entrée)
1EEF  LD C,A
1EF0  LD B,$00
1EF2  LD HL,$43DC          ; base de la table
1EF5  ADD HL,BC             ; HL = &table[index]
1EFA  LD E,(HL) / INC HL
1EFC  LD D,(HL) / INC HL
1EFE  LD A,(HL) / INC HL
1F00  LD H,(HL)
1F01  LD L,A                 ; HL = adresse cible lue dans la table
1F03  JP (HL)                 ; saute — table de dispatch par objet/entité
```

Pour l'occurrence fautive, l'index calculé vaut `0x3C` (60 décimal),
menant à `table[60]` en `0x4418`. Les 4 octets lus là (`E`, `D`, puis le
couple `A`/`H` qui forme l'adresse cible) donnent une cible de
**`0xEF06`** — de la mémoire vide, jamais chargée. Ce n'est pas une
corruption en cours d'exécution : les octets à `0x4418`-`0x441B`
contiennent authentiquement cette valeur invalide au moment de la
lecture (vérifié directement, sans qu'aucune écriture n'intervienne
entre-temps).

Cette excursion-là se rétablit d'elle-même (retombe dans une petite
boucle `RST $38` sans casser `SP`) ; c'est une excursion **suivante**,
plus tard, qui finit par corrompre `SP` (voir plus haut, la pile de
secours jamais restaurée) et provoque le vrai plantage vers `0x0000`.
Les deux sont vraisemblablement liées à la même cause : quelque chose
fait consulter un index d'objet/entité hors de portée dans cette table
de dispatch, et selon la table concernée, l'atterrissage est plus ou
moins destructeur.

## Origine de l'index confirmée : ce n'est pas `A'` mais un octet lu en RAM dynamique

En remontant depuis `0x1EE9`, l'index utilisé n'est PAS directement le
registre alternatif `A'` (celui-ci n'est qu'un relais temporaire entre
deux appels successifs à ce dispatcher, sans rapport avec la donnée
elle-même). La vraie source, tracée instruction par instruction :

```
1ED0  LD A,L / ADD A,$20 / LD L,A   ; avance HL de $20 (page) si besoin
1ED4  LD A,(IX+1)                    ; drapeau "objet actif" ?
1EDD  AND A                          ; teste ce drapeau (Z)
1EDE  LD A,(HL)                      ; charge le TYPE d'objet depuis la RAM
1EDF  JP Z,$1EE6                     ; si drapeau inactif, saute le dispatch
1EE2  CP $05 / JR NZ,$1F05           ; (branche secondaire, type 5 à part)
1EE6  OR A / JR Z,$1F05              ; type 0 = rien à faire
1EE9  EX AF,AF' ... (dispatch avec le type comme index)
```

`HL` parcourt une table d'état d'objets dynamique en RAM, à
`0x86xx`-`0x87xx` (pas la table de saut statique `0x43DC` elle-même) :
chaque emplacement contient le **type courant de l'objet** occupant ce
slot. C'est cet octet, lu directement depuis la RAM de jeu, qui sert
d'index (après ×4) dans la table de dispatch `0x43DC`.

Avec un point d'arrêt en écriture sur `0x8600`-`0x867F` : toute la zone
est mise à zéro au moment où l'écran de course s'initialise (`PC=0x04CA`,
juste après le lancement), **puis, presque immédiatement après**, le
code en `0x2661` écrit `0x0F` (15) dans l'emplacement `0x8662` — un seul
octet, une seule fois, très tôt dans l'initialisation de la course (bien
avant que quoi que ce soit dépende du timing de la partie). D'autres
emplacements reçoivent ensuite `0x04` à intervalles réguliers (objets
de type 4 créés périodiquement, ex. voitures adverses). **La création de
l'objet de type 15 est donc déterministe et systématique au lancement de
la course, pas un effet de bord du timing.**

## La table de dispatch confirmée incomplète

Dump complet de la table à `0x43DC` (32 entrées de 4 octets, `E D A H`
→ cible `HA`) :

```
type  0 → 0020    type  8 → 2546    type 16 → 0000 (garbage)
type  1 → 252D    type  9 → 1F80    type 17 → 0A01 (garbage)
type  2 → 1F15    type 10 → 1F80    type 18 → 0000 (garbage)
type  3 → 1F15    type 11 → 2240    type 19 → 0110 (garbage)
type  4 → 2219    type 12 → 0000    type 20 → 00C6 (garbage)
type  5 → 2302    type 13 → 2240    ...
type  6 → 2219    type 14 → 23EF
type  7 → 241F    type 15 → EF06  ← INVALIDE
```

Les cibles des types 0 à 14 (sauf le 12, à `0x0000`, sans doute un type
jamais instancié) sont toutes des adresses plausibles dans la zone de
code du jeu (`0x1F00`-`0x2600`). À partir du type 15, les valeurs
deviennent incohérentes (`EF06`, puis des motifs qui ressemblent à des
octets de code voisins mal alignés plutôt qu'à de vraies adresses cibles)
— la table **s'arrête réellement après 15 entrées valides (types 0-14)**.
Ce n'est pas une entrée isolée corrompue : c'est la table qui est courte
d'au moins une entrée face à un type d'objet (15) que le jeu instancie
pourtant systématiquement et immédiatement au lancement de la course.

## Conclusion (provisoire, à confirmer face à Caprice32)

Le tableau ci-dessus penche fortement pour l'hypothèse 1 (donnée
manquante) plutôt que 2 (index qui dérive) : la création de l'objet de
type 15 est déterministe, immédiate, et ne dépend d'aucun état de jeu
variable observé jusqu'ici. Reste à vérifier si Caprice32, avec la même
disquette, a bien une entrée 15 valide dans sa copie de la table (ce qui
confirmerait un problème de chargement/décompression chez nous,
peut-être un ricochet du même mécanisme que le premier bug WEC) ou si
lui aussi ne l'a pas mais ne la sollicite jamais à cause d'un détail de
timing qui nous échappe encore (ce qui semble moins probable vu le
caractère déterministe de la création de l'objet).

## Prochaine étape recommandée

Comparer directement avec Caprice32 (ROM identiques, injection clavier
directe dans `keyboard_matrix`, voir `doc/wec-le-mans-frozen-splash.md`
section « Harnais de diagnostic ») : dumper la même table à `0x43DC` au
même instant (juste après le lancement de la course) pour voir si
l'entrée 15 y est valide. Si oui, comparer ensuite la mémoire chargée
depuis le disque (avant toute exécution du code de jeu) entre les deux
émulateurs pour localiser où la divergence apparaît — probablement dans
la zone source dont `0x43DC` est recopiée/décompressée.

## Harnais de diagnostic

Tests `investigate_wec2_*` dans `src/machine.rs`, tous retirés après
cette session. Reproduction : taper `run"wec`, attendre ~12 s (le menu
apparaît), presser « 2 » (ligne 8 bit 1 + SHIFT, table AZERTY), puis
observer ~6 s. Méthode de capture d'écran identique à l'enquête
précédente (buffer RGB24 → PPM → PNG via `magick`).
