# Discology : la copie de disquette bloquée sur « Lecture piste 00 »

## Le symptôme

Discology 2.0 se lance et navigue normalement. Dans le module *Copieur*,
menu *Disquette* → *Copie Intégrale*, il affiche « Lecture piste : 00 » puis
ne bouge plus. Le Z80 tourne alors indéfiniment dans cette boucle, en RAM :

```
11C8  PUSH AF          ; routine "envoyer un octet au FDC"
11C9  PUSH AF
11CA  IN A,(C)         ; lecture du registre d'état (MSR)
11CC  ADD A,A          ; la retenue reçoit RQM (bit 7)
11CD  JR NC,$11CA      ; tant que le FDC n'est pas prêt
```

Le piège : notre MSR renvoyait TOUJOURS RQM=1, cette boucle ne pouvait donc
pas s'y arrêter… sauf que `BC` valait `0x0005` et non `0xFB7E`. La lecture
n'atteignait pas le FDC mais le PPI, qui répond `0x00` : RQM=0 pour
l'éternité. Autrement dit, le blocage observé n'était que le symptôme final
d'une mémoire déjà corrompue — le vrai problème était en amont.

## Ce que fait vraiment le copieur

Avant de copier une piste, Discology en relève la carte. La boucle, en RAM :

```
12FA  PUSH DE
12FB  CALL $121E       ; Read ID (commande 0x4A du FDC)
12FE  POP DE
12FF  LD A,($103F)     ; octet fort du compteur d'attente
1302  CP $41
1304  JP NC,$1441      ; budget écoulé -> fin du relevé, recherche des doublons
1307  LD HL,$BE4F      ; C,H,R,N du secteur qu'on vient d'identifier
130A  LD BC,$0004
130D  LDIR             ; ... rangés dans la table des secteurs
130F  INC (IY+$01)     ; un secteur de plus
1312  EX DE,HL
1313  LD DE,$0005
1316  ADD HL,DE        ; entrée suivante de la table
1317  EX DE,HL
1318  JR $12FA
```

et le compteur d'attente, incrémenté une fois par interrogation du MSR :

```
111C  PUSH HL
111D  PUSH DE
111E  LD HL,($103E)
1121  IN A,(C)         ; MSR
1123  INC HL           ; <- on COMPTE le temps d'attente
1124  CP $C0
1126  JR C,$1121
1128  LD ($103E),HL
```

