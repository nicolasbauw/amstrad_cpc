# Les "sprites" qui clignotaient (Cauldron, BMX Simulator)

Note d'enquête. **RÉSOLU.** Deux causes, toutes deux corrigées : le rendu en
instantané unique (voir plus bas), puis un CPU émulé environ 5 % trop rapide,
qui décalait la routine de tracé du jeu par rapport au faisceau. Le
clignotement de fin de course est passé de 6840 pixels oscillants à **zéro**,
verrouillé par le test `bmx_simulator_finish_line_sprite_does_not_flicker`.

Une fausse piste a été suivie puis **annulée** en cours de route : aligner
le recalage VSYNC du Gate Array sur la lecture littérale de la
documentation. La comparaison avec Caprice32 a montré que ce changement
nous éloignait de l'émulateur de référence sans rien régler. Détail dans
« La règle du recalage VSYNC : documentation contre émulateur de référence »
— l'épisode est conservé ici parce que la contradiction entre la
documentation et l'implémentation de référence vaut d'être connue.

## Le symptôme

Sous Cauldron, tout ce qui est animé (chauve-souris, personnage du joueur)
clignote très rapidement. Sous BMX Simulator, seul le second joueur de la
démo (déclenchée après un temps d'inactivité sur l'écran d'accueil) est
concerné. Absent sur l'émulateur de référence (Caprice32), présent aussi en
build release.

## Reproduction automatisée

Un harnais (boot du jeu via `--autocmd`, capture d'une trame rendue par
VSYNC comme le fait `main.rs`, détection des pixels qui oscillent A→B→A
d'une trame sur l'autre) a permis de capturer le phénomène de façon fiable
et de sauver les trames en cause en PPM pour inspection visuelle. Sous
Cauldron, le sprite du joueur (la sorcière) disparaissait entièrement
pendant exactement une trame avant de réapparaître à l'identique.

## Cause

`video::render` ne prenait qu'**un seul instantané de toute la VRAM**, à
l'instant précis du VSYNC, pour peindre la trame entière. Un vrai tube
cathodique peint chaque ligne avec le contenu de la VRAM tel qu'il est
exactement à l'instant où le faisceau la balaie — jamais un état figé
global.

Or le tracé de sprite du CPC se fait classiquement par XOR, en boucle
(confirmé par traçage pas à pas : routine à `0xA353` chez Cauldron, deux
boucles imbriquées `LD A,(DE) / XOR (HL) / LD (HL),A`), et cette boucle ne
masque pas les interruptions. Quand l'interruption vidéo tombe pendant son
exécution — ce qui est normal et attendu — puis que l'exécution reprend,
la boucle peut se retrouver à moitié terminée pile au moment où
`video::render` prenait son instantané global : un octet sur deux du
sprite déjà XORé, l'autre pas encore. D'une trame à l'autre, ce
chevauchement dépend de la position exacte de cette boucle par rapport au
VSYNC, qui dérive légèrement — d'où un clignotement rare, irrégulier et
propre aux zones tracées ainsi.

## Correctif

`Machine::scanline_vram` (un `Vec<Vec<u8>>`, un slot par scanline de la
trame) mémorise désormais les octets de VRAM affichés sur chaque scanline
à l'instant même où le CRTC la balaie (`video::capture_scanline_vram`,
appelée depuis `Machine::step` juste après `Crtc::step_scanline`) — sur le
même principe que `scanline_states`, qui fait déjà ça pour l'état du Gate
Array (mode vidéo, palette) afin de gérer les ruptures d'écran.
`video::render` lit ces octets capturés en priorité, avec repli sur une
lecture directe de la VRAM si la ligne n'a pas été capturée (bordure,
tout premier appel).

Résultat : la trame rendue reflète toujours ce qu'un vrai tube cathodique
aurait peint, quelle que soit la précision cycle-à-cycle du CPU émulé au
moment considéré. Vérifié par le harnais de reproduction : plus aucun
pixel en clignotement sur 118 trames (Cauldron) ; le résidu observé sous
BMX Simulator (quelques pixels, zone du bandeau de score) s'est avéré être
la mise à jour normale des chiffres du chronomètre, pas le bug signalé.

