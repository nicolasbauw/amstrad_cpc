# Plan V3 — ByteBox, on peaufine les détails

> Le « V3 » compte les ITÉRATIONS de ce projet (V1 : l'émulateur lui-même ;
> V2 : l'interface, voir `doc/Plan V2.md` ; V3 : ce qui suit), pas le numéro
> de version de ByteBox — qui suit sa propre logique semver, sans rapport.
> La lecture des `.SNA` ci-dessous a par exemple donné la 2.1.0.

Sept points connus et documentés, VOLONTAIREMENT LAISSÉS EN L'ÉTAT depuis la
V1 (cinq désormais résolus — points 1, 3, 4, 5 et 7 — et un sixième,
le 2, refermé comme impasse démontrée) : aucun ne nuisait au
fonctionnement (tous les logiciels du premier batch tournaient déjà), ce sont
des approximations acceptées dont on connaît la limite. C'est une liste de
référence, pas un jalonnage figé : chaque point est indépendant des autres et
peut être traité dans n'importe quel ordre, ou laissé de côté indéfiniment si
rien ne le réclame.

## 1) RÉSOLU — FDC — écriture par secteur au lieu de l'image entière

Détails dans `doc/persistance-disquette.md`. Chaque écriture de secteur
réécrivait tout le fichier `.dsk` (160 Ko pour Discology.dsk). Négligeable
pour un `SAVE` BASIC ponctuel, sensible pour un formatage piste par piste ou
un jeu qui journalise sa progression.

`Fdc::persist_sector` écrit désormais les 512 octets à leur offset exact
(`seek` + `write`), plus l'octet ST2 du descripteur : ~320 fois moins
d'octets écrits par secteur. L'offset se calcule depuis le FICHIER (les deux
formats rangent les pistes différemment, et une image Extended le reste tant
qu'on ne l'a pas réécrite en entier), avec repli systématique sur la
réécriture complète dès que la géométrie ne correspond pas — c'est ce repli
qui rend le correctif sans risque. Format Track continue de réécrire tout,
puisqu'il change la structure des pistes.

## 2) IMPASSE DÉMONTRÉE — FDC — vrai modèle de rotation

L'espacement des secteurs vient d'une constante globale empirique
(`SECTOR_OVERHEAD_BYTES` = 100, dans `fdc.rs`), non monotone et sans plage
stable, qui a déjà dû être recalée quand le temps CPU a été corrigé.

Le correctif envisagé — donner à chaque secteur sa position angulaire réelle,
lue dans l'image `.dsk` — **a été instrumenté et invalidé**. Mesures
détaillées dans `doc/discology-copie.md` ; en résumé :

- la valeur physiquement exacte (144) casse la copie, aujourd'hui encore ;
- le champ GAP#3 du `.dsk` n'est pas exploitable : `Discology.dsk` déclare 78
  partout, ce qui donnerait 6520 octets sur une piste qui n'en contient
  que 6250 — la géométrie déclarée ne rentre pas dans un tour ;
- le seul modèle sans paramètre ("une piste = un tour") échoue aussi ;
- le temps CPU est hors de cause : notre boucle de sondage vaut exactement
  celle de Caprice32, opcode par opcode ;
- Caprice32 ne peut pas servir de référence : il n'a **aucun** modèle de
  rotation (Read ID instantané, simple compteur d'index).

La constante compense en réalité un budget angulaire trop court côté
Discology, dont la cause reste inconnue. Reprendre ce point supposerait de
désassembler sa routine de seuil (compteur en `103E`) — chantier ouvert,
sans garantie, et sans urgence tant qu'aucun autre logiciel n'en souffre.

## 3) RÉSOLU — FDC — marques "Deleted Data"

Détails dans `doc/discology-copie.md`. Read Data (0x06) ignorait complètement
un secteur "deleted" (donc se comportait toujours comme SK=1), et Write
Deleted Data (0x09) n'existait pas : une copie perdait la protection des
disquettes qui s'en servent.

Le bit SK est désormais respecté dans les deux sens : SK=1 saute le secteur
à la marque inattendue, SK=0 le lit quand même, lève Control Mark (bit 6 de
ST2) et arrête la commande après lui. Write Deleted Data partage le chemin
de Write Data, seule la marque posée diffère. Troisième correctif découvert
en chemin : `write_dsk_file` n'écrivait pas ST2, si bien que la marque se
serait perdue à la persistance — annulant les deux autres.

Le risque annoncé (Teenage Mutant Hero Turtles, dont la protection repose
sur ces marques) a été vérifié : le jeu atteint son écran de titre comme
avant.

## 4) RÉSOLU — Vidéo — capture de la VRAM par caractère plutôt que par ligne

Détails dans `doc/sprite-flicker.md`. Nous capturions les octets d'une ligne
au moment où elle commençait à être balayée ; le CRTC, lui, les lit au fil de
la ligne (deux octets par position de caractère). Une écriture survenant en
milieu de ligne n'était donc pas reflétée sur sa moitié droite.

La capture suit désormais le faisceau : `Machine::capture_beam_progress`
convertit la position dans la scanline (`hsync_accumulator`) en position de
caractère, et `video::capture_scanline_chars` n'ajoute que les positions
nouvellement franchies. La conversion utilise la géométrie réellement
programmée (`R0 + 1` caractères par ligne), pas une constante, pour rester
juste quand un logiciel reprogramme R0 en cours de trame.

Aucun symptôme ne le réclamait — les deux jeux témoins (Cauldron, BMX
Simulator) étaient déjà à zéro pixel oscillant et le restent ; c'est de la
fidélité de principe. Vérifié en plus par un test dédié : une écriture faite
alors que le faisceau est au milieu de la ligne ne se voit que sur les
positions pas encore balayées.

## 5) RÉSOLU — Vidéo — la vidéo doit lire les 64 premiers Ko, pas la vue bankée du Z80

`video.rs` passait par `Memory::read_ram_byte`, qui suit la commutation de
banques du Z80. Sur un vrai 6128, le circuit vidéo va toujours chercher
l'image dans les 64 premiers Ko, quelle que soit la configuration &7Fxx : un
logiciel qui bascule en &C2 pour se faire un tampon de 64 Ko continue
d'afficher l'écran resté en banque 3.

Un premier essai avait été retiré faute de logiciel pour le démontrer
(Discology n'y est pas sensible) — traité sans attendre un cas de test réel,
jugé suffisamment important pour la fidélité de l'émulation : nouvelle
méthode `Memory::read_video_ram_byte` (toujours banques 0-3, indépendante de
`ram_config`/`extended_page1_bank`), utilisée par `video.rs` à la place de
`read_ram_byte`, qui garde son comportement banké pour son autre usage (le
débogueur, commande `ReadMem`). Vérifié : Discology et BMX Simulator (les
deux tests d'intégration les plus sensibles à la VRAM) toujours au vert.

## 6) Clavier — "@" inatteignable, et c'est normal

Détails dans `doc/clavier-mac-azerty.md`. Rien à corriger : le caractère
n'existe pas dans la police de cette ROM AZERTY (`CHR$(64)` y dessine "à"),
mais la touche émet bien le code 64. Noté ici seulement pour éviter qu'on le
rouvre comme un bug.

## 7) RÉSOLU — Son — amplitude du sifflement cassette non paramétrable

`TAPE_AMPLITUDE` (`sound.rs`) était figée à 0.10. Réglée en passant pendant
le jalon M3 de la V2, qui construisait justement le panneau F6 : la
constante n'est plus que la valeur par défaut de `Sound::tape_amplitude`,
modifiable à chaud par la commande console `tapevol <0-100>` comme par un
curseur du panneau F6.

## RÉSOLU — Lecture des snapshots .SNA (interfaçage RASM)

`core/src/snapshot.rs` ne savait qu'ÉCRIRE un `.SNA` — décision d'origine
assumée ("un format à moitié relu serait un piège"), levée par un usage qui
justifiait de le faire correctement : RASM assemble directement vers un
`.SNA` prêt à tourner, d'où un cycle "assemble, charge, teste" sans image
disque.

`snapshot::load` restaure l'état en rejouant les écritures d'I/O (méthode de
Caprice32), avec deux divergences délibérées : le registre de contrôle du
PPI est écrit EN PREMIER (le configurer remet ses ports à zéro chez nous,
comportement exigé par Barbarian), et le port B n'est pas restauré du tout —
c'est le câblage de la machine (straps constructeur), pas de l'état
programme. Exposé par `snapload` en console et `--snapshot=<fichier>` en
ligne de commande, les instantanés vivant dans `~/.bytebox/SNA`.

Deux limites documentées dans `doc/sna-format.md` : les snapshots v3 à
mémoire compressée (blocs `MEM0`-`MEM8`) sont refusés franchement plutôt que
chargés à moitié — c'est le défaut de RASM, d'où le `-v2` recommandé — et
les champs d'en-tête v2/v3 sont lus sans être appliqués, sauf le modèle de
CPC qui produit un avertissement quand ce n'est pas un 6128.
