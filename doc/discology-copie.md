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

### À faire un jour : un vrai modèle de rotation

Le correctif de fond serait de donner à chaque secteur sa **position
angulaire réelle** sur la piste, lue dans l'image `.dsk` (ordre physique des
secteurs, tailles réelles, et pour l'Extended DSK les tailles non standard),
plutôt que de déduire un espacement uniforme d'une constante globale. Le
temps d'attente d'un Read ID deviendrait alors « temps jusqu'à ce que le
prochain identifiant passe sous la tête », sans paramètre à régler, et le
test de Discology cesserait d'être un équilibre instable.

Tant qu'aucun autre logiciel n'en souffre, ça reste un chantier de finition
(V3), pas un correctif urgent : la copie fonctionne, et le réglage actuel est
verrouillé par le test de bout en bout.

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

## Ce qui reste

Les secteurs à marque « Deleted Data » (la protection de cette disquette :
pistes 9 à 17, secteurs 0x31-0x33 de 4 Ko) ne sont PAS reproduits. Deux
raisons, toutes deux dans `fdc.rs` :

- Read Data (0x06) ne voit pas du tout un secteur à marque « deleted », alors
  qu'un vrai µPD765A le lit quand même (bit SK à 0), signale la marque via le
  bit 6 de ST2 (Control Mark) et s'arrête après ce secteur ;
- Write Deleted Data (0x09) n'est pas implémentée : même relus, ces secteurs
  seraient réécrits avec une marque normale.

Une copie faite sous l'émulateur est donc fidèle pour une disquette ordinaire,
mais perd la protection de celle-ci. Le comportement actuel avait été retenu
pour Teenage Mutant Hero Turtles : le corriger demande de retester ce jeu.
