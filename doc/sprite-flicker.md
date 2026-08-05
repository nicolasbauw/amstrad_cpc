# Les "sprites" qui clignotaient (Cauldron, BMX Simulator)

Note d'enquête. **La cause principale (rendu en instantané unique) est
corrigée.** Un résidu, plus rare, persiste sous BMX Simulator — voir
« Résidu après correctif : l'alternance 5/6 interruptions » plus bas, qui
documente une investigation qui n'a pas abouti à un correctif.

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

## Résidu après correctif : l'alternance 5/6 interruptions

Après le correctif ci-dessus, Cauldron n'affiche plus jamais aucun pixel en
clignotement (118 trames capturées). BMX Simulator, en revanche, en
présente encore, plus rarement : un vrai sprite (un petit drapeau près du
panneau "FINISH") disparaît entièrement pendant une trame, ce qui prouve
qu'il ne s'agit pas cette fois d'un artefact de rendu à moitié dessiné (le
correctif ci-dessus s'applique bien) mais d'un contenu VRAM réellement
différent d'une trame à l'autre.

**Corrélation exacte trouvée par traçage** : la trame qui clignote est
systématiquement celle où `measured_interrupts_per_frame` tombe à **5** au
lieu de 6, alors que la longueur de trame reste rigoureusement stable à 312
lignes à chaque fois. Exemple mesuré : trames 21→22→23, interruptions
6→5→6, le sprite disparaît précisément à la trame 22.

**Mécanisme identifié** (`GateArray::step_hsync`, `gate_array.rs`) : le
compteur d'interruption 6 bits (52 lignes) est recalé sur le VSYNC, et ne
force une interruption supplémentaire à ce recalage QUE si son bit 5 est
déjà armé à cet instant précis — sinon il se contente de se remettre à zéro
sans lever d'interruption. C'est le comportement documenté du vrai Gate
Array CPC (déjà commenté et testé dans ce fichier avant cette enquête) : un
vrai CPC peut légitimement produire 5, 6 ou 7 interruptions selon la phase.
Une expérience de contrôle (compteur Gate Array pur, hors `Machine`) confirme
qu'un délai d'acquittement **fixe** ne fait jamais varier le compte d'une
trame à l'autre — seule une **dérive de phase** peut le faire basculer, et
cette dérive dépend de l'historique d'exécution du CPU (essentiellement,
combien de temps les interruptions restent masquées par endroits).

**Conclusion** : cette variabilité n'est vraisemblablement *pas* un bug de
l'émulateur — c'est un comportement matériel authentique. Le bug est plus
probablement dans BMX Simulator lui-même (une routine d'animation qui
suppose à tort un nombre fixe d'interruptions par trame), un bug qui peut
très bien exister aussi sur un vrai CPC / Caprice32, simplement déclenché
par un chemin d'exécution différent (donc une phase différente) du nôtre.

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
précise à corriger. Cela ne prouve pas l'absence totale d'imprécision
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
