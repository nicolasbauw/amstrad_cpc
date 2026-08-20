# Plan V3 — ByteBox, on peaufine les détails

Sept points connus et documentés, VOLONTAIREMENT LAISSÉS EN L'ÉTAT depuis la
V1 (un désormais résolu, voir le point 5) : aucun ne nuisait au fonctionnement
(tous les logiciels du premier batch tournaient déjà), ce sont des
approximations acceptées dont on connaît la limite. Rien n'est codé ici pour
les six qui restent — c'est une liste de référence, pas un jalonnage figé :
chaque point est indépendant des autres et peut être traité dans n'importe
quel ordre, ou laissé de côté indéfiniment si rien ne le réclame.

## 1) FDC — écriture par secteur au lieu de l'image entière

Détails dans `doc/persistance-disquette.md`. Chaque écriture de secteur
réécrit tout le fichier `.dsk` (quelques centaines de Ko). Négligeable pour un
`SAVE` BASIC ponctuel, mais un logiciel qui écrit en continu (formatage piste
par piste, jeu qui journalise sa progression) réécrirait tout à chaque
secteur.

Piste : ouvrir le fichier en lecture-écriture et écrire à l'offset exact du
secteur (seek + write ciblé), ce qui suppose de calculer cet offset selon le
format (Standard vs Extended DSK, taille de piste, position du secteur dans
sa piste).

À FAIRE APRÈS LA V2, comme convenu.

## 2) FDC — vrai modèle de rotation

Détails dans `doc/discology-copie.md`. L'espacement des secteurs vient d'une
constante globale empirique (`SECTOR_OVERHEAD_BYTES` = 100, dans `fdc.rs`),
non monotone et sans plage stable, qui a déjà dû être recalée quand le temps
CPU a été corrigé (voir `doc/sprite-flicker.md`).

Le correctif de fond : donner à chaque secteur sa position angulaire réelle,
lue dans l'image `.dsk`. Supprimerait le paramètre et rendrait le test
Discology robuste.

## 3) FDC — marques "Deleted Data"

Détails dans `doc/discology-copie.md`. Read Data (0x06) ignore complètement un
secteur "deleted", alors qu'un vrai µPD765A le lit quand même quand SK=0,
signale la marque par le bit 6 de ST2 et s'arrête après ce secteur. Et Write
Deleted Data (0x09) n'existe pas. En conséquence une copie perd la protection
des disquettes qui s'en servent.

ATTENTION : le comportement actuel avait été retenu pour Teenage Mutant Hero
Turtles — à retester si on y touche.

## 4) Vidéo — capture de la VRAM par caractère plutôt que par ligne

Détails dans `doc/sprite-flicker.md`. Nous capturons les octets d'une ligne au
moment où elle commence à être balayée ; le CRTC, lui, les lit au fil de la
ligne (deux octets par position de caractère). Une écriture survenant en
milieu de ligne n'est donc pas reflétée sur sa moitié droite.

Per-ligne est déjà bien plus fidèle que per-trame (c'est ce qui a réglé
Cauldron), per-caractère le serait davantage. Aucun symptôme connu ne le
réclame aujourd'hui.

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

## 7) Son — amplitude du sifflement cassette non paramétrable

`TAPE_AMPLITUDE` (`sound.rs`) est figée à 0.10. À exposer dans `config.toml`,
ou dans la fenêtre de configuration F6 prévue par `Plan V2.md` (jalon M3).