## L'alternance 5/6 interruptions : la règle du recalage VSYNC

Après le correctif ci-dessus, Cauldron n'affiche plus jamais aucun pixel en
clignotement (118 trames capturées). BMX Simulator, en revanche, en
présentait encore, plus rarement : un vrai sprite (un petit drapeau près du
panneau "FINISH") disparaît entièrement pendant une trame, ce qui prouve
qu'il ne s'agit pas cette fois d'un artefact de rendu à moitié dessiné mais
d'un contenu VRAM réellement différent d'une trame à l'autre.

**Corrélation exacte trouvée par traçage** : la trame qui clignote est
systématiquement celle où `measured_interrupts_per_frame` tombe à **5** au
lieu de 6, alors que la longueur de trame reste rigoureusement stable à 312
lignes.

### Pourquoi 5 alors que 312 = 6 x 52 exactement

Le compteur du Gate Array est recalé à zéro à chaque VSYNC + 2 lignes.
Entre deux recalages il s'écoule exactement 312 lignes, soit exactement six
périodes de 52 : le compte devrait donc être rigoureusement stable à 6, pour
toujours. Une seule chose peut décaler cette phase : `acknowledge_interrupt`,
qui efface le bit 5 du compteur (`&= 0x1F`). Si le compteur vaut 32 ou plus
à cet instant, cet effacement le **recule de 32 lignes**, et l'interruption
suivante arrive d'autant plus tard — jusqu'à en faire manquer une sur la
trame.

