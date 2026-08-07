# WEC Le Mans : reste figé sur l'écran de démarrage (ouvert)

Note d'enquête, point de départ pour une reprise ultérieure. **Le symptôme
n'est pas résolu**, mais la cause immédiate est désormais identifiée : à
l'ouverture de `WEC.BI2`, l'exécution dévale le jumpblock cassette du
firmware jusqu'à `CAS WRITE`, ce qui déclenche une écriture cassette de
deux minutes (voir « Cause immédiate » plus bas). **Comparaison directe
avec Caprice32 (voir tout en bas) : la corruption mémoire qui déclenche
tout ceci se produit à l'identique chez Caprice32 (mêmes registres, mêmes
octets copiés), et pourtant Caprice32 ne se fige pas — `WEC.BI2` s'ouvre
et le jeu continue.** C'est donc très probablement un vrai bug
d'émulation chez nous, pas un défaut du jeu ou de cette disquette
précise. Reste à localiser l'instruction/le mécanisme exact qui diverge.

## Le symptôme

```
cargo run -- -d WEC_Le_Mans.dsk -a 'run"wec'
```

L'image de démarrage (splash "WEC Le Mans") s'affiche puis reste figée
indéfiniment. Sur Caprice32 avec la même disquette et la même commande, le
menu du jeu devrait s'afficher après quelques secondes.

## Contenu de la disquette

Pas de `WEC.BAS` : `RUN"WEC"` lance `WEC.BIN`. La commande utilisée est
donc bien la bonne. Catalogue réel — **le vrai catalogue AMSDOS n'existe
que sur la piste 0** (secteurs C1-C4), jamais un par piste ; le traiter
par piste fait apparaître de faux fichiers (voir la correction plus bas
à propos de « TRACK0F.BIN ») :

```
WEC.BIN  (2 records,  1 extent)
WEC.BI1  (50 records, 1 extent)
WEC.BI2  (128+128+57 records, 3 extents)
```

Le jeu charge donc `WEC.BIN`, puis `WEC.BI1`, puis `WEC.BI2` — et c'est
sur ce dernier que tout se joue (voir plus bas). Attention en refaisant
l'analyse : filtrer les extensions sur une liste blanche du genre
`BAS/BIN/SCR` masque justement `BI1` et `BI2`, les deux fichiers qui
comptent.

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

## Hypothèse « détection disque/cassette » : écartée

Hypothèse testée : le jeu choisirait sa source de données (disque ou
lecteur de cassettes) en sondant le port cassette, et notre émulation de
ce port le tromperait.

