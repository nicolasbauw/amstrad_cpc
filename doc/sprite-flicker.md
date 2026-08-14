# Les "sprites" qui clignotaient (Cauldron, BMX Simulator)

Note d'enquête. **Deux causes ont été trouvées et corrigées** : le rendu en
instantané unique (voir plus bas), puis une règle d'interruption du Gate
Array inversée par rapport à la documentation de référence (voir
« L'alternance 5/6 interruptions : une règle du Gate Array inversée »). Un
résidu subsiste malgré tout sous BMX Simulator, réduit d'environ 18 % mais
pas éliminé.

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

## L'alternance 5/6 interruptions : une règle du Gate Array inversée

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

### La règle inversée

Reste à savoir pourquoi ce décalage n'était jamais rattrapé. Il aurait dû
l'être : c'est précisément la fonction du recalage sur le VSYNC. La
documentation de référence
([cpctech.cpcwiki.de/docs/ints.html](https://cpctech.cpcwiki.de/docs/ints.html),
reprise par CPCWiki) est explicite :

> "If the top bit of the 6-bit counter is set to '1' (i.e. the counter >=32),
> then there is no interrupt request, and the 6-bit counter is reset to '0'.
> If the top bit of the 6-bit counter is set to '0' (i.e. the counter <32),
> then a interrupt request is issued, and the 6-bit counter is reset to '0'."

Autrement dit : à VSYNC + 2, une interruption est levée quand le bit 5 est à
**zéro**. Notre code faisait exactement l'inverse (`!= 0` au lieu de `== 0`),
et les deux tests unitaires qui couvraient ce point encodaient eux aussi la
règle inversée — écrits pour correspondre à l'implémentation plutôt qu'à la
spécification, le piège classique.

Avec la règle corrigée, le recalage rattrape la dérive : le compte
d'interruptions redevient **rigoureusement stable à 6 par trame** pendant
toute la course démo (mesuré : plus aucune trame hors-6, alors que les
décalages de phase, eux, continuent de se produire — c'est bien le recalage
qui les compense).

Effet de bord à connaître : la toute première trame après la mise sous
tension n'a plus d'interruption pendant son VSYNC, le temps que le recalage
aligne le compteur. Un vrai CPC a le même transitoire, et le firmware attend
simplement une trame de plus avant son premier commit INK/BORDER. Le test
`an_interrupt_lands_during_vsync_on_every_frame` (crtc.rs) exclut donc
explicitement cette trame de chauffe.

### Ce que ça règle, et ce que ça ne règle pas

Mesuré sur 398 trames capturées dès le départ de la course démo, en
comptant les pixels qui oscillent A→B→A d'une trame à l'autre (bandeau de
score exclu : le chronomètre y change normalement à chaque trame, ce n'est
pas le bug cherché) :

| | pixels oscillants | pire trame |
|---|---|---|
| Règle inversée (avant) | 3112 | 672 |
| Règle conforme (après) | **2544** | 544 |

Une réduction d'environ 18 %, dans la zone même du panneau START/FINISH
(x 604..671, y 364..401) où le drapeau signalé se trouve — mais **pas une
disparition**. Le correctif est juste et vaut pour lui-même (c'est un écart
avéré à la documentation matérielle, qui affecte potentiellement tout
logiciel sensible au rythme des interruptions), mais il ne suffit pas à
expliquer tout le symptôme visible.

Piste restante, non explorée : le détecteur A→B→A ne sait pas distinguer un
bug d'une **animation légitime sur deux trames** (un drapeau qui flotte en
alternant deux images produit exactement ce motif). Une partie du résidu
mesuré est donc peut-être parfaitement normale. Trancher demanderait
d'inspecter visuellement les trames en cause, ou de désassembler la routine
d'animation du jeu.

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

**Aucune anomalie trouvée.** Cet audit n'a pas identifié d'instruction
précise à corriger — et il regardait au mauvais endroit : la cause réelle de
l'alternance 5/6 n'était pas une imprécision de cycles, mais la règle
d'interruption inversée décrite plus haut. Cela ne prouve pas l'absence totale d'imprécision
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
