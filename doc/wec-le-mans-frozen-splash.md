# WEC Le Mans : reste figé sur l'écran de démarrage (ouvert)

Note d'enquête, point de départ pour une reprise ultérieure. **Le symptôme
n'est pas résolu.**

## Le symptôme

```
cargo run -- -d WEC_Le_Mans.dsk -a 'run"wec'
```

L'image de démarrage (splash "WEC Le Mans") s'affiche puis reste figée
indéfiniment. Sur Caprice32 avec la même disquette et la même commande, le
menu du jeu devrait s'afficher après quelques secondes.

## Reproduction automatisée

Boot via `--autocmd`, captures périodiques toutes les secondes. L'écran
cesse de changer après ~7 secondes ; le PC reste ensuite indéfiniment dans
une petite plage d'adresses (confirmé sur 30 secondes d'observation, sans
qu'aucun octet de VRAM ne bouge). La capture confirme visuellement l'écran
titre figé (voiture rouge, logo "WEC Le Mans"), conforme à la description.

## Ce qui a été écarté

- **Pas un blocage type TMHT** (DI jamais suivi d'un EI) : le compteur
  d'interruptions acceptées avance normalement (107 sur une fenêtre de
  200 000 pas), et le code visité fait bien des paires DI/EI équilibrées
  (section critique classique autour de la lecture clavier/PSG, pas un
  verrou permanent).
- **Pas une simple attente de touche/tir manette** : simuler un appui
  ESPACE puis un tir manette (Joystick A Fire 1) après le figement ne
  change strictement rien à l'écran.
- Une adresse mémoire (`0xB831`) qui semblait de prime abord être le
  drapeau attendu par la boucle figée (`LD A,($B831) / OR A / JR Z,...` en
  `0xB958`) s'est révélée être un octet à usage multiple, réécrit des
  centaines de fois par seconde par des routines sans rapport apparent
  avec un quelconque "signal prêt" — fausse piste, à ne pas reprendre
  telle quelle.

## Ce qui est confirmé sur la boucle figée

Traçage de l'exécution réelle (méthode déjà éprouvée sur Bruce Lee et
TMHT : un désassemblage linéaire à partir d'une adresse arbitraire se
désynchronise au premier branchement, ne pas s'y fier — tracer les PC
réellement visités puis désassembler chacun individuellement).

Boucle d'attente interne (`0x2BFC`-`0x2BFF`, simple décompte BC) imbriquée
dans une boucle externe (`0x2BE2`-`0x2BF3`) qui rappelle une routine de
lecture via les ports `&F6xx`/`&F4xx` (PPI Port C / Port A, sélection puis
lecture — le même motif que la lecture clavier standard du firmware CPC,
10 itérations = 10 lignes de matrice), mais l'expérience "appui simulé"
ci-dessus a démontré que ce n'est probablement pas (uniquement) une
attente de clavier.

## Ce qui reste à faire

Retrouver ce qu'attend réellement la boucle externe `0x2BE2` — a priori ni
une touche, ni un tir manette. Pistes possibles à explorer dans une
prochaine session :
- tracer précisément ce que lit le port `&F4xx` dans cette boucle (peut-être
  pas le clavier mais autre chose lié au PSG ou à un DIP switch émulé) ;
- vérifier si le jeu attend un état du FDC (le lecteur de disquette) —
  `motor_on=false` était observé en fin d'exécution, peut-être un accès
  disque attendu qui ne se déclenche jamais ou dont le résultat n'est
  jamais celui espéré ;
- comparer avec Caprice32 si un moyen fiable de capture d'écran/trace est
  disponible (tenté cette session via ffmpeg + X11grab, sans succès net —
  voir aussi les limites d'outils de capture d'écran rencontrées
  précédemment).

## Harnais de diagnostic

Tests `investigate_wec_le_mans*` dans `src/machine.rs`, retirés après
cette session. Méthode réutilisable : captures périodiques pour localiser
le moment du figement, puis traçage pas à pas dédupliqué de l'exécution
réelle (voir `doc/bruce-lee-palette.md` pour le détail de la technique).