Relevé de **tous** les lecteurs du port B du PPI (&F5xx, dont le bit 7 est
l'entrée cassette) sur tout le démarrage, 0 → 9 s :

```
1 seul lecteur : PC=00BA, 2176 fois, valeurs {1E, 1F}, bit 7 toujours 0
```

`0x00BA` est l'attente de VSYNC du firmware, qui ne teste que le bit 0.
**Personne ne lit jamais l'entrée cassette**, et aucun code ne teste le
bit 7. Le jeu ne fait donc pas de détection disque/cassette par ce biais :
la bascule vient uniquement du test de la touche ESC en `0x2AEC`.

Relevé complémentaire des lectures d'I/O entre 4,5 s et 9,0 s, chacune
attribuée à son PC (toutes les valeurs sont celles attendues) :

| PC | port | ce que c'est | valeurs |
|---|---|---|---|
| `00BA` | `&F5xx` | attente VSYNC du firmware | `1E`,`1F` |
| `08A3` | `&F44x` | balayage clavier du firmware, 10 lignes | `FF` (repos) |
| `2AEE` | `&F4xx` | **le test ESC qui déclenche la cassette** | `FF` |
| `C6E0`/`C6E5`, `C92x` | `&FB7x` | AMSDOS, lectures de secteurs | — |

## Cause immédiate : une cascade dans le jumpblock cassette

Le jeu ne demande jamais d'écrire quoi que ce soit. Il charge ses fichiers
par les vecteurs cassette du firmware (`CAS IN OPEN`/`IN DIRECT`/`IN
CLOSE`), qu'AMSDOS détourne vers le disque. Le code appelant, en `0xAE50`,
est on ne peut plus banal :

```
AE51  LD B,$07        ; longueur du nom
AE53  LD HL,$AE76     ; nom du fichier
AE56  LD DE,$C000     ; tampon de 2 Ko
AE59  CALL $BC77      ; CAS IN OPEN  -> une LECTURE
AE5D  CALL $BC83      ; CAS IN DIRECT
AE61  CALL $BC7A      ; CAS IN CLOSE
```

Chronologie relevée sur 200 s émulées :

```
 2.21s CAS IN OPEN  WEC.BI1  -> se charge normalement (IN DIRECT 4.49s, IN CLOSE 4.79s)
 6.23s CAS IN OPEN  WEC.BI2  retour=AE5C DE=C000 HL=AE76   <- appel correct
 6.23s CAS IN CLOSE     |
 6.23s CAS IN CHAR      |  l'exécution DÉVALE le jumpblock,
 6.23s CAS IN DIRECT    |  entrée par entrée, dans l'ordre des adresses
 6.23s CAS OUT OPEN     |
 6.23s CAS OUT CHAR     |
 6.23s CAS OUT DIRECT   |
 6.23s CAS CATALOG      v
 6.23s CAS WRITE     <- RST 1 -> routine cassette du firmware (0x29AF)
```

Autrement dit : l'ouverture de `WEC.BI2` part correctement, mais au lieu
de revenir à l'appelant (`0xAE5C`), l'exécution **retombe sur l'entrée
suivante du jumpblock**, et ainsi de suite en cascade jusqu'à `CAS WRITE`
— la seule entrée qu'AMSDOS ne détourne pas, et qui saute donc pour de bon
dans l'écriture cassette du firmware.

Le mécanisme se comprend en regardant le contenu du jumpblock (identique à
une machine vierge, donc non corrompu par le jeu) :

```
BC77..BC9B  CAS IN OPEN .. CAS CATALOG   = DF 8B A8   (RST 3, far call -> &A88B)
BC9E        CAS WRITE                    = CF AF A9   (RST 1, low jump -> 0x29AF)
BCA1        CAS READ                     = CF A6 A9
BCA4        CAS CHECK                    = CF C1 A9
```

Toutes les entrées détournées portent **les mêmes trois octets** : AMSDOS
les route vers un point d'entrée unique et distingue la fonction demandée
d'après l'adresse de retour empilée par `RST 3` (qui pointe juste après
les deux octets d'opérande, donc identifie l'entrée). C'est aussi ce qui
rend la panne si spectaculaire : chaque retour mal ajusté atterrit
mécaniquement sur l'entrée suivante.

Sur les bits 15-14 de l'adresse d'un `RST 1` (LOW JUMP) : ils encodent la
configuration ROM, d'où `0xA9AF` → saut en `0x29AF` **avec la ROM basse
active**. `0x29AF` est donc bien la routine cassette du firmware, en ROM
basse — et non du code du jeu. De même `0x29A6` (`CAS READ`) n'est jamais
atteint (compté : 0 fois), simplement parce qu'AMSDOS assure les lectures.

Le figement n'est d'ailleurs pas définitif : l'écriture cassette finit par
se terminer, et le `CAS IN OPEN` de `WEC.BI2` est rejoué avec succès vers
**130 s**. Le jeu perd donc environ deux minutes de temps émulé, ce qui à
l'écran est indiscernable d'un blocage.

### Chaîne causale complète

Le point de divergence exact entre l'appel qui marche et celui qui casse a
été isolé en capturant les deux chemins d'exécution et en les comparant
(en filtrant les adresses du gestionnaire d'interruption, qui tombe à des
instants différents et produirait des divergences fictives). Il tient en
deux instructions du dispatcher de far call, en RAM :

```
B9C9  POP HL          ; adresse de retour -> opérande du RST 3
B9CA  LD E,(HL) / INC HL / LD D,(HL)   ; DE = &A88B (bloc "far address")
B9CF  EX DE,HL
B9D0  LD E,(HL) / INC HL / LD D,(HL)   ; DE = adresse cible
B9D5  LD A,(HL)       ; A = NUMÉRO DE ROM
B9D6  CP $FC / JR NC,$B998
B9E6  CP $10
B9E8  JR NC,$B9F9     ; A >= 0x10 -> voie "sans ROM"   <-- LA divergence
```

Ce que lit le dispatcher dans le bloc `&A88B` :

| appel | bloc `&A88B` | cible | n° ROM | voie prise |
|---|---|---|---|---|
| `WEC.BI1` (2,21 s) | `30 CD 07` | `&CD30` | **7** (AMSDOS) | voie ROM → chargement OK |
| `WEC.BI2` (6,23 s) | `0C BD 72` | `&BD0C` | **0x72** | voie « sans ROM » → cascade |

Et le bloc est bel et bien écrasé entre les deux, par du **code du jeu** :

```
2,08 s  PC=CCC1/CCC4/CCCA (ROM AMSDOS) écrit 30 CD 07 en &A88B  <- installation correcte
4,80 s  PC=A74B (code du jeu)         écrase &A880-&A8BC
```

Le coupable, en `&A73C` :

```
A742  LD DE,$A800     ; destination fixe
A745  LD A,($A7FD)    ; longueur
A74A  LD C,A          ; BC = 0x00BD = 189 octets
A74B  LDIR            ; écrit &A800..&A8BC — donc &A88B-&A88D
```

Soit, bout à bout :

1. le jeu charge `WEC.BI1` par `CAS IN OPEN` → AMSDOS le sert depuis le
   disque, tout va bien ;
2. juste après la fermeture du fichier, le jeu recopie 189 octets en
   `&A800`, ce qui **écrase la zone de travail d'AMSDOS**, dont le bloc
   far address de `&A88B` ;
3. le `CAS IN OPEN` suivant (`WEC.BI2`) fait lire au dispatcher un numéro
   de ROM devenu `0x72` au lieu de `7` ;
4. `CP $10 / JR NC` l'envoie sur la voie « sans ROM » : l'appel n'atteint
   jamais AMSDOS et ne revient jamais à l'appelant ;
5. l'exécution retombe sur l'entrée suivante du jumpblock et dévale toute
   la table jusqu'à `CAS WRITE`, seule entrée non détournée → écriture
   cassette de deux minutes.

Notre ROM AMSDOS est authentique (16 Ko, version 1.0.5, RSX `|CPM`
`|DISC` `|DISC.IN` `|DISC.OUT` `|TAPE` `|TAPE.IN` `|TAPE.OUT` `|A` `|B`
`|DRIVE` `|USER`), donc le placement de sa zone de travail est celui
d'origine.

### Correction : `TRACK0F.BIN` n'existe pas comme fichier séparé

**Erreur de méthode dans la session précédente, à corriger.** Le « catalogue
piste 2 » qui semblait révéler un fichier `TRACK0F.BIN` jamais chargé
était un faux positif : le vrai catalogue AMSDOS n'existe que sur la
**piste 0** (secteurs C1-C4), jamais un par piste. Le scan qui l'a
« trouvé » traitait par erreur le début de chaque piste comme un
catalogue.

Le catalogue réel, complet, ne contient que 3 fichiers :

```
WEC.BI1 (1 extent, 50 records)
WEC.BI2 (3 extents, 128+128+57 records)
WEC.BIN (1 extent, 2 records)
```

Ce qu'on a réellement trouvé : les octets `TRACK0F.BIN` sont le **nom
d'origine embarqué dans l'en-tête AMSDOS du tout premier bloc de
`WEC.BI2`** — exactement comme `WEC.BI1` embarque `WEC.SCR` comme nom
d'origine (déjà relevé plus haut, section « cause immédiate ») :

```
WEC.BI1 (bloc 2, son 1er bloc) : nom d'origine = WEC.SCR   load=&68F1
WEC.BI2 (bloc 9, son 1er bloc) : nom d'origine = TRACK0F.BIN
```

C'est la signature classique d'une **conversion cassette → disquette** :
les fichiers cassette d'origine ont été renommés en
`WEC.BI1`/`WEC.BI2`/`WEC.BIN` pour le catalogue du disque, mais chaque
en-tête AMSDOS a conservé le nom sous lequel il avait été sauvegardé à
l'origine.

**Précision (et correction d'une première hypothèse) : la source réelle
du `LDIR` de `&A73C` a été vérifiée directement, ce n'est pas `WEC.BI2`
mais `WEC.BI1`/`WEC.SCR`.** Capture au moment exact de l'instruction :

```
4,80 s  LDIR : source HL=&69FE  destination DE=&A800  longueur BC=&BD (189)
        &69FE - &68F1 (adresse de chargement declarée de WEC.SCR) = +269
```

`&69FE` tombe très exactement 269 octets après le début du chargement de
`WEC.BI1`/`WEC.SCR` (`&68F1`, la longueur déclarée dans son en-tête). Le
contenu copié n'est ni du texte ni du code reconnaissable — une suite
d'octets qui décroît puis remonte (`F5 F1 E0 D0 B5 B3 91 81 78 6E 4B 42
25 0B 09 07 05 03 01 F4 C8 B8 ...`), plus compatible avec une table
(dégradé, sinus, palette) qu'avec du code exécutable. Cette copie a lieu
**juste après la fermeture de `WEC.BI1`** (4,79 s) et **avant même
l'ouverture de `WEC.BI2`** (6,23 s) : elle n'a donc rien à voir avec la
cascade dans le jumpblock qui, elle, ne se produit que plus tard, sur le
second fichier.

Le fichier `WEC.SCR` d'origine (6169 octets, bien plus petit qu'un écran
mode 1 complet de 16 Ko) contient donc vraisemblablement, en plus des
données d'écran, une ou plusieurs tables annexes que le jeu redistribue
en mémoire après le chargement — exactement le genre d'étape de
post-traitement qu'un chargement cassette réel aurait exécutée à
l'identique. Si `&A800` était déjà la destination sur la version cassette
d'origine, la collision avec le poste de travail AMSDOS serait alors
**inhérente à cette combinaison** (ce jeu + AMSDOS + cette adresse), pas
spécifique à la conversion disquette — et la question redevient : sur un
vrai 6128, qu'est-ce qui empêche normalement cette collision ? (HIMEM
déjà abaissé avant le premier accès disque par un mécanisme qu'on n'a pas
encore identifié, ou bien le poste de travail AMSDOS s'installe
réellement ailleurs sur le vrai matériel.)

### La question à trancher

Le jeu écrit délibérément en `&A800` (adresse en dur), or cette zone
appartient à AMSDOS une fois le système disque initialisé.

**La zone de travail AMSDOS elle-même n'est probablement pas en cause.**
Traçage de son installation : la routine d'installation (ROM AMSDOS,
`&CCA0`) reçoit son adresse de base via le registre `IY`, déjà établi
avant son premier appel (`&BE7D` contenait déjà `&A700` avant même que la
routine ne s'exécute). Elle calcule ensuite tout le reste par arithmétique
relative à `IY` (routine `&CA98` : `DE = IY + DE`, un simple décalage,
aucune négociation HIMEM à cet endroit précis) :

```
IY = &A700                          ; base du poste de travail AMSDOS
IY + &164 = &A864                   ; copie de sauvegarde de &BC77-&BC9D (13 entrées, 39 octets)
IY + &164 + &27 = &A88B             ; bloc "far address" (CAS IN OPEN..CATALOG)
IY + &18B = &A88B                   ; (même adresse, retrouvée par le second calcul)
```

Or `&A700` est l'adresse standard bien documentée du poste de travail
AMSDOS sur un 6128 avec BASIC 1.1 + AMSDOS seuls (HIMEM par défaut
`&A6FF`). Notre valeur colle donc à ce qu'on attendrait d'une vraie
machine — ce n'est probablement pas une négociation HIMEM ratée de notre
côté.

**Confirmation : la négociation est bien dynamique, pas une constante
figée.** Test décisif — ajouter une ROM supplémentaire au démarrage
(mode diagnostic, ROM en slot 15) fait bouger `IY` :

```
sans ROM diagnostic : IY = &A700
avec ROM diagnostic  : IY = &A6FC   (4 octets de moins, HIMEM baisse)
```

Le mécanisme réagit donc correctement au nombre de ROM installées, comme
sur une vraie machine — ce n'est manifestement pas une valeur codée en
dur côté émulateur. Avec la configuration réelle utilisée pour WEC (2
ROM : BASIC 1.1 + AMSDOS, aucune autre), `&A700` reste la valeur la plus
probable. Sans référence matérielle ou Caprice32 fonctionnelle dans cet
environnement pour trancher au bit près, l'analyse statique/dynamique
seule ne permet pas d'aller plus loin sur ce point précis.

**Piste abandonnée : la piste 2 (physique) n'est jamais visitée par le
lecteur sur 200 s émulées** (`{0, 1, 10}` seulement, jamais 2). Cette
observation en elle-même n'est pas fausse, mais l'interprétation qui en
avait été tirée l'était (« `TRACK0F.BIN` est un fichier séparé jamais
chargé ») — voir la correction ci-dessus, ce fichier n'existe pas. La
vraie explication est plus simple : les blocs 9-13 de `WEC.BI2` (son
tout premier morceau) vivent physiquement sur la piste 2, mais la
cascade dans le jumpblock cassette se produit **avant même que le FDC
n'ait besoin d'y chercher quoi que ce soit** — `CAS IN OPEN` s'égare dans
le dispatcher AMSDOS avant d'atteindre l'étape qui interrogerait
réellement le disque pour cette piste. Piste 2 jamais visitée = symptôme
de la cascade, pas une étape de chargement manquante.

## Piège d'instrumentation à connaître

Le trait `Bus` de `../ZilogZ80` fournit des `read_io`/`write_io` **par
défaut qui ne font rien**. En instrumentant `CpcBus`, faire sortir par
mégarde `write_io` du bloc `impl Bus for CpcBus` (par exemple en ouvrant
un `impl CpcBus` trop tôt pour y loger un helper) compile sans le moindre
avertissement, mais **toutes les écritures d'I/O disparaissent en
silence**. Symptôme observé : le balayage clavier du firmware renvoyait
`00` sur les dix lignes (« toutes touches enfoncées »), ce qui ressemblait
beaucoup à un vrai bug d'émulation. Une session entière de mesures a été
invalidée ainsi.

Le test `bus::tests::io_actually_goes_through_the_bus_trait` verrouille
désormais ce comportement en passant délibérément par le trait. Toute
instrumentation future du bus doit commencer par vérifier qu'un
`OUT &F640` atteint bien `ppi.port_c`.

## Comparaison directe avec Caprice32 (décisive)

L'utilisateur dispose d'un checkout Caprice32 compilable localement
(`~/Dev/caprice32`), avec ses ROM par défaut ET les octets `amsdos.rom`
strictement identiques aux nôtres (md5 égaux). Son OS+BASIC par défaut
(`rom/cpc6128.rom`) est en revanche une variante **anglaise**, différente
de notre `OS6128-AZERTY.rom`/`BASIC1-1-AZERTY.ROM` — importante précaution
avant de comparer quoi que ce soit.

Méthode : Caprice32 accepte `--autocmd` et un mode headless
(`SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy`, `-O system.limit_speed=0`
pour tourner à vitesse débridée). Instrumentation ajoutée temporairement
dans `src/z80.cpp` (boucle `z80_execute`, revert après usage — voir
`extern t_z80regs z80`, `membank_read[4]` pour lire la mémoire telle que
vue par le CPU) : impression sur stderr à des adresses précises.

**Piège rencontré, à savoir pour une prochaine tentative :** utiliser
notre propre ROM (via `-O rom.rom_path=...`) pour comparer aux mêmes
adresses casse la frappe automatique — la table `--autocmd`
caractère→matrice clavier de Caprice32 suppose vraisemblablement le ROM
anglais par défaut, et produit une saisie incorrecte contre notre ROM
AZERTY (rien ne se passe, `RUN"WEC` n'est jamais réellement exécuté). Les
comparaisons ci-dessous utilisent donc le ROM anglais PAR DÉFAUT de
Caprice32 (frappe fiable), ce qui rend invalide toute comparaison
d'adresses **dans les ROM OS/BASIC** (elles diffèrent), mais reste
parfaitement valide pour tout ce qui se passe **en RAM utilisateur**
(code du jeu, chargé depuis le même fichier .dsk).

**Confirmé dans l'autre sens** : faire tourner NOTRE émulateur avec les
ROM anglaises de Caprice32 (`~/Dev/caprice32/rom/cpc6128.rom` scindé en
OS+BASIC, `amsdos.rom`) et NOTRE `AutoTyper` produit le symétrique exact
du problème de l'utilisateur avec Caprice32 (qui n'arrivait pas à taper
le guillemet `"`) : notre table clavier AZERTY, interprétée par le ROM
anglais, tape `run3zec` au lieu de `run"wec` (`Syntax error`, capture
d'écran à l'appui) — le guillemet de notre table (Shift+3 en AZERTY)
devient `3` pour le firmware anglais. La correspondance clavier est donc
spécifique à chaque ROM, dans les deux émulateurs : aucun des deux ne
peut taper correctement une commande contre le ROM de l'autre sans une
table dédiée. Confirme qu'il faudra soit une table clavier anglaise pour
notre `AutoTyper`, soit contourner la frappe entièrement (injection
mémoire directe) pour obtenir la comparaison à ROM strictement identique
recommandée ci-dessus.

Résultats :

- **Le `LDIR` corrupteur de `&A73C` est identique au nôtre, à l'octet
  près** : `HL=&69FE DE=&A800 BC=&00BD`, et les 189 octets sources
  copiés se terminent identiquement par `...0C BD 72...` à l'offset
  correspondant à l'équivalent de `&A88B` — la même corruption qu'on a
  documentée plus haut se produit donc à l'identique chez Caprice32 ;
- **Caprice32 n'en reste pas moins pas figé** : l'ouverture suivante de
  `WEC.BI2` (`CAS_IN_OPEN fichier=WEC.BI2`) est franchie, et l'exécution
  continue vers des adresses (`0x2FCD`, `0x2F5F`, `0x2ED1`, `0x2F5D`,
  `0x3535`...) sans aucun rapport avec notre boucle figée
  (`0x2BAE`/`0x2BAF`, `0x42BD`/`0x42EF`) — signe que le jeu progresse
  réellement vers du nouveau code (cohérent avec la capture d'écran de
  l'utilisateur, montrant l'écran d'options/crédits avec le texte
  `BYTES`/`BLOBS`/`BITS`/`BUZZ`/`...HILL.`/`...B.`/`...ONAMI.` qu'on avait
  déjà repéré dans les données de `WEC.BI2`).
- Tentative de comparer précisément le dispatcher far-call lui-même
  (adresses `0xB9C7`-`0xB9E9`, `0xCCA0`) : **invalidée après coup** — ces
  adresses viennent de la désassemblage de notre ROM AZERTY, et avec le
  ROM anglais de ce test, elles ne correspondent pas au même code (un
  `HL` constant et sans rapport à chaque déclenchement l'a révélé). Ne
  pas réutiliser ces adresses précises sans le même ROM.

**Conclusion de cette comparaison : c'est très probablement un vrai bug
d'émulation de notre côté**, pas un défaut de cette disquette ou du jeu.
La même corruption mémoire, produite par le même code de jeu à partir des
mêmes données, n'entraîne pas le même échec chez Caprice32.

## Ce qui reste à faire

- **Priorité** : refaire la comparaison ci-dessus avec un ROM
  **identique au nôtre** des deux côtés, pour localiser l'instruction
  exacte où le dispatcher (`0xB9C7`-`0xB9E9`, notamment le test `CP $10`
  /`JR NC,$B9F9` en `0xB9E6`-`0xB9E8`) diverge entre notre émulateur et
  Caprice32/le vrai matériel. Contourner le piège de frappe : soit
  patcher la table clavier de Caprice32 pour notre AZERTY, soit injecter
  directement les octets `RUN"WEC` + retour dans le tampon de saisie
  BASIC par écriture mémoire plutôt que par simulation de frappe, soit
  utiliser `--inject` avec un petit programme qui saute directement au
  point d'entrée voulu ;
- pistes pour cette instruction exacte : le `CP $10` compare le numéro
  de ROM à 0x10 — sur un vrai 6128/Caprice32, peut-être que la valeur
  lue n'a pas le même sens qu'on le suppose (le test pourrait chercher
  autre chose qu'un simple garde-fou « ROM valide »), ou le chemin
  « sans ROM » (`&B9F9` et suite) fait réellement quelque chose
  d'acceptable qu'on n'a pas tracé jusqu'au bout ;
- l'entrée cassette n'étant jamais lue, la divergence n'est pas une
  question de périphérique sondé : c'est bien le chemin d'exécution.

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

**Comparaison Caprice32**, méthode réutilisable (voir section dédiée
plus haut pour les résultats) :

```bash
cd ~/Dev/caprice32
# ROM identique à la nôtre (nécessaire pour comparer des adresses ROM,
# mais casse la frappe --autocmd — voir le piège documenté plus haut) :
mkdir -p /tmp/wec_roms
cat .../bin/OS6128-AZERTY.rom .../bin/BASIC1-1-AZERTY.ROM > /tmp/wec_roms/cpc6128.rom
cp rom/amsdos.rom rom/MF2.rom /tmp/wec_roms/
make -j$(nproc)   # après avoir ajouté un hook de debug dans z80_execute() (src/z80.cpp)
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy ./cap32 \
  -O system.model=2 -O system.limit_speed=0 \
  [-O rom.rom_path=/tmp/wec_roms pour notre ROM, omis pour le ROM anglais par défaut] \
  -a 'run"wec' chemin/vers/WEC_Le_Mans.dsk 2>&1 | grep WECDBG
```

Hook de debug : dans `z80_execute()` (`src/z80.cpp`, juste avant l'appel
à `z80_execute_instruction()`), tester `_PCdword == <adresse>` et
imprimer `z80.IY.d`/`z80.HL.d`/etc. via `fprintf(stderr, ...)` — accès
mémoire tel que vu par le CPU via `membank_read[addr >> 14][addr &
0x3FFF]` (`extern byte *membank_read[4];`). Toujours `git checkout
src/z80.cpp` après usage : ce n'est pas notre dépôt.
