# Le clavier Mac AZERTY vers la matrice CPC : `<`, `>`, `$/*`, et le mystère du `@`

Cette investigation a traversé plusieurs bugs empilés, découverts un par un
sur clavier réel. Elle mérite d'être consignée : chaque symptôme avait l'air
d'un problème différent, et ne s'est révélé qu'en creusant sous le précédent.

## Le point de départ

Le TODO signalait trois défauts sur le clavier Mac AZERTY :
- `SHIFT+@` donnait `>` au lieu de `#`.
- Les touches `<` et `>` du CPC (pourtant bien réelles, voir photo de
  référence du clavier) n'étaient pas mappées du tout.
- `SHIFT+$` donnait `à` au lieu de `*`.

Trois touches sont en cause, chacune avec deux caractères Mac différents
selon SHIFT, dont les cibles CPC ne sont pas nécessairement "l'une la
variante shiftée de l'autre" :

| Touche Mac (ISO) | Sans SHIFT | Avec SHIFT | Cible CPC sans SHIFT | Cible CPC avec SHIFT |
|---|---|---|---|---|
| `# / @` (Scancode::Grave) | `#` | `@` | `#` (2,3) | `#` (2,3) — **le même** |
| `$ / * / €` (Scancode::RightBracket) | `$` | `*` | `$` (2,6) | `*` (2,1) |
| `< / >` (Scancode::NonUsBackslash) | `<` | `>` | `<` du CPC (2,1)+SHIFT CPC | `>` du CPC (2,3)+SHIFT CPC |

Point commun aux trois : le SHIFT du *Mac* ne doit jamais être confondu avec
le SHIFT du *CPC*. Ce sont deux systèmes de touches différents, et leurs
correspondances ne s'alignent pas naturellement.

## Bug 1 — Keycode macOS non fiable

`Keycode::Less` et `Keycode::Greater` (fournis par SDL2 selon la disposition
clavier active) ne varient **pas** avec l'état réel de SHIFT sur macOS : SDL
rapporte le même Keycode que SHIFT soit enfoncé ou non pour cette touche ISO
— défaut déjà connu et déjà contourné pour `#/@` et `$/*/€` avant cette
investigation (voir `Scancode::Grave` et `Scancode::RightBracket` dans
`psg.rs`, qui utilisent déjà le `Scancode`, insensible à la disposition
active, plutôt que le `Keycode`).