Discology ne compte donc pas les secteurs : il **chronomètre**. Il enchaîne
les Read ID jusqu'à avoir attendu l'équivalent d'un tour de disquette, et en
déduit la liste des secteurs de la piste (la routine en `1441` repère au
passage l'identifiant qui revient, signe que le tour est bouclé).

Deux comportements du contrôleur étaient indispensables, et absents :

1. **Read ID renvoyait toujours le premier secteur de la piste.** Sur un vrai
   µPD765A, la disquette tourne : chaque Read ID rapporte l'identifiant qui se
   présente à ce moment-là sous la tête, donc les secteurs défilent dans
   l'ordre physique de la piste.
2. **Le contrôleur répondait instantanément.** Le budget de Discology ne
   s'écoulait donc jamais : la boucle de relevé tournait sans fin, la table
   des secteurs (5 octets par entrée) débordait sur le code du programme, et
   c'est ce code écrasé qui appelait la routine d'envoi au FDC avec un `BC`
   fantaisiste — d'où la boucle finale en `11CA`.

## Le correctif

Dans `fdc.rs` :

- **Position angulaire.** Le contrôleur tient une horloge (`Fdc::time`,
  avancée depuis `Machine::step`). Read ID cherche quel identifiant se
  présentera ensuite sous la tête et fait patienter le contrôleur jusque-là
  (`busy_ticks`) ; pendant ce temps le MSR annonce « occupé, rien à
  transférer » (CB=1, RQM=0). Modéliser la POSITION plutôt qu'un délai fixe
  est important : le temps de n Read ID successifs vaut alors exactement un
  tour, quel que soit le temps de traitement du logiciel entre deux
  commandes.
- **Espacement réel des secteurs.** Ce n'est pas un tour divisé par le nombre
  de secteurs : au format CPC habituel, les secteurs n'occupent qu'une partie
  du tour, le reste étant l'intervalle final avant le trou d'index.
  L'espacement est donc calculé à partir de la taille des secteurs
  (`sector_pitch_ticks`, 128 cycles par octet à 250 kbit/s), plafonné à un
  tour. Une valeur trop grande fait manquer des secteurs au relevé (copie
  incomplète), une valeur trop petite en fait relever en double.

## L'espacement des secteurs est un réglage empirique, et il est fragile

`SECTOR_OVERHEAD_BYTES` (`fdc.rs`) chiffre les octets de service autour de
chaque secteur : synchronisation, en-tête d'identification et son CRC,
marque de données, CRC des données, intervalle jusqu'au secteur suivant.

**La valeur physiquement exacte ne convient pas.** Le format AMSDOS standard
donne 144 (22 pour l'en-tête d'identification, 22 de GAP2, 18 autour du champ
de données, 82 de GAP3) — et avec elle, la copie échoue. La raison : le
relevé de Discology ne dispose que d'environ 0,92 tour de disquette pour
cartographier une piste (16 640 interrogations du registre d'état, à 44
cycles chacune), et ne peut donc pas voir les neuf secteurs d'une piste qui
en occuperait 0,94. Les disquettes réelles s'en tirent parce que leur GAP3
est ajusté au nombre de secteurs — celle de Discology en loge dix sur sa
piste 0.

**Et le comportement n'est pas monotone.** Balayage mesuré : 96 et 100
conviennent ; 92, 98, 104, 115 et 144 non ; 108 et 130 si. Le nombre
d'identifiants relevés bascule piste par piste, sans seuil net. La valeur
retenue (100) est une valeur *vérifiée*, pas un optimum : il n'existe pas de
plage stable.

### Effet de bord assumé, découvert en corrigeant le CPU

Ce réglage dépend directement de la vitesse du CPU émulé, puisque Discology
mesure le temps en comptant des tours de boucle. Quand la correction du
temps machine du Gate Array a été apportée (voir
`doc/sprite-flicker.md` : le CPC étire chaque cycle machine, pas
l'instruction entière), la boucle de sondage de Discology s'est allongée —
elle contient un `IN A,(C)`, passé de 12 à 16 cycles. Son budget a donc
changé, et `SECTOR_OVERHEAD_BYTES` a dû être recalé de 62 à 100 pour que la
copie reste fidèle.

C'est le signe que ce paramètre compense l'imprécision du modèle de rotation
plutôt qu'il ne décrit une réalité physique : toute modification du temps
CPU le remettra en cause.

### Le "vrai modèle de rotation" est une impasse — mesuré

L'idée paraissait évidente : donner à chaque secteur sa **position angulaire
réelle**, lue dans l'image `.dsk`, plutôt que de déduire un espacement
uniforme d'une constante globale. Le temps d'attente d'un Read ID
deviendrait « temps jusqu'à ce que le prochain identifiant passe sous la
tête », sans paramètre à régler.

Trois mesures l'ont invalidée. Elles sont consignées ici pour que personne
ne recommence.

**1. La valeur physiquement exacte échoue toujours.** Avec
`SECTOR_OVERHEAD_BYTES = 144` (les gaps AMSDOS standard), la copie ressort
vide. Pire que « il manque le dernier secteur » : l'instrumentation montre
que Discology n'émet que **2 Read ID** au lieu d'entamer un relevé — son
budget est épuisé bien avant.

**2. La donnée de l'image n'est pas exploitable.** L'en-tête de piste du
`.dsk` porte bien un champ GAP#3 (offset 0x16), que notre parseur ignorait.
Le lire ne servirait à rien : `bin/Discology.dsk` déclare `GAP#3 = 78`
partout, y compris sur sa piste 0 qui loge **10 secteurs** de 512 octets.
Soit 10 × (512 + ~140) = 6520 octets, pour une capacité de piste de 6250 à
250 kbit/s. **La géométrie déclarée ne rentre pas dans un tour** : c'est une
valeur nominale, pas une description du tracé réel.

**3. Le seul modèle sans paramètre échoue aussi.** « Une piste occupe
exactement un tour » (physiquement vrai, et qui supprimerait la constante) :
copie en échec également.

### Ce que l'instrumentation a montré du mécanisme réel

En traçant chaque Read ID (delta de temps, index de secteur, sondages MSR) :

- Discology relève une piste en exactement autant de Read ID qu'elle a de
  secteurs, les index défilant proprement (`0,1,2…8,0`) — le modèle de
  rotation fait donc bien son travail.
- Chaque Read ID coûte ~**1666 sondages MSR** à ~**45 cycles** l'un.
- Un relevé de 9 secteurs occupe ~705 000 cycles, soit **0,88 tour**, avec la
  constante à 100. Une piste physiquement réelle en occuperait 0,945.

### Deux causes éliminées

**Le temps CPU est hors de cause.** La boucle de sondage de Discology coûte
chez nous exactement ce qu'elle coûte chez Caprice32, opcode par opcode :
`IN A,(C)` = 16 (leur `Ix` = 12, plus 4 pour le préfixe `ED`) et `JR` pris =
12 (leur `cc_op` = 8 plus `cc_ex` = 4). Aucun écart à récupérer de ce côté.

**Caprice32 ne peut pas servir de référence ici : il n'a aucun modèle de
rotation.** Son `fdc_readID` (`src/fdc.cpp`) renvoie le secteur suivant
*instantanément*, via un simple compteur d'index remis à zéro en fin de
piste — ni attente, ni position angulaire. Notre modèle est donc un ajout
par rapport à l'émulateur de référence, et c'est lui qui rend Discology
sensible à un budget temps que Caprice32 n'exerce jamais.

### Où ça en reste

La constante ne compense pas une imprécision de la géométrie du disque,
comme on l'a d'abord cru : elle compense le fait que **Discology dispose de
moins de marge angulaire chez nous que sur une vraie machine**, pour une
raison qui n'est ni le temps CPU ni les données de l'image. Resserrer les
secteurs sous leur espacement physique lui rend cette marge.

La suite logique serait de désassembler la routine de seuil (le compteur en
`103E`) pour savoir ce qu'elle attend exactement. Chantier ouvert, sans
garantie : tant qu'aucun autre logiciel ne souffre du réglage actuel, la
copie fonctionne et le test de bout en bout le verrouille.

## Vérification

`machine::tests::discology_copies_a_disk_track_by_track` (ignoré par défaut,
il émule près de trois minutes de CPC) va au bout du scénario : lancement de
Discology, navigation au clavier jusqu'à *Copie Intégrale*, lecture des 40
pistes, insertion d'une disquette vierge à la demande de DESTINATION,
écriture, puis comparaison octet à octet de l'image obtenue avec la source.

```
cargo test --release discology_copies -- --ignored
```

Résultat : toutes les pistes sont copiées, et tous les secteurs à marque
normale sont identiques à la source.

## Les marques « Deleted Data » (Plan V3, point 3)

Les secteurs à marque « Deleted Data » (la protection de cette disquette :
pistes 9 à 17, secteurs 0x31-0x33 de 4 Ko) n'étaient pas reproduits, pour
trois raisons — la troisième découverte en corrigeant les deux premières :

- **Read Data (0x06) ne voyait pas du tout un secteur « deleted ».**
  L'implémentation filtrait strictement sur la marque, se comportant donc
  toujours comme si le bit SK valait 1. Un vrai µPD765A distingue les deux
  cas : avec SK=1 il saute le secteur, avec **SK=0 il le lit quand même**,
  lève le bit 6 de ST2 (Control Mark) et s'arrête après lui. C'est
  précisément ce signalement que cherchent les protections.
- **Write Deleted Data (0x09) n'existait pas.** Même relus, ces secteurs
  auraient été réécrits en marque normale. La commande partage désormais
  tout le chemin de Write Data (0x05), seule la marque posée diffère.
- **L'écriture du `.dsk` ne reportait pas la marque.** `parse_track_header`
  lit pourtant ST2 (offset +5 de chaque descripteur de secteur) au
  chargement, mais `write_dsk_file` n'écrivait que C/H/R/N : la marque se
  perdait à la persistance, ce qui aurait suffi à annuler les deux
  correctifs ci-dessus.

### Le risque annoncé, et sa vérification

Le comportement précédent avait été retenu pour **Teenage Mutant Hero
Turtles**, dont la protection repose sur ces marques : le plan prévenait
qu'y toucher demandait de retester ce jeu. Fait —
`bytebox --disk=bin/Teenage_Mutant_Hero_Turtles.dsk --autocmd='RUN"DISK'`
atteint l'écran de titre comme avant. Discology et les tests unitaires du
FDC (dont deux nouveaux couvrant SK=0 et SK=1 dans les deux sens) sont
également au vert.
