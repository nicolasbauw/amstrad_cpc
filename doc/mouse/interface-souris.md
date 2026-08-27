# Interface souris — souris logicielle ByteBox

## Contexte et décision

La souris émulée par ByteBox n'est **pas** la reproduction d'un adaptateur
matériel réel (AMX Mouse, SYMBiFACE II/X-MEM...). Décision prise pendant le
développement du projet [dune-cpc](https://github.com/nicolasbauw) (premier
consommateur de cette interface) : abandon de la piste "coller au standard
existant" après recherche — la documentation fiable de ces protocoles a
disparu (pages CPCWiki disparues), et le public possédant physiquement ce
matériel est quasi inexistant. Comme ByteBox et le logiciel qui l'utilise
sont développés par la même équipe, il n'y a aucun gain de compatibilité à
chasser un protocole externe non fiabilisé.

Le protocole choisi reste inspiré des souris relatives classiques (deltas
X/Y + état des boutons), par choix de conception général — pas pour imiter
un standard existant. Ça garde la porte ouverte à moindre coût si un
support hardware réel devenait pertinent un jour, sans que ce soit un
objectif actuel. Le hardware CPC réel n'a évidemment aucun support souris :
un logiciel qui en dépend doit prévoir un repli manette/clavier.

## Protocole