**Correctif** : router `< / >` par `Scancode::NonUsBackslash` (position
physique, pas caractère traduit), avec `shift_held` (lu depuis `keymod` de
l'événement SDL) pour choisir la cible CPC.

## Bug 2 — Le saut simultané de deux bits confond le firmware

Une fois `#/@`, `$/*/€` et `</>` routés par Scancode, le SHIFT du Mac devait
être découplé du SHIFT du CPC : par exemple, `SHIFT+@` doit engager la
position `#` (2,3) du CPC **sans** engager son SHIFT — sinon la combinaison
donne la variante shiftée du `#`, c'est-à-dire `>`.

Le correctif initial forçait donc le bit SHIFT du CPC (relâché ou engagé,
selon le cas) **dans le même appel** que celui qui posait la position. Testé
sur clavier réel : `SHIFT+$` donnait *systématiquement* `à` au lieu de `*`,
pas seulement au premier essai.

### Le diagnostic, par capture de la matrice en direct

Une instrumentation temporaire (`KEYLOG=1`, retirée depuis) a tracé chaque
lecture de la ligne 2 de la matrice par le firmware, autour d'un appui
SHIFT+$. Extrait révélateur :

```
[scan ligne 2] = 0b11011111   ; SHIFT seul, 21 lectures stables
KeyDown RightBracket, shift_held=true
[scan ligne 2] = 0b11111101   ; "*" pose, SHIFT relache -- 8 lectures stables
```

Le bit de position et le bit SHIFT changent **dans la même scrutation** :
0b11011111 → 0b11111101 en un seul saut, deux bits différents à la fois. Sur
un vrai clavier, ça n'arrive jamais : même en tapant vite, il y a toujours
quelques cycles où un seul doigt a bougé avant l'autre. L'anti-rebond du
firmware, présenté avec cette transition "impossible", décodait mal la toute
première occurrence de la scrutation ambiguë.

**Correctif** (`Psg::deferred`, dans `psg.rs`) : étaler les deux écritures
sur deux scrutations distinctes. Le bit SHIFT (relâché ou synthétisé) est
posé immédiatement — c'est une transition d'un seul bit, sûre — et la
position est posée `DEFER_TICKS` (~10 ms, plusieurs interruptions clavier du
CPC à 300 Hz) plus tard, le temps qu'une scrutation propre intervienne entre
les deux.

## Bug 3 — Deux inversions de polarité

Une fois le mécanisme de délai en place, deux nouveaux symptômes sont
apparus à l'usage prolongé (relâcher une touche, puis en presser une autre) :

- **`$` redevenait `à` après un premier `SHIFT+$`.** Cause : le relâchement
  de `RightBracket`/`Grave` appelait `set_bit_now(2, 5, true)` — `true`
  signifiant *pressé* dans cette fonction, donc ce relâchement **enfonçait**
  le SHIFT du CPC au lieu de le relâcher. Inversion de polarité pure et
  simple (`true`/`false` inversés), qui laissait le SHIFT du CPC coincé
  enfoncé pour la frappe suivante.
- **`>` redevenait `#` après un premier `SHIFT+>`, tant que SHIFT restait
  physiquement tenu.** Cause différente : pour `>`, le SHIFT du CPC n'est
  *pas* synthétisé — il s'appuie sur celui déjà posé par la touche SHIFT
  elle-même, toujours tenue. Mais le relâchement de `>` relâchait quand même
  le SHIFT du CPC sans condition, désynchronisant le bit CPC de l'état réel
  du SHIFT physique encore enfoncé. L'appui suivant sur `>` en héritait.

**Correctifs** : les trois `set_bit_now(2, 5, ...)` de relâchement corrigés
(`false`, pas `true`). Pour `>`, `less_greater_target` retient désormais
aussi si *cette touche* a synthétisé le SHIFT du CPC (cas `<`) ou s'est
appuyée sur celui déjà là (cas `>`) : le relâchement ne touche au bit SHIFT
que dans le premier cas.

Chacun des trois bugs de cette section a un test de régression dans
`psg.rs` qui échoue sans son correctif (vérifié explicitement en réappliquant
temporairement le bug avant de conclure).

## Le `@` : le caractère existe, c'est le dessin qui a changé

Une fois `<`, `>` et `SHIFT+$` fiables, restait une dernière anomalie
apparente : rien à l'écran ne ressemble jamais à un `@`. La légende du
clavier CPC réel place pourtant `@` en variante SHIFTée de `$`, à la
position `(2,6)` de la matrice.

### Ce que montre l'écran

Testé position par position en pilotant la matrice CPC directement (sans
passer par la traduction clavier Mac, pour écarter toute suspicion côté
SDL) :

- `(2,6)` avec SHIFT CPC posé proprement : **`à`**, pas `@`.
- Toutes les autres positions à caractère imprimable des lignes 2 à 8, y
  compris les variantes SHIFT jamais vérifiées (`^` mort, `-`, `)`, `,`,
  `;`) : aucune ne donne `@`. (Trouvaille au passage : le `^` shifté donne
  aussi `ù`, doublon avec sa position déjà connue.)
- Le pavé "calculatrice" `f0`-`f9` / `.` / flèches (lignes 0 et 1) avec
  SHIFT : toujours les mêmes chiffres.
- Balayage complet de la police en BASIC, sur clavier réel, scruté
  plusieurs fois :
  ```basic
  10 FOR i=32 TO 255
  20 PRINT i;CHR$(i);" ";
  30 IF (i-31) MOD 8=0 THEN PRINT
  40 IF (i-31) MOD 96=0 THEN PRINT "SUITE=touche":WHILE INKEY$="":WEND
  50 NEXT i
  ```
  Aucun `@` nulle part dans les 224 caractères affichables.

Confirmé enfin en extrayant directement la police de la ROM (table de 256
glyphes de 8 octets, à partir de `0x3800` dans la ROM basse — offset
vérifié en rendant `A`, `B` et `0`) : le dessin du `@` est absent des 256
entrées. `bin/OS6128-AZERTY.rom` et `bin/cpc6128.rom` ont tous deux `à` au
code 64.

### Mais la touche n'est pas vide de sens

Il était tentant d'en conclure que `@` était inatteignable. C'est faux, et
l'objection qui l'a fait remarquer était la bonne : Amstrad n'aurait pas
gravé une touche ne correspondant à rien. La bonne question n'était pas
« quel dessin apparaît ? » mais « quel **code** la touche émet-elle ? » :

```basic
PRINT ASC("<SHIFT+$>")
```

Réponse de la ROM : **64**.

La touche émet donc bien le code 64, c'est-à-dire le caractère `@` de
l'ASCII. Ce que la ROM française a changé, c'est uniquement le **glyphe**
associé à ce code : elle y dessine `à`, pour loger les lettres accentuées
(`à`, `ç`...) sans agrandir une table de taille fixe. C'est le principe des
variantes nationales ISO 646 : mêmes codes, dessins différents selon le
pays (on retrouve d'ailleurs `ç` au code 92, celui du `\` en ASCII — ce qui
explique le second symbole de la légende `@ \ $` de cette touche).

Conséquence pratique : `@` **est** accessible et parfaitement fonctionnel.
Un programme qui écrit ce caractère dans un fichier, l'envoie à une
imprimante ou le compare à `CHR$(64)` manipule bien un `@`. Seul son rendu
à l'écran diffère. La légende gravée sur le clavier décrit le code émis
(sens international), pas le dessin qu'en fait le firmware français.

**Rien à corriger dans l'émulateur** : le comportement observé est fidèle
au matériel. La touche Mac `#/@` reste routée vers le `#` du CPC, et
`SHIFT+$` produit le code 64 (`@`), affiché `à` — exactement comme sur un
vrai 6128 français.
