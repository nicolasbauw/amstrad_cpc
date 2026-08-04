# Barbarian : la démo jouait au ralenti (résolu par ricochet)

Note d'enquête, conservée pour la rétro-ingénierie du moteur qu'elle contient —
elle servira sans doute à d'autres jeux. **Le symptôme a disparu**, réglé par un
correctif fait pour un tout autre bug (voir « Résolution » en fin de document).

## Le symptôme

Lancé par `RUN"BARBA.I`, le jeu démarre sur sa démo. Les deux combattants
s'avancent l'un vers l'autre, puis restent quasi immobiles : ils tiennent chaque
posture plusieurs secondes au lieu d'enchaîner. Sur un émulateur de référence,
avec la même image disque, le combat est vif et la démo complète dure 62
secondes ; chez nous, un cycle complet en dure 214.

Le clavier et la manette répondent, le mode de jeu (MODE 3) se joue normalement
et les coups y sortent instantanément : **le défaut est propre à la démo**.

## La mesure qui cadre le problème

| phase | temporisation demandée | chez nous | référence |
|---|---|---|---|
| approche (sous-script 0x0D) | 18 × 8 = 144 unités | 3,5 s | 3,5 s |
| combat (sous-script 0x11) | 40 × 8 = 320 unités | 3,2 s | ~1 s |

L'approche dure autant des deux côtés : **la base de temps du moteur est
juste**. Seule la phase de combat diffère.

## Le moteur, tel qu'il a été reconstitué

### Interruption et cadence

Vecteur `0x0038` → `JP 0x7AE2`. Le gestionnaire commence par `EI` : il est
délibérément réentrant.

```
7AEB  IN A,(0xF5xx)     ; port B du PPI, bit 0 = VSYNC
7AF3  LD (0xC7D4),0     ; au VSYNC : remise a zero du compteur
7AF7  LD A,(0xC7EF)
7AFA  CP 3
7AFC  CALL Z,0x7B20     ; logique de jeu, SEULEMENT a l'etat 3
7AFF  INC (0xC7D4)      ; compteur d'interruptions dans la trame
7B0B  CP 2 : CALL Z,0x7DC2   ; mise a jour des entites
7B13  CP 5 : CALL Z,0x7DC2   ; et une seconde fois
```

Six interruptions par trame, compteur remis à zéro une fois par trame : **deux
mises à jour d'entités par trame, soit 100 Hz**. C'est structurel, sur n'importe
quelle machine. Le gestionnaire n'est pas auto-modifié.

Les variables du jeu (`0xC7D0` et suivantes) vivent dans les 48 octets non
affichés de chaque bloc de 2 Ko de l'écran — rangement classique sur CPC.

### Les entités

Trois structures de 48 octets : `0x82D3`, `0x8303`, `0x8333`. Champs identifiés :

| offset | rôle |
|---|---|
| +00/+01 | script de base |
| +02/+03 | pointeur de script courant |
| +04/+05 | décompte de temporisation (16 bits) |
| +06/+07 | valeur de rechargement de la temporisation |
| +08, +09 | champs réglés par les opcodes 0x81 et 0x80 |
| +0A/+0B | pointeur de l'image à afficher |
| +10, +13 | compteurs d'animation, chargés depuis +22 et +18 |
| +16..+24 | enregistrement de 15 octets copié par l'opcode 0x87 depuis la table `0x8370` |
| +2A | ajouté à +09 pour l'affichage |
| +2B/+2C | curseur d'animation, ajouté à +0A/+0B pour l'affichage |
| +2D | octet de drapeaux (RES 0 et 1 par l'opcode 0x87, OR par 0x8A) |
| +2E/+2F | pointeur de retour, sauvegardé par l'opcode 0x8C |

Le rendu affiche `image = (+0A/+0B) + (+2B/+2C)` (construction de la liste
d'affichage en `0x7EF6`-`0x7F19`).

### Le langage de script

Boucle de dispatch en `0x7DF0` : un octet ≥ 0x80 est une commande, cherchée dans
la table de branchement `0x829F` ; un octet < 0x80 est un numéro d'image, résolu
par la table `0x81DF`.

| opcode | routine | rôle |
|---|---|---|
| 0x80 | 0x7FED | écrit l'opérande dans +09 |
| 0x81 | 0x7FF6 | écrit l'opérande (masquée à 0x09) dans +08 |
| 0x82 | 0x8001 | reboucle : pointeur courant ← script de base |
| 0x83 | 0x8010 | temporisation = opérande × facteur global (`0x82C7`) |
| 0x84 | 0x8025 | fin de pas |
| 0x85 | 0x804C | fixe le facteur global : `6000 / (opérande × 8)` |
| 0x86 | 0x8029 | — |
| 0x87 | 0x807A | charge 15 octets de paramètres depuis `0x8370 + n×15` |
| 0x88 | 0x80C3 | posture : range une valeur en `0x836E` et valide des champs |
| 0x89 | 0x80D7 | — |
| 0x8A | 0x8066 | arme des bits dans +2D |
| 0x8B | 0x7FDB | **installe** un sous-script (préempte le courant) |
| 0x8C | 0x7FBB | appelle un sous-script (sauvegarde le retour en +2E/+2F) |
| 0x8D | 0x7FD2 | retour au script appelant |

Table des sous-scripts : `0x907D`, entrées de 16 bits.

Routines utilitaires : multiplication `0x8160` (HL = A × DE), division `0x8146`
(BC = BC / DE, reste dans HL), indexation de table `0x8132`.

### Les deux chemins de commande

**En jeu** — un coup est *installé*, donc immédiat :

```
0x764C  lit un octet du flux de mouvements pointe par (0xC7D0)
0x70EA  point d'entree de l'installation
0x7D8D  ecrit le script de base ET le pointeur courant, temporisation a ZERO
```

Ce chemin vit dans la logique de jeu, qui ne tourne qu'à l'état 3.

**En démo** — pure chorégraphie, appel et retour. Sur 30 secondes, le langage
n'emploie que `80 81 83 84 85 87 88 8C 8D`. Jamais `0x8B` (l'installation),
jamais `0x82`.

Séquences de démo : `0x85F2` pour le combattant 1, `0x8673` pour le second,
`0x86C1` pour la troisième entité. Listes linéaires de `8C nn`, terminées par un
`82` qui reboucle. Celle du combattant 1 comporte 22 mouvements, puis 31 appels
au sous-script 0x23 (une pause voulue, 23 s chez nous), puis un final de 11
mouvements.

### La temporisation

`temporisation = octet du script × facteur global`, le facteur étant en
`0x82C7`. Il est calculé par l'opcode 0x85 : `6000 / (86 × 8) = 8`. Fixé neuf
fois par tranche de trente secondes, hérité entre sous-scripts, et toujours égal
à 8. Le décompte est consommé à 100 Hz.

## Ce qui est éliminé

Toutes ces pistes ont été mesurées et écartées :

- **le chargement** : sous-scripts, séquences de démo, calcul du tempo et
  gestionnaire d'interruption sont identiques octet par octet au contenu du
  `.dsk` (offsets 0x1EB8D et 0x1F1E0 notamment) ;
- **l'arithmétique** : opérandes et résultats de chaque multiplication et
  division capturés, tous exacts ;
- **l'entrée** : maintenir ENTER, ESPACE, une direction ou le feu ne change
  rien ;
- **la géométrie de trame** : écran standard 312 lignes, 50,1 Hz, 6,00
  interruptions par trame ;
- **l'imbrication du gestionnaire** : testée en étirant le temps CPU jusqu'à
  175 %, sans effet ; il faudrait qu'une mise à jour dure trois périodes
  d'interruption ;
- **les temps d'attente du Gate Array** : implémentés depuis (chaque instruction
  arrondie au multiple de 4 cycles), neutres sur ce défaut ;
