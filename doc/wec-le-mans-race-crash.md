# WEC Le Mans (2e bug) : redémarrage ~1 s après le lancement de la course (ouvert — piste : timing FDC)

Note d'enquête. **Le symptôme n'est pas résolu.** L'enquête a suivi
plusieurs pistes successives, dont plusieurs ont été abandonnées après
vérification (voir les sections « Ce qui a été écarté » et « Le FDC est
au cœur du chemin de données », cette dernière revenant sur une
conclusion intermédiaire erronée qui écartait le FDC à tort).

**Le fait central : Caprice32 lance la course normalement avec la même
disquette et les mêmes ROM, alors que nous redémarrons.** Les octets en
mémoire sont pourtant identiques des deux côtés (`0x4375=0x1F`,
`table[15]→0xEF06`, vérifiés). La divergence est donc dans l'exécution,
et **le bug est bien chez nous** — ce n'est ni un défaut de la disquette,
ni un problème de chargement, ni une question de timing FDC (les trois
ont été testés et écartés, voir plus bas).

Piste courante : le dispatcher ne saute sur la table que si un **drapeau
« objet actif »** (`IX+1`) est non nul. L'hypothèse à tester est que cet
objet de type 15 devrait rester inactif, et qu'il est actif à tort chez
nous.

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

## Comparaison Caprice32 : la table est identique, bit pour bit — ce n'est pas un bug de chargement

Comparaison faite avec Caprice32 (même ROM combinée AZERTY 32 Ko, même
fichier disque), méthode habituelle (voir
`doc/wec-le-mans-frozen-splash.md`, section « Harnais de diagnostic »,
injection directe dans `keyboard_matrix` plutôt que `--autocmd`). La
frappe de `run"wec` a fonctionné (capture d'écran : menu « WEC LE MANS /
1. JOYSTICK / 2. KEYBOARD / 3. REDEFINE KEYS » correctement atteint,
générique de la disquette visible — `BYTES.. BLOBS.. BITS.. BUZZ..`,
signature typique d'un groupe de crack de l'époque, à noter). La touche
« 2 » pour lancer la course en clavier n'a en revanche pas pu être
déclenchée de façon fiable dans le temps disponible (piège clavier
différent de celui déjà documenté pour `run"wec`, probablement lié à la
façon dont le jeu lit la matrice pour ce menu plutôt qu'au clavier du
firmware — non résolu, pas creusé davantage, le dump de la table au
niveau du menu suffit à trancher la question qui nous intéressait).

Dump de la table à `0x43DC` chez Caprice32, juste après l'affichage du
menu (donc après chargement complet de `WEC.BI1`/`WEC.BI2` depuis le
disque, avant tout calcul dépendant du déroulement réel de la course) :

```
type  0 → 0020    type  8 → 2546    type 15 → EF06  ← INVALIDE, IDENTIQUE
type  1 → 252D    type  9 → 1F80    type 16 → 0000 (garbage, identique)
type  2 → 1F15    type 10 → 1F80    type 17 → 0A01 (garbage, identique)
...               ...               ...
```

**Byte pour byte identique à notre propre dump**, y compris l'entrée 15
invalide (`0xEF06`) et le motif de données non initialisées au-delà. La
table est donc construite ainsi dès le chargement du disque, sur les deux
émulateurs, avec la même ROM et la même image disque : **ce n'est pas un
problème de chargement/décompression de notre côté.**