Trois ports, sur la page `&FC00-&FCFF` réservée aux périphériques logiciels
ByteBox sans équivalent matériel (`core/src/bus.rs`,
`bytebox_mouse_selected` — ne recoupe le décodage d'aucun composant réel) :

| Port | Registre | Comportement |
|---|---|---|
| `&FC00` | Delta X | Octet signé. **Consommé à la lecture** (remis à 0 côté core). |
| `&FC01` | Delta Y | Même principe que `&FC00`. |
| `&FC02` | Boutons | Bit 0 = gauche, bit 1 = droit. **Pas** consommé (lecture répétée sans changement rend la même valeur). |

Le bouton du milieu n'atteint jamais le CPC — intercepté côté ByteBox pour
capturer/relâcher la souris (voir plus bas) — il n'a donc pas de bit dédié
sur `&FC02` dans la pratique, bien que `core::mouse::Mouse` réserve le bit 2
en interne pour lui.

Désactivée par défaut. Port non décodé (`0xFF`) tant que désactivée, pour
qu'un pilote ne confonde pas "absente" et "immobile". Trois façons de
l'activer :
- `config.toml`, section `[mouse]`, `enabled = true` ;
- commande console à chaud `mouse on`/`mouse off` (F10/F11) ;
- case "Enable mouse" du panneau de configuration (F6).

## Capture (`bytebox/src/sdl.rs`)

Clic gauche dans la fenêtre principale (hors overlay F6/F7/F10, souris
activée dans la configuration) capture la souris de l'hôte en mode relatif
SDL2 ; ce clic n'est pas transmis au CPC. Clic milieu relâche — également
absorbé, jamais transmis. Relâche automatique sur perte de focus,
ouverture d'un overlay, ou désactivation en cours de route. Le masquage du
curseur au survol de la fenêtre (comportement existant) n'est pas affecté.

## Driver bas niveau RASM (`mouse-driver.asm`)

Fourni dans ce même dossier, prêt à assembler/inclure tel quel dans un
projet RASM. Convention d'appel par registres (CALL/RET/PUSH/POP), pas de
convention C (`__sdcccall` ou autre) — à envelopper séparément si le reste
du projet appelant est écrit en C.

- `mouse_update` : lit les trois ports, accumule les deltas dans
  `mouse_x`/`mouse_y` (16 bits signés, **sans borne** — le repère
  écran/carte appartient à l'appelant). Met à jour `mouse_buttons`/
  `mouse_previous_buttons`. À appeler une fois par itération de la boucle
  de jeu.
- `mouse_left_click_evt` / `mouse_right_click_evt` : détection de front
  montant (à appeler après `mouse_update`), pour un clic "évènement"
  plutôt que l'état brut — utile pour un menu/bouton, par exemple.
- `mouse_read_dx` / `mouse_read_dy` / `mouse_read_buttons` : accès bas
  niveau direct aux trois ports, si `mouse_update` ne convient pas.

Piège rencontré et documenté dans les commentaires du driver : `CALL`/`RET`
exigent SP en RAM simple, jamais dans une zone ROM potentiellement paginée
(`&C000-&FFFF` haute ou `&0000-&3FFF` basse) — la ROM masque les
**lectures** à ces adresses (pas les écritures), ce qui corrompt
silencieusement toute adresse de retour empilée là. La pile par défaut
d'un snapshot RASM (`SP=&C000`) y tombe en plein dedans.

Validé par du test manuel en conditions réelles (souris capturée,
clic/déplacement, retour visuel à l'écran) dans le projet dune-cpc.

## Exemple : déplacer un pointeur (`mouse-cursor-demo.asm`)

Programme complet et autonome, construit sur `mouse-driver.asm` : déplace un
petit triangle (caractère `0xF4` de la police ROM du CPC) sur l'écran, dans
le sens de chaque delta souris, **au pixel près sur les deux axes**.

Une version intermédiaire ne l'était qu'à la verticale (voir plus bas
pourquoi), l'horizontale restant à la cellule de caractère (8 pixels) —
anodin en apparence (chaque axe testé seul semblait correct), mais très
perturbant en diagonale : un même mouvement physique de la souris
parcourait alors 8 fois plus de distance à l'écran horizontalement que
verticalement, un vrai décalage de sensibilité entre les deux axes, pas
juste une impression. La précision pixel sur les deux axes coûte une vraie
complexité en plus : un glyphe ne commence alors plus forcément sur une
frontière d'octet (8 pixels) — son motif 8x8 doit être scindé, balayage par
balayage, entre les deux octets écran qu'il chevauche désormais (voir
`draw_glyph`) — MODE 2 empile 8 pixels par octet, aucun moyen d'y échapper
sur un écran adressé par octet.

```
rasm mouse-cursor-demo.asm -oi mouse-cursor-demo.sna -v2 && bb --snapshot=mouse-cursor-demo.sna
```

**Aucun appel firmware** (pas de `TXT_*`), volontairement, comme les POC de
dune-cpc : une snapshot construite par `BUILDSNA` de RASM donne directement
la main au programme, sans jamais exécuter le vrai démarrage ROM — le
tableau de saut du firmware, en RAM, normalement peuplé par cette séquence,
reste donc vide, et l'appeler ne fait rien (vérifié : une première version
de cette démo utilisant `TXT_WR_CHAR`/`TXT_SET_CURSOR` affichait un écran
uniformément vide). La démo pose donc directement les octets en mémoire
écran (MODE 2, qu'elle configure elle-même — 1 bit par pixel, correspondant
exactement au format de la police ROM), à l'adresse
`&C000 + (ligne_pixel/8)*80 + colonne_octet + (ligne_pixel AND 7)*&800` —
`&800` octets séparent deux balayages consécutifs d'une même ligne de
caractère, `80` sépare deux lignes de caractère consécutives. La
division/le AND gèrent la position verticale au pixel près : les 8
balayages d'un même glyphe peuvent ainsi chevaucher deux lignes de
caractère différentes dès que sa position verticale n'est plus un multiple
de 8.

Pour la colonne, `colonne_octet` (`cursor_col`/8) désigne l'octet de
GAUCHE des deux que le glyphe peut toucher ; `cursor_col AND 7` donne son
décalage en bits à l'intérieur. `shift_glyph_byte` scinde chaque octet
source du glyphe en deux contributions (octet gauche, octet droit) via
l'astuce classique du décalage 16 bits d'une paire de registres —
`cursor_col` doit donc être sur 16 bits (`defw`), 600 ne tenant pas dans un
octet.

Trois pièges rencontrés et corrigés pendant la mise au point :
- Les routines `calc_row_addr`/`draw_glyph`/`erase_glyph` utilisent toutes
  `BC` (calcul intermédiaire ou compteur de boucle) — un appelant qui y
  range encore le delta souris le perd silencieusement à l'appel suivant.
  Les deltas sont donc mis de côté en mémoire, pas gardés en registre, le
  temps de ces appels.
- Le compteur de balayage (0-7) de `draw_glyph`/`erase_glyph` est lui
  aussi en mémoire (`plot_i`), pas en registre : `calc_row_addr` étant
  appelée une fois par balayage et clobbant A/BC/HL, aucun registre (sauf
  IX, réservé au pointeur vers le glyphe) ne survit d'un appel à l'autre.
- Résolution pixel identique sur les deux axes seulement une fois le
  besoin remonté par un test réel en diagonale — les deux axes testés
  séparément (horizontal seul, vertical seul) ne suffisaient pas à
  détecter le déséquilibre de sensibilité entre eux.