- **le réveil de la logique** : forcer `0xC7EF` à 3 pendant la démo fait bien tourner la logique à 50 Hz, mais elle ne lit jamais le flux de
  mouvements et n'écourte que deux temporisations en trente secondes ;
- **le curseur d'animation** (+2B/+2C) : il reste à zéro parce que la table de
  paramètres le dit, et cette table est correctement chargée.

## Ce qui restait ouvert, et la fausse piste qu'il fallait éviter

Nous exécutions la démo exactement comme elle est écrite, et pourtant la
référence était 3,5 fois plus rapide sur la seule phase de combat, tout en
concordant sur la phase d'approche. Toutes les pistes ci-dessus tenaient
toujours : c'est bien pour ça qu'aucune ne menait nulle part. La démo ne
prenait pas un chemin plus lent que la référence — **elle prenait un chemin
différent**, toujours le même, parce qu'une de ses entrées était figée.

## Résolution

En reprenant Caprice32 (installé localement, donc un témoin bien plus fiable
que l'émulateur en ligne) comme seconde référence, l'écart est apparu net :
aucune pause n'était visible nulle part, pas même sur les mouvements dont on
avait vérifié qu'ils programmaient une attente de 320 unités (3,2 s chez
nous). Un facteur global ne pouvait pas expliquer ça — puisqu'on avait
justement vérifié que ce facteur (`0x82C7 = 8`) était calculé correctement,
à partir de données disque elles-mêmes vérifiées correctes.

La bonne question n'était donc pas « pourquoi c'est plus lent » mais
« pourquoi la démo ne varie-t-elle jamais ». Or entre-temps, une séance sur
un défaut de Hotshot avait mis au jour que le registre R (compteur de
rafraîchissement mémoire) du Z80 n'avançait jamais dans `zilog_z80` — un vrai
bug d'émulation, indépendant de Barbarian, corrigé dans le dépôt `zilog_z80`
(commit `a8b39ac`, « Advance the R register on every M1 cycle, as the Z80
actually does »).

La démo de Barbarian se sert elle aussi de R comme source d'aléa à un
embranchement non identifié dans la rétro-ingénierie ci-dessus (`0x86` et
`0x89`, marqués « ? » dans la table des opcodes, en sont les candidats les
plus probables). Avec R figé à zéro, cet embranchement choisissait
**toujours** la même branche — celle qui contient les 31 répétitions du
sous-script 0x23 (pause de 23 s) et le final scripté de 11 mouvements, la
branche la plus lente possible. Une fois R réparé, la démo varie son
déroulement comme sur le vrai matériel : combat vif, scores qui progressent,
et un cycle mesuré à ~77 s chez nous contre 70 s chronométrées sur Caprice32
et 62 s rapportées par l'émulateur en ligne — le même ordre de grandeur,
avec l'écart résiduel attendu d'une pseudo-alea qui ne tire pas la même
séquence de branches d'une exécution à l'autre.

Aucun changement n'a été nécessaire dans `amstrad_cpc` : la correction vit
entièrement dans `zilog_z80`, en dépendance de chemin.

**Leçon pour la suite** : un symptôme de timing qui résiste à toute
vérification interne de l'arithmétique et de la cadence, mais qui varie
qualitativement (pas seulement en durée) d'une exécution à l'autre ou d'un
émulateur à l'autre, vaut la peine d'être regardé sous l'angle « qu'est-ce
qui pourrait rendre ce chemin non déterministe », avant de continuer à
chercher un facteur de temps uniforme.