Mesuré sur la course démo de BMX Simulator (harnais headless, ~70 s de temps
émulé pour l'atteindre) : **17 à 22 trames par seconde sur 50** avaient un
compte différent de 6, et le nombre de décalages de phase mesurés
correspondait exactement, une pour une. La cause est donc établie, pas
seulement corrélée.

La raison pour laquelle ces acquittements arrivent si tard : le jeu masque
les interruptions pendant **145 lignes** (~9,3 ms, presque la moitié d'une
trame) dans sa routine de tracé principale (`PC` 0x9831 → 0x9D6D, parcourant
0x504F..0xA275). Une interruption peut donc rester en attente jusqu'à 110
lignes avant d'être acceptée — bien au-delà des 32 lignes qui déclenchent
l'effacement du bit 5.

### La règle du recalage VSYNC : documentation contre émulateur de référence

Reste à savoir pourquoi ce décalage n'était jamais rattrapé. Il aurait dû
l'être : c'est précisément la fonction du recalage sur le VSYNC. La
documentation de référence
([cpctech](https://cpctech.cpcwiki.de/docs/ints.html), reprise mot pour mot
par [Grimware](https://www.grimware.org/doku.php/documentations/devices/gatearray)
— ce n'est donc probablement qu'une seule source, recopiée) dit :

> "If the top bit of the 6-bit counter is set to "1" (i.e. the counter >=32),
> then there is no interrupt request, and the 6-bit counter is reset to "0".
> (If a interrupt was requested and acknowledged it would be closer than 32
> HSYNCs compared to the position of the previous interrupt).
> If the top bit of the 6-bit counter is set to "0" (i.e. the counter <32),
> then a interrupt request is issued, and the 6-bit counter is reset to "0"."

Notre code fait l'inverse : interruption quand le bit 5 est à 1. Il a été
aligné un temps sur cette lecture littérale, puis **remis comme avant**
(commit d'origine annulé). Trois éléments justifient ce retour en arrière,
découverts en comparant ensuite avec l'émulateur de référence :

1. **Caprice32 fait l'inverse de la documentation**, et donc comme notre
   code d'origine (`src/crtc.cpp`) :

   ```c
   if (GateArray.sl_count >= 32 && CRTC.interrupt_sl == 0) { // counter above save margin?
      z80.int_pending = 1; // queue interrupt
   }
   GateArray.sl_count = 0; // clear counter
   ```

   Son commentaire — *"counter above save margin?"* — dit bien l'intention :
   on ne lève l'interruption que si la précédente est assez loin.

2. **La documentation se contredit elle-même.** Sa parenthèse justifie le
   cas « pas d'interruption » par *"it would be closer than 32 HSYNCs
   compared to the position of the previous interrupt"*. Or si le compteur
   vaut ≥32, la précédente remise à zéro date de ≥32 lignes : lever une
   interruption ne serait justement PAS trop proche. Cette justification
   décrit le cas inverse de celui auquel elle est attachée — les deux
   conditions semblent avoir été interverties à la rédaction.

3. **Notre code d'origine avait été écrit pour corriger un vrai bug
   observé** (INK/BORDER jamais committés, faute d'interruption pendant le
   VSYNC — voir le commentaire du test `interrupt_fires_two_lines_after_vsync_start`).
   Avec la règle « corrigée », la toute première trame après mise sous
   tension perd son interruption pendant le VSYNC, ce qui a obligé à
   assouplir le test `an_interrupt_lands_during_vsync_on_every_frame` : un
   signal qui aurait dû alerter.

Et surtout : **ce changement n'avait pas réglé le clignotement** (18 % de
réduction sur le phénomène intermittent, et aucune amélioration sur le
clignotement permanent de fin de course, cf. plus bas). Il nous éloignait
de l'émulateur de référence sans rien apporter : d'où son annulation.

### Ce que ça changeait (et pourquoi ça ne suffisait pas)

Avec la règle littérale de la documentation, le compte d'interruptions par
trame devenait rigoureusement stable à 6 pendant la course démo (contre 17 à
22 trames hors-6 par seconde avant). Mais le clignotement visible ne
disparaît pas pour autant : 3112 → 2544 pixels oscillants sur 398 trames,
soit 18 %, et rien du tout sur le clignotement permanent de fin de course
décrit ci-dessous. Un compte stable n'est donc pas le bon critère — et vu
que Caprice32 obtient un affichage correct **sans** cette stabilité, ce
n'est vraisemblablement pas ce que fait le vrai matériel.

## Le clignotement de fin de course : le tracé qui court après le faisceau

Mesuré sur cette scène (harnais headless, 58 trames capturées) : environ
118 pixels oscillent à chaque trame, dans une boîte de 40 x 22 pixels
(x 436..475, y 372..393) — la taille d'un sprite, et deux captures
consécutives montrent bien le coureur **présent puis totalement absent**.

Le mécanisme, lui, se lit dans la position de la routine de tracé du jeu
(sa longue section DI) par rapport au faisceau :

| | scanlines où tourne le tracé | pixels oscillants |
|---|---|---|
| Code actuel | 111→254, 143→278, 163→294 (cycle de 3) | 6840 sur 58 trames |
| Sans l'effacement du bit 5 | **163→296 à chaque trame** | **0** |

Le sprite occupe les scanlines ~136 à 146. Quand le tracé démarre à 163, le
faisceau a déjà dépassé cette zone : le sprite est effacé puis redessiné
hors champ, invisible. Quand il démarre à 111 ou 143, le faisceau traverse
la zone pendant que le sprite est effacé — il disparaît pour cette trame.

L'écart entre ces positions de départ (32 lignes) est exactement celui de
l'effacement du bit 5 par `acknowledge_interrupt`. Comme le jeu masque les
interruptions 145 lignes d'affilée, ses acquittements sont très tardifs, le
compteur vaut alors 33 à 51, et chaque effacement le recule de 32 lignes :
la boucle principale du jeu, cadencée sur ces interruptions, démarre son
tracé à une position de faisceau différente d'une trame à l'autre.

**Mais ce n'est pas là qu'est notre écart avec le matériel réel.** Trois
vérifications convergent :

- l'effacement du bit 5 à l'acquittement est bien documenté
  ([cpctech](https://cpctech.cpcwiki.de/docs/ints.html) : *"The top bit
  (bit 5), of the counter is set to '0' and the interrupt request is
  cleared."*) ;
- notre détection de l'acquittement est exacte : la crate `zilog_z80` ne
  vide sa requête (`self.int`) que lorsqu'elle accepte réellement
  l'interruption, jamais quand elle l'ignore faute d'IFF1 ;
- **Caprice32 implémente la même règle** (`GateArray.sl_count &= 0x1f` dans
  son `z80.c`) et ne montre pourtant pas le clignotement.

Retirer cet effacement supprimerait bien le symptôme (0 pixel oscillant,
tracé verrouillé sur la scanline 163), mais masquerait un écart de phase
dont la cause est ailleurs — c'est pourquoi ça n'a pas été fait.

### Comparaison instrumentée avec Caprice32

Caprice32 étant installé en local avec ses sources (`~/Dev/caprice32`), il a
été instrumenté temporairement (compteur de microsecondes, relevé des
sections DI, des interruptions par trame, des effacements du bit 5) puis
**restauré à son état d'origine**. Les deux émulateurs ont joué la même
scène (`--autocmd='RUN"BMXSIM'`, course démo, t ≈ 100-110 s).

| Mesure | Caprice32 | ByteBox |
|---|---|---|
| Durée de la section DI de tracé | 138 à 151 lignes | ~145 lignes |
| Départ du tracé (lignes après VSYNC) | 207 à 229 (bande de ~22) | 183 / 215 / 235 (3 positions, écart 52) |
| Interruptions par trame | **6, toujours** | variable (17 à 22 trames hors-6 par seconde) |
| Effacements du bit 5 à l'acquittement | **0** | des centaines |
| Grille d'interruptions | 1.9, 53.9, 105.7, 156.1, 208.0, 261.2 lignes après VSYNC | même grille *quand* aucun décalage n'a lieu |

Deux enseignements nets :

1. **Notre timing CPU n'est pas en cause.** La durée de la section DI du jeu
   — ~145 lignes chez nous — tombe en plein dans la fourchette mesurée chez
   Caprice32 (138-151). L'hypothèse d'un CPU émulé trop lent ou trop rapide,
   qui motivait l'audit des tables de cycles, est écartée.

2. **Toute la différence tient aux effacements du bit 5.** Caprice32 n'en
   fait **aucun** pendant toute la course : ses acquittements tombent
   systématiquement à moins de 32 lignes de l'interruption qui les a
   déclenchés, donc le compteur n'est jamais rogné, la phase ne dérive
   jamais, la grille reste figée et le tracé démarre toujours dans la même
   bande de ~22 lignes. Chez nous, des acquittements tombent régulièrement
   au-delà de 32 lignes, chaque effacement recule le compteur de 32 lignes,
   et le tracé se met à sauter entre trois positions distantes d'une période
   d'interruption complète.

La question à instruire n'est donc plus « la règle du bit 5 est-elle
juste ? » (elle est identique des deux côtés) mais : **pourquoi nos
acquittements arrivent-ils plus tard, relativement à l'interruption qui les
provoque, que ceux de Caprice32 ?** Un décalage systématique d'une
dizaine de lignes suffit à faire basculer d'un régime à l'autre — et une
fois du bon côté, le système est stable (pas d'effacement, donc pas de
dérive, donc toujours pas d'effacement).

Pistes concrètes pour la suite, dans l'ordre :

- instrumenter ByteBox exactement comme Caprice32 l'a été (position de
  chaque interruption dans la trame, valeur du compteur à chaque
  acquittement) et comparer les deux relevés ligne à ligne sur la même
  scène ;
- vérifier en particulier le délai entre la demande d'interruption par le
  Gate Array et son acceptation par le CPU : c'est là que se logerait un
  décalage systématique (moment exact où l'interruption est présentée, et
  traitement du délai d'une instruction après `EI`) ;
- vérifier aussi la position de la première interruption après le recalage
  VSYNC : la grille de Caprice32 démarre à 1.9 ligne après le début du
  VSYNC, valeur à confronter à la nôtre.

### La cause : le Gate Array étire chaque cycle machine, pas l'instruction

La comparaison instrumentée ci-dessus disait que nos sections DI duraient
~145 lignes contre 138-151 chez Caprice32 : même fourchette, donc CPU hors
de cause. C'était une lecture trop rapide. En relevant la durée **par
trame** plutôt que le maximum par seconde, le motif à trois valeurs apparaît
des deux côtés, et l'écart devient systématique :

| | valeur 1 | valeur 2 | valeur 3 |
|---|---|---|---|
| Nous (avant) | 131,4 | 135,7 | 143,8 |
| Caprice32 | 138,1 | 142,7 | 150,5 |

Environ 7 lignes de moins à chaque fois, soit ~5 % trop rapide. Assez pour
que le tracé démarre à la scanline 175 au lieu de 209 et rattrape le
faisceau au niveau du sprite.

**Pourquoi.** Le Gate Array n'ouvre au Z80 qu'une fenêtre d'accès mémoire par
microseconde : chaque **cycle machine** est étiré à 4 cycles d'horloge. Nous
arrondissions la durée **totale** de l'instruction, ce qui n'est équivalent
que si le découpage tombe juste. `PUSH BC` en est l'exemple type : 11 cycles
nominaux, dont un M1 de 5, soit 8 + 4 + 4 = **16** sur CPC — alors
qu'arrondir 11 donne 12. Un cycle machine entier de retard, sur une
instruction omniprésente dans une routine de tracé.

La comparaison opcode par opcode avec les tables de temps CPC de Caprice32
(`src/z80.cpp` : `cc_op`, `cc_ed`, `cc_xy`) donne exactement **51 opcodes**
sous-estimés, tous de 4 cycles :

- `PUSH rr` et `RST n` (M1 allongé) ;
- `LD (nn),HL`, `LD HL,(nn)`, `EX (SP),HL` et leurs formes indexées ;
- côté ED : `IN r,(C)`, `OUT (C),r`, `LD (nn),dd`, `LD dd,(nn)`, ainsi que
  `LDI`/`LDD`/`CPI`/`CPD` (donc chaque itération des formes répétitives) ;
- rien sur les familles CB et DDCB/FDCB.

C'est `Machine::cpc_mcycle_extra` qui rétablit ce complément, opcode par
opcode. Confirmation par la mesure : nos sections DI passent à 136 / 140 /
148 lignes démarrant aux scanlines 209 et 229, soit la fourchette de
Caprice32 (138-151, départs 207-229), et le clignotement tombe à zéro.

Au passage, la même comparaison a révélé un vrai bug dans la table de la
crate `zilog_z80` : `SET 1,(HL)`, `SET 3,(HL)`, `SET 5,(HL)` et `SET 7,(HL)`
étaient à 8 cycles au lieu de 15 (les quatre autres `SET b,(HL)` et tous les
`RES b,(HL)` étaient corrects). Corrigé côté crate.

### Le correctif de Cauldron reste-t-il pertinent ?

Question légitime une fois la cause de BMX trouvée : le clignotement de
Cauldron venait-il lui aussi du CPU trop rapide, auquel cas la capture de la
VRAM ligne par ligne (voir « Correctif » plus haut) ne serait qu'un
pansement ?

Mesuré, plutôt que raisonné. Cauldron lancé jusqu'en jeu (chauves-souris et
sorcière animées), 58 trames comparées, avec le CPU désormais correct :

| | pixels oscillants |
|---|---|
| Capture ligne par ligne (code actuel) | **0** |
| Capture désactivée (modèle d'avant le correctif) | **8488** |

Le correctif de Cauldron reste donc indispensable : la correction du CPU ne
le rend pas caduc, les deux causes étaient bien distinctes.

Et il ne nuit pas à la fidélité — c'est l'inverse. Un tube cathodique peint
chaque ligne avec le contenu de la VRAM tel qu'il est à l'instant où le
faisceau la balaie ; prendre un instantané unique pour toute la trame est
l'approximation, pas le contraire. La capture ligne par ligne est donc le
modèle juste, et il le restera quelle que soit la précision du CPU.

Il reste d'ailleurs perfectible dans le même sens : nous capturons les
octets d'une ligne au moment où elle *commence* à être balayée, alors que le
CRTC les lit au fil de la ligne, deux octets par position de caractère. Une
écriture survenant en milieu de ligne n'est donc pas reflétée sur sa moitié
droite. Per-ligne est déjà bien plus fidèle que per-trame ; per-caractère le
serait davantage.

### Effet de bord : le copieur de Discology a dû être recalé

Discology chronomètre le contrôleur de disquette en comptant ses
interrogations du registre d'état, dans une boucle qui contient un
`IN A,(C)` — précisément l'un des opcodes corrigés (12 → 16 cycles). Son
budget de relevé a donc changé, et la constante empirique
`SECTOR_OVERHEAD_BYTES` (`fdc.rs`) a dû être réajustée de 62 à 100 pour que
la copie reste fidèle.

Cette constante reste fragile, et le commentaire qui l'accompagne le dit
franchement : le comportement n'est pas monotone (96 et 100 conviennent,
92, 98, 104 et 144 non, 108 et 130 si), et la valeur physiquement exacte du
format AMSDOS (144) ne convient pas, parce que le relevé de Discology ne
dispose que d'environ 0,92 tour de disquette. Un modèle de rotation plus
fidèle — position angulaire réelle de chaque secteur — rendrait ce réglage
inutile.

### Audit des tables de cycles Z80 (`zilog_z80`)

Pour vérifier que ce n'est pas *notre* imprécision cycle-à-cycle qui nous
fait atterrir sur une phase que le vrai matériel évite, les catégories
d'instructions les plus à risque ont été vérifiées à la main contre les
tables Zilog de référence, dans `zilog_z80/src/cycles.rs` et
`zilog_z80/src/cpu.rs` :

- `CYCLES` (table de base, 256 opcodes) : entièrement conforme, y compris
  les marqueurs à 0 pour les instructions à durée variable (JR cc, DJNZ,
  CALL cc, RET cc), correctement complétés dans `execute_1byte`
  (`cycles += 6/7` quand la condition est prise, aux bons endroits).
- `CYCLES_ED` (bloc/E-S préfixés ED) : conforme, y compris IN/OUT r,(C),
  ADC/SBC HL,rr, LD (nn),rr / LD rr,(nn), RRD/RLD, et les formes à
  répétition (LDIR/CPIR/INIR/OTIR, base 21, ramenée à 16 par
  `repeat_block` sur la dernière itération).
- `CYCLES_DD_FD` (formes indexées IX/IY) : conforme, y compris les formes à
  déplacement `(IX+d)`/`(IY+d)` (19 pour un accès simple, 23 pour
  INC/DEC).
- Le délai d'un instruction après `EI` (`ei_instr_delay`) désactive
  correctement les interruptions pendant exactement l'instruction qui suit
  `EI`, conformément à la spécification Z80.

Le fichier `zilog_z80/src/test.rs` contient par ailleurs déjà ~938
assertions portant sur des durées de cycles, dont une table de référence
dédiée (`REFERENCE_TIMINGS`, 33 entrées) qui couvre précisément ces
catégories à risque.

**Aucune anomalie trouvée** — et pour cause : cet audit vérifiait les durées
**nominales Zilog**, qui étaient justes. L'erreur était dans la conversion
vers le temps CPC (arrondi du total au lieu de chaque cycle machine), une
étape que l'audit ne couvrait pas. Voir « La cause » plus haut. Cela ne prouve pas l'absence totale d'imprécision
(seules les catégories les plus suspectes ont été vérifiées, pas les 1024
entrées une par une, et le CRTC n'a pas été réaudité), mais faute d'indice
concret pointant vers une instruction particulière, creuser plus loin
demanderait de désassembler BMX Simulator pour savoir exactement quel code
tourne autour de la trame qui clignote (à la manière de
`doc/barbarian-demo.md`) — une investigation à part entière, non entamée
ici.

## Harnais de diagnostic

Le code qui a servi à ces deux investigations (tests `investigate_*` dans
`src/machine.rs`, `video::addresses_in_rect`, un compteur Gate Array de
contrôle) a été retiré après chaque session. Méthodes réutilisables si un
symptôme similaire réapparaît :

- booter via `--autocmd`, capturer les trames rendues, détecter les
  oscillations A→B→A entre trames consécutives ;
- poser des watchpoints ciblés (géométrie CRTC → adresses VRAM d'un
  rectangle de pixels) pour tracer les écritures en cause ;
- journaliser chaque demande/acquittement d'interruption directement aux
  deux points de `Machine::step` qui les traitent (pas par détection de
  transition externe, qui peut en perdre si une requête et un acquittement
  tombent dans le même appel à `step`), pour corréler avec
  `measured_interrupts_per_frame`.