> **ATTENTION — ne pas en tirer la conclusion inverse.** Une version
> antérieure de cette note enchaînait ici sur « donc c'est un défaut de
> cette image disque crackée, le jeu planterait aussi sur un vrai 6128 ».
> **C'est faux : sous Caprice32, avec cette même disquette, la course se
> lance normalement et ne plante pas** (constaté par l'utilisateur).
>
> Données identiques + comportements différents = **le problème est chez
> nous, dans l'exécution**, pas dans la donnée. La piste « disquette
> crackée défectueuse » est morte, et avec elle l'idée que ce bug ne
> serait pas à corriger dans ce dépôt. Il l'est.

## Le FDC est au cœur du chemin de données (et le piège qui l'avait masqué)

Cette piste a d'abord été écartée à tort, sur un raisonnement qui
s'arrêtait une étape trop tôt. Le piège vaut d'être noté, car il est
facile d'y retomber :

> l'octet fautif est écrit en `0x4374` par un `LDIR` en `0xC8CF`
> (source `0xAB50`, destination `0x4360`, 96 octets) à t=12,78 s ; or le
> dernier `CAS_IN_OPEN` (`WEC.BI2`) date de t=10,03 s ; donc « aucune
> lecture disque entre les deux », donc « copie RAM→RAM ordinaire », donc
> FDC hors de cause.

**L'étape manquante : d'où vient le contenu de `0xAB50` ?** Et,
corollaire : *compter les `CAS_IN_OPEN` ne dit rien de l'activité disque
réelle* — une ouverture de fichier est suivie de plusieurs secondes de
lectures secteur par secteur.

En traçant les écritures dans `0xAB50`-`0xAB7F`, la réponse est sans
ambiguïté : **3707 écritures sur la session**, toutes venant de
`PC=0xC6E2`, avec la ROM haute n° 7 (AMSDOS) active. Le désassemblage
autour de ce point ne laisse aucun doute — c'est la boucle interne de
lecture de secteur du FDC :

```
C6DE  LD B,$0C
C6E0  IN A,(C)      ; lit le registre de donnees du FDC
C6E2  LD (HL),A     ; <-- ecrit l'octet lu dans le tampon (0xAB50+)
C6E3  DEC C
C6E4  INC HL
C6E5  IN A,(C)      ; relit le statut
C6E7  JP P,$C6E5    ; attend que le FDC soit pret
C6EA  AND $20
C6EC  JR NZ,$C6DF   ; octet suivant
C6EE  RET
```

`0xAB50` **est le tampon de secteur d'AMSDOS**, alimenté directement par
le port de données du FDC. Et le `LDIR` de `0xC8CF` (lui aussi en ROM
AMSDOS) n'est pas une copie RAM→RAM arbitraire : c'est l'étape qui livre
un enregistrement du fichier depuis ce tampon vers la mémoire du
programme. Les lectures disque **continuent bien au-delà de t=10,03 s** :
un `CAS_IN_OPEN` ouvre le fichier, mais les enregistrements sont ensuite
tirés secteur par secteur pendant plusieurs secondes. Compter les
`CAS_IN_OPEN` ne dit rien de l'activité disque réelle.

Plus frappant encore, la chronologie fine autour de l'octet fautif
(`0xAB64`, celui qui devient le type d'objet 15 après `SUB $10` /
`AND $0F`) :

```
t=12,7659 s   0xAB64 <- 0x10   (ecrit par le FDC, C6E2)
t=12,7768 s   LDIR lit le tampon  <-- fenetre de 11 ms
t=12,7890 s   0xAB64 <- 0x00   (ecrase par le secteur suivant)
```

Le `LDIR` lit ce tampon dans une fenêtre de **11 ms**, entre deux
réécritures espacées d'environ 23 ms.

**Attention : il est tentant d'en conclure à une course dont l'issue
dépendrait du timing. C'est faux, et ç'a été vérifié directement.** En
rejouant la scène avec la frappe décalée de 0, 3, 5, 8, 11, 17, 23 et
40 ms (donc bien au-delà de la période de réécriture de 23 ms), le
résultat est *rigoureusement identique à chaque fois* : type d'objet
`0x0F` en `0x8662`, et redémarrage à +1,18 s.

```
offset  0 ms : types vus en 0x8662 = [00, 0F]   reboot = OUI a +1.184s
offset 11 ms : types vus en 0x8662 = [00, 0F]   reboot = OUI a +1.173s
offset 23 ms : types vus en 0x8662 = [00, 0F]   reboot = OUI a +1.181s
offset 40 ms : types vus en 0x8662 = [00, 0F]   reboot = OUI a +1.200s
```

La raison est simple une fois vue : le remplissage du tampon et le `LDIR`
ne sont pas deux processus indépendants qui courent l'un contre l'autre,
ce sont **deux étapes successives d'un même appel de lecture AMSDOS**
(remplir le tampon de secteur, puis en extraire l'enregistrement). La
« fenêtre de 11 ms » n'est que l'intervalle normal entre ces deux étapes.
Décaler la frappe décale les deux ensemble. Le timing du FDC n'est donc
**pas** la variable recherchée — le chemin de données passe bien par le
FDC, mais son résultat est déterministe.

Ceci ne contredit pas la comparaison Caprice32 de la section précédente :
la table `0x43DC` y était bien identique, mais elle a été relevée au
niveau du **menu**, avant cette phase de chargement continu. Les deux
observations portent sur des instants différents.

## Données confirmées identiques chez Caprice32 (2e relevé)

Second passage sous Caprice32 (même ROM, même disquette), relevé au
niveau du menu :

```
0x4375 = 1F          (octet source du type d'objet — IDENTIQUE au notre)
table[15] -> EF06    (entree de dispatch invalide — IDENTIQUE)
```

Les données chargées depuis le disque sont donc **au même endroit avec
la même valeur** dans les deux émulateurs. Combiné au caractère
déterministe démontré plus haut, cela veut dire qu'un émulateur correct
exécutant ce code arrivera au même `JP (HL)` vers `0xEF06`.

**Et pourtant Caprice32 ne plante pas :** avec cette même disquette, la
course s'y lance normalement (constaté par l'utilisateur en usage
interactif). Mes trois tentatives d'automatiser la touche « 2 » sous
Caprice32 ont échoué (le menu reste affiché) alors que la même méthode
passe pour `run"wec` — mais c'est une limite de mon harnais, pas du jeu.
**Ne pas repartir sur cette automatisation sans idée neuve** : trois
essais y sont déjà passés (pressions longues en trames, pressions
courtes en nombre d'instructions, avec et sans SHIFT).

**C'est le fait central de l'enquête :** mêmes ROM, même disquette,
mêmes octets en mémoire (`0x4375=0x1F`, `table[15]→0xEF06`) — et
pourtant Caprice32 joue la course quand nous redémarrons. La divergence
est donc dans **l'exécution**, et le bug est chez nous.

## Piste du drapeau « objet actif » : testée et ÉCARTÉE

Hypothèse testée : un objet de type 15 resterait **inactif** sur matériel
réel, et serait actif à tort chez nous, d'où le saut vers `0xEF06`.
**Fausse, sur deux plans.**

D'abord un piège de désassemblage. À `0x1ED4` l'instruction n'est *pas*
`LD A,(IX+1)` mais `LD A,IXL`, et `IX` est **décalé de −0x30 juste
après** ; relever `IX` en `0x1ED4` donne donc une base d'objet fausse
(et fait surveiller la mauvaise adresse en mémoire) :

```
1ED4  DD 7D     LD A,IXL      <-- PAS le drapeau
1ED6  D6 30     SUB $30
1ED8  DD 6F     LD IXL,A      <-- IX ajuste ICI
1EDA  DD 7E 01  LD A,(IX+1)   <-- le vrai relevé du drapeau
```

Relevé correctement (`IX=0x8752`, drapeau en `0x8753`), le drapeau de
l'objet fautif vaut **`0x00`** — il est donc bien *inactif*. Et pourtant
le dispatch a lieu. La logique du branchement explique pourquoi :

```
1EDA  LD A,(IX+1)   ; drapeau = 0x00
1EDD  AND A         ; Z=1
1EDE  LD A,(HL)     ; A = type (LD n'affecte pas les flags : Z reste a 1)
1EDF  JP Z,$1EE6    ; drapeau NUL -> saute en 1EE6
1EE2  CP $05        ; (branche "drapeau non nul")
1EE4  JR NZ,$1F05   ;   -> seul le type 5 y est traite
1EE6  OR A          ; A = type ; non nul -> Z=0
1EE7  JR Z,$1F05
1EE9  EX AF,AF'     ; DISPATCH
```

Autrement dit, c'est l'inverse de ce que je supposais : **un drapeau nul
est le chemin *normal* du dispatch par type** (n'importe quel type non
nul y passe), tandis qu'un drapeau non nul restreint le traitement au
seul type 5. Le jeu dispatche donc légitimement sur le type 15. Rien
d'anormal du côté du drapeau.

## Piste suivante : quand `0x4375` prend-il la valeur `0x1F` ?

Un écart relevé au passage, **à vérifier avant d'en tirer quoi que ce
soit** (les faux pas précédents invitent à la prudence) :

- chez nous, `0x4375` vaut `0x50` au moment du menu, et ne devient `0x1F`
  qu'après le `LDIR` de `0xC8CF` déclenché par le lancement de la course
  (t=12,777 s, après la frappe du « 2 ») ;
- sous Caprice32, le relevé fait **au menu**, sans qu'aucune touche n'ait
  été pressée, donne déjà `0x4375 = 0x1F`.

Si l'écart se confirme, deux lectures possibles : soit les bases de temps
des deux relevés ne sont pas comparables (le « menu » de Caprice32 est
pris à la trame 850, bien plus tard que nos 12 s — piège classique déjà
rencontré deux fois ici), soit le contenu de cette zone diverge
réellement à état de jeu équivalent, ce qui serait la première divergence
mémoire observée entre les deux émulateurs. **À trancher en comparant à
état de jeu identique, pas à temps écoulé identique.**

Note : l'audit demandé sur `LDIR` lui-même n'a rien donné — les
drapeaux documentés (H, N, P/V) sont correctement posés par les
gestionnaires `0xEDB0`/`0xEDA8`/`0xEDB8` (et non dans `ldi`/`ldd`, ce qui
peut tromper à la lecture), l'interruptibilité, les cycles et le cas
`BC=0` (64 Ko) sont couverts par des tests. Seuls les deux bits **non
documentés** (3 et 5, dérivés de `A + octet transféré`) ne sont pas
posés — écart réel mais sans effet plausible ici, aucun code ne les
observant hors `PUSH AF`.

## Harnais de diagnostic

Tests `investigate_wec2_*` dans `src/machine.rs`, tous retirés après
cette session. Reproduction : taper `run"wec`, attendre ~12 s (le menu
apparaît), presser « 2 » (ligne 8 bit 1 + SHIFT, table AZERTY), puis
observer ~6 s. Méthode de capture d'écran identique à l'enquête
précédente (buffer RGB24 → PPM → PNG via `magick`).
