# Plan V3 — ByteBox, on peaufine les détails

> Le « V3 » compte les ITÉRATIONS de ce projet (V1 : l'émulateur lui-même ;
> V2 : l'interface, voir `doc/Plan V2.md` ; V3 : ce qui suit), pas le numéro
> de version de ByteBox — qui suit sa propre logique semver, sans rapport.
> La lecture des `.SNA` ci-dessous a par exemple donné la 2.1.0.

Sept points connus et documentés, VOLONTAIREMENT LAISSÉS EN L'ÉTAT depuis la
V1 (trois désormais résolus, voir les points 4, 5 et 7) : aucun ne nuisait au
fonctionnement (tous les logiciels du premier batch tournaient déjà), ce sont
des approximations acceptées dont on connaît la limite. C'est une liste de
référence, pas un jalonnage figé : chaque point est indépendant des autres et
peut être traité dans n'importe quel ordre, ou laissé de côté indéfiniment si
rien ne le réclame.

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

## 3) FDC — marques "Deleted Data"

Détails dans `doc/discology-copie.md`. Read Data (0x06) ignore complètement un
secteur "deleted", alors qu'un vrai µPD765A le lit quand même quand SK=0,
signale la marque par le bit 6 de ST2 et s'arrête après ce secteur. Et Write
Deleted Data (0x09) n'existe pas. En conséquence une copie perd la protection
des disquettes qui s'en servent.

ATTENTION : le comportement actuel avait été retenu pour Teenage Mutant Hero
Turtles — à retester si on y touche.

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

## Piste ouverte — lecture des snapshots .SNA (interfaçage RASM)

Pas une approximation à corriger comme les sept points ci-dessus : une
capacité qui n'existe pas encore, à étudier.

`core/src/snapshot.rs` sait déjà ÉCRIRE un `.SNA` (utile pour comparer
notre état exact avec un autre émulateur), mais explicitement pas le LIRE —
décision prise à l'époque pour éviter "un format à moitié relu", vu son
absence d'utilité immédiate. Elle en gagnerait une : RASM (l'assembleur Z80/
CPC de Roudoudou, largement utilisé dans la scène) sait produire directement
un `.SNA` prêt à l'emploi à partir du code assemblé, RAM et PC d'entrée déjà
en place — un cycle "assemble avec RASM, charge direct dans ByteBox" sans
repasser par une image disque/cassette, précieux pour du développement Z80
rapide.

À étudier avant de coder quoi que ce soit :
- quelles versions du format (v1/v2/v3 — tailles d'en-tête et de RAM
  différentes, champs Gate Array/CRTC/PSG plus ou moins complets) RASM
  produit-il réellement, et lesquelles vaut-il la peine de couvrir en
  lecture (pas forcément les trois) ;
- où l'exposer côté ByteBox : un `--snapshot=<fichier.sna>` en ligne de
  commande (même esprit que `--disk`/`--tape`, `main.rs`), une commande
  console dédiée, ou un onglet du panneau F6 — à trancher selon l'usage
  réel visé (dev Z80 au lancement vs charger un snapshot en cours de
  session) ;
- restaurer un état COMPLET (registres, RAM, configuration Gate Array/CRTC/
  PPI/PSG/FDC) suppose de repasser la machine par un chemin d'initialisation
  différent du power-on habituel — à concevoir avec soin plutôt qu'en
  pièces détachées au fil des champs du format.
