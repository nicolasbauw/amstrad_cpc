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

## Ce qui a été écarté

- **Pas une simple imbrication d'interruptions au sens Z80 du terme** :
  à chaque entrée en `0x0038`, `IFF1` est bien à `false` (comme attendu,
  une interruption acceptée masque les suivantes jusqu'au `EI` final du
  gestionnaire). Une tentative de détecter une « interruption imbriquée »
  en surveillant un retour à `0x3111` (fin du chemin long du
  gestionnaire) a produit de nombreux faux positifs : le gestionnaire a
  aussi un chemin de sortie COURT (`0x3090`-`0x309A`, simple
  `POP`×4/`EI`/`RET`, sans bascule de pile), pas systématiquement
  emprunté — un détecteur qui ne surveille que la sortie du chemin long
  se dérègle dès que le chemin court est pris une seule fois. Cette piste
  d'instrumentation est à refaire correctement (détecter les DEUX points
  de sortie, ou mieux : surveiller `IFF1` directement plutôt que des
  adresses PC précises) avant de conclure quoi que ce soit sur une
  éventuelle vraie imbrication.
- Le tout premier bug WEC (RST qui saute l'incrément du PC) est déjà
  corrigé et vérifié sans effet secondaire sur ce nouveau symptôme — ce
  n'est pas une résurgence du même bug, au moins pas directement (le
  mécanisme ici est un `RET` qui dépile une mauvaise adresse, pas un
  opérande de far-call mal aligné).

## Hypothèses à trancher

1. **Un vrai bug de timing d'interruption chez nous**, différent du
   premier : par exemple si notre émulation autorise, dans une fenêtre
   très précise, l'acceptation d'une interruption à un moment où le vrai
   Z80 ne le ferait pas (pas nécessairement une « imbrication » classique
   — pourrait être lié à `ei_instr_delay`, au moment exact où `iff1`
   redevient vrai après le `EI` de `0x3110`, ou à un cas limite autour de
   `RETI`/`RETN` si le gestionnaire les utilise ailleurs) ;
2. **Un dépassement de pile légitime côté jeu** que le vrai matériel
   évite uniquement parce que son timing (cycles Z80 exacts, longueur
   réelle de chaque instruction) diffère suffisamment du nôtre pour que
   la zone `0x3143` et la pile principale du jeu ne se télescopent
   jamais — auquel cas le bug serait plus subtil (un écart cumulatif de
   timing, pas une seule instruction fautive) ;
3. **Une piste plus simple à vérifier d'abord** : le jeu utilise-t-il À
   NOUVEAU le mécanisme de far-call (RST) étudié pour le premier bug
   pendant cette séquence de préparation de course ? Si oui, revérifier
   spécifiquement ce chemin avec le correctif déjà en place (peut-être
   pas suffisant à lui seul, ou un cas apparenté non couvert par le
   correctif).

## Prochaine étape recommandée

Refaire un détecteur d'imbrication d'interruption fiable (suivre `IFF1`
directement, pas des adresses PC de sortie), puis comparer avec
Caprice32 (méthode déjà rodée : ROM identiques, injection clavier directe
dans `keyboard_matrix`, voir `doc/wec-le-mans-frozen-splash.md` section
« Harnais de diagnostic ») sur la même séquence (menu → « 2 » → écran de
préparation) pour voir si `SP` y dérive aussi dans la zone `0x31xx`, ou
si notre émulation diverge quelque part de précis dans cette fenêtre de
~1,18 s.

## Harnais de diagnostic

Tests `investigate_wec2_*` dans `src/machine.rs`, tous retirés après
cette session. Reproduction : taper `run"wec`, attendre ~12 s (le menu
apparaît), presser « 2 » (ligne 8 bit 1 + SHIFT, table AZERTY), puis
observer ~6 s. Méthode de capture d'écran identique à l'enquête
précédente (buffer RGB24 → PPM → PNG via `magick`).
