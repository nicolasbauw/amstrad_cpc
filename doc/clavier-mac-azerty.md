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

## Le mystère du `@`

Une fois `<`, `>` et `SHIFT+$` fiables, restait : `@` (Mac, sans SHIFT)
donne `#` sur le CPC au lieu de `@`. La légende du clavier CPC réel place
pourtant `@` en variante SHIFTée de `$`, à la position `(2,6)` de la
matrice.

### Recherche exhaustive, sans résultat

Testé méthodiquement, position par position, en pilotant la matrice CPC
directement (sans passer par la traduction clavier Mac, pour écarter toute
suspicion côté SDL) :

- `(2,6)` avec SHIFT CPC posé proprement (isolé, sans rien d'autre) : **`à`**,
  pas `@`.
- Toutes les positions à caractère imprimable des lignes 2 à 8 de la
  matrice, y compris toutes les variantes SHIFT jamais vérifiées jusque-là
  (`^` mort, `-`, `)`, `,`, `;`) : aucune ne donne `@`. (Trouvaille au
  passage : le `^` shifté donne aussi `ù`, un doublon avec sa position déjà
  connue — la table de caractères du CPC a apparemment plusieurs
  redondances de ce genre.)
- Le pavé "calculatrice" `f0`-`f9` / `.` / flèches (lignes 0 et 1) avec
  SHIFT : toujours les mêmes chiffres, SHIFT n'y change rien.
- La page Wikipédia *Amstrad CPC character set* : confirme que `@` est au
  code standard ASCII `0x40`, mais ne documente aucune disposition clavier.

### La conclusion, obtenue autrement

Plutôt que de continuer à chercher quelle touche produit `@`, la question a
été retournée : **que produit vraiment le code `CHR$(64)` sur cette ROM ?**

```basic
PRINT CHR$(64)
```

Réponse de la ROM : **`à`**, pas `@`.

La table de caractères française a réattribué le code 64 (habituellement
`@` en ASCII standard) à `à`, pour loger les lettres accentuées (`à`, `é`,
`è`, `ç`, `ù`...) dans une table de taille fixe. `@`, peu utilisé en BASIC
français, a été sacrifié. Ce n'est donc pas un problème de disposition
clavier : le glyphe `@` n'existe simplement plus dans la police de cette
ROM. La légende `@ \ $` gravée sur le clavier physique (probablement un
moule de touches générique, réutilisé pour plusieurs marchés) ne correspond
plus au firmware français réellement chargé.

**Comportement retenu** : la touche Mac qui produirait `@` (sans SHIFT, sur
la touche `#/@`) reste routée vers le `#` du CPC — c'est le choix le plus
utile possible, puisqu'aucun caractère `@` n'existe à atteindre de toute
façon.

## Méthode

Toute cette investigation s'est faite sans jamais avoir la main sur un vrai
Mac : par itérations avec l'utilisateur (qui teste sur son clavier réel et
rapporte le symptôme exact), combinées à une instrumentation temporaire
(`KEYLOG`, retirée) tracée événement par événement, et à des bancs d'essai
headless (`Machine::step` piloté directement, sans fenêtre, capture d'écran
PNG relue directement) pour vérifier chaque hypothèse avant de la proposer
comme correctif — jamais l'inverse.
