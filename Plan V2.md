# Plan V2 — ByteBox, de l'outil de développeur à l'émulateur présentable

Ce document répond à `TODO v2.txt` : il tranche les choix d'architecture
qu'il posait en questions, et découpe le résultat en jalons vérifiables
indépendamment. Rien n'est codé ici — c'est la V1 (chasse aux bugs) qui reste
la priorité immédiate, ce plan sert de référence pour quand la V2 démarrera.

## Vision (rappel)

Un émulateur qui se lance depuis le bureau, sans terminal, avec une
interface simple pour le joueur occasionnel — et qui garde intactes les
fonctions actuelles (console de commandes, écran "machine status") pour
l'usage avancé. Plus un shader CRT réaliste et discret. Le tout doit
distinguer ByteBox des autres émulateurs CPC, pas juste les imiter.

## Décision d'architecture : SDL2 + wgpu + egui, pas Qt

### Les deux questions posées sur Qt

**Peut-on garder SDL pour les entrées (clavier, manette) avec une fenêtre
Qt ?** Techniquement oui, en pompant les événements SDL depuis un timer Qt
en parallèle de la boucle d'événements Qt — mais ça revient à faire
cohabiter deux boucles d'événements et deux couches d'abstraction clavier
dans le même process. Le mapping clavier actuel (`psg.rs`) est déjà décrit
comme "100% parfait" pour l'usage réel (clavier Mac sous Linux) : le risque
de ce chantier est justement de le fragiliser sans bénéfice, pour un
problème (l'intégration OS) que Qt ne résout pas à cet endroit-là.

**Peut-on intégrer un rendu Vulkan dans une fenêtre Qt ?** Oui,
`QVulkanWindow` (ou un `QWindow` en mode `VulkanSurface`) est un mécanisme
Qt officiel et documenté. Mais aucun exemple établi ne le fait cohabiter
avec SDL pour la gestion des entrées — ce serait deux runtimes graphiques
concurrents dans le même process, un terrain à défricher soi-même plutôt
qu'une voie balisée.

### Ce qui est retenu à la place

| Rôle | Choix | Statut vérifié |
|---|---|---|
| Fenêtrage, entrées clavier/manette | **SDL2** (inchangé) | déjà en place, mapping validé |
| Rendu GPU (remplace le `Canvas` logiciel actuel) | **wgpu** | `rust-sdl2` fournit un exemple officiel de rendu wgpu dans une fenêtre SDL2 via `raw-window-handle` — la combinaison est balisée, contrairement à Qt+SDL |
| Widgets (panneaux config, clavier virtuel, console, machine status) | **egui**, via `egui_sdl2_platform` (ou `egui-sdl2-event`) + `egui-wgpu` | crates actifs, mis à jour récemment, conçus précisément pour ce trio SDL2+wgpu+egui |
| Sélecteur de fichier natif | **rfd** | multiplateforme, 100 % natif (GTK ou portail XDG sous Linux, Cocoa, Win32), répond directement à la question subsidiaire sur le sélecteur de fichier |
| Console de commandes | panneau **egui** custom, pas libghostty-rs | `libghostty-rs` est pré-1.0, API instable, changements cassants attendus — disproportionné pour une poignée de commandes texte sans besoin d'émulation de terminal réelle (couleurs ANSI, redimensionnement, etc.) |

### Pourquoi ce choix plutôt que Qt

- **Un seul process, une seule boucle d'événements.** Le clavier reste géré
  exactement comme aujourd'hui ; rien de ce qui marche déjà n'est remis en jeu.
- **Le rendu wgpu remplace juste le blit logiciel actuel**, dans la même
  fenêtre SDL2 : la bascule est un changement de moteur de rendu, pas une
  réécriture de l'application.
- **Pas de dépendance système lourde.** Qt (via `cxx-qt`) demande un
  toolchain C++ et les bibliothèques Qt installées ou embarquées (plusieurs
  dizaines de Mo) — à l'opposé de l'objectif de packaging léger (AUR) déjà
  sur le TODO.
- **La distinction visuelle** que Qt est censé apporter s'obtient tout autant
  (mieux, même) par une interface egui à l'identité graphique propre à
  ByteBox, plutôt que des widgets Qt génériques qui ressemblent à n'importe
  quelle appli Qt.
- **Le shader CRT** devient plus simple qu'avec Qt : wgpu choisit Vulkan en
  priorité sous Linux (Metal sous macOS, DX12 sous Windows), donc l'esprit
  de la demande ("Vulkan") est respecté, avec repli automatique propre sur
  les configurations sans Vulkan.

Ce choix n'exclut pas de revisiter Qt plus tard si l'un des jalons ci-dessous
révèle un besoin que egui ne couvre pas — mais rien dans l'état actuel du
projet ne le justifie.

## Ce qui ne change pas

Tout le cœur émulé (`machine.rs`, `bus.rs`, `fdc.rs`, `tape.rs`, `psg.rs`,
`video.rs`, `sound.rs`...) est hors périmètre : ce plan touche uniquement la
couche de présentation (`sdl.rs` et les nouveaux modules qu'il décrit). Les
118 tests existants, qui pilotent `Machine` directement sans passer par
`sdl.rs`, ne sont pas concernés par ce chantier.

## Trois catégories d'état, trois façons d'y accéder

Cette distinction existe déjà dans le code, sans être documentée nulle
part ; elle doit guider M1, M2 et M3 pour éviter que chaque nouveau panneau
n'invente sa propre façon de parler à `Machine`.

**Config statique (`config.toml`)** — lue une seule fois, à `Machine::new()`.
Pas seulement "n'évolue pas en cours d'exécution" : elle n'est même jamais
relue depuis le disque une fois l'émulateur démarré, il n'existe aucune
commande de rechargement. Un `power_cycle()` réutilise la `Config` déjà
chargée en mémoire, il ne retouche pas au fichier. C'est l'endroit pour tout
ce qui façonne la construction des structures (chemins de ROM,
`extra_ram_banks` qui dimensionne le `Vec` de `Memory`).

**État machine dynamique, via `MonitorCmd`/mpsc** — disquettes, cassette,
power cycle, breakpoints, trace, volume : déjà le canal unique aujourd'hui
(fil `console.rs` → `Machine::console_handle`). **Toute future façade (F6,
F11) doit pousser dans ce même canal**, plutôt que d'appeler les méthodes
`Machine` directement — sinon deux implémentations de "insérer une
disquette" (celle du texte, celle des boutons) finissent par diverger. Le
fil `console.rs` disparaît ou change de forme (M2), mais le canal et
l'énumération `MonitorCmd` restent l'unique point d'entrée.

**État de présentation, jamais vu par `Machine`** — existe déjà aussi : F1-F4
(zoom) agissent directement sur le `Canvas` SDL dans `sdl.rs`, sans passer
par le canal, parce que le zoom n'est pas un état de la machine émulée mais
de la fenêtre. F5 (shader CRT on/off) et la visibilité des panneaux
F6/F7/F11/F12 sont du même ordre : état local du futur module de
présentation, pas une `MonitorCmd`.

Deux cas ne rentrent proprement dans aucune des trois catégories
aujourd'hui, et devront être tranchés explicitement au moment de M3 :
- **`extra_ram_banks`** dimensionne `Memory` à la construction ; le rendre
  réglable depuis F6 ne peut pas être un effet instantané — la valeur
  devra être mise en attente et appliquée au prochain power cycle.
- **`font_path`** n'est consommé que par `sdl.rs` pour la police du
  debugger, jamais vu par `Machine` : il reste "état de présentation", pas
  une `MonitorCmd`.
- À l'inverse, si l'amplitude du signal cassette dans le mixage audio
  (`sound.rs`, `TAPE_AMPLITUDE`, citée dans le TODO v1 comme "à rendre
  paramétrable") devient réglable depuis F6, elle mute un état réellement
  possédé par `Machine`/`Sound` : elle doit suivre le même modèle que
  `Volume`, donc devenir une `MonitorCmd`.

## Jalons

Chaque jalon est utilisable indépendamment (rien ne casse si le suivant
n'est jamais fait) et se vérifie concrètement — en s'appuyant sur le banc
d'essai headless déjà éprouvé (`Machine::step` piloté sans fenêtre, capture
PNG via `video::render`, voir la note mémoire correspondante) pour tout ce
qui touche au rendu.

### M0 — Bascule du rendu vers wgpu, sans rien changer d'autre

Remplacer le `Canvas`/texture streaming logiciel de `sdl.rs` par un pipeline
wgpu qui dessine dans la même fenêtre SDL2 (surface créée via
`raw-window-handle`, suivant l'exemple officiel de `rust-sdl2`). Aucune
fonctionnalité nouvelle : le rendu doit être visuellement identique à
aujourd'hui (même letterboxing/pillarboxing x1/x2/x3/plein écran — à
recoder manuellement, `set_logical_size` de SDL2 ne s'appliquant plus).

**Vérification** : comparaison des captures PNG du banc d'essai headless
avant/après, sur plusieurs jeux et zooms. C'est le jalon qui porte le plus
de risque technique (nouvelle dépendance graphique) : il doit être validé
avant d'investir dans tout le reste, qui en dépend.

### M1 — egui au-dessus de wgpu, sur un cas réel et déjà existant

Intégrer `egui_sdl2_platform` + `egui-wgpu`, et migrer la fenêtre "machine
status" actuelle (texte dessiné à la main avec `SDL_ttf`, dans une fenêtre
SDL2 séparée) vers un panneau egui. C'est un remplacement à périmètre
fonctionnel identique — donc un test réel et utile du pipeline de widgets,
avant de s'en servir pour des écrans qui n'existent pas encore.

**Vérification** : la fenêtre "machine status" (F12) affiche les mêmes
informations qu'avant, à jour à chaque trame.

### M2 — Console de commandes intégrée (F11), fin de la dépendance au terminal

Remplacer le fil `console.rs` (bloqué sur `stdin().read_line()`, qui tourne
en boucle chaude sans jamais bloquer quand l'émulateur est lancé sans
terminal — c'est la cause du CPU qui s'emballe, `read_line` renvoie `Ok(0)`
sur EOF plutôt qu'une erreur) par un panneau egui (F11) : zone de
défilement + ligne de saisie, qui alimente le même canal `MonitorCmd` que le
fil actuel. Le code de correspondance texte → `MonitorCmd`
(`console.rs::launch`) est à extraire dans une fonction partagée
(`monitor.rs`, par exemple `parse_command(line) -> (MonitorCmd, String,
String)`), réutilisée par le panneau F11. Le fil `stdin` d'origine peut soit
disparaître, soit être conservé en option pour l'usage en ligne de commande
pure (`--headless`, scripts) — à trancher au moment venu.

**Vérification** : lancer l'émulateur sans terminal (double-clic / .desktop)
et constater que le CPU hôte reste au repos ; toutes les commandes console
existantes (`disk`, `tape`, `pc`, `b`, `t`...) fonctionnent depuis le
panneau F11.

### M3 — Fenêtre de configuration/médias (F6)

Panneau egui reprenant ce qui est aujourd'hui dans `config.toml` et les
commandes console `disk`/`tape`/`blank` : insertion/éjection disquette A/B,
cassette, activation du lecteur B, zoom par défaut, banques RAM
supplémentaires, chemin de police, volume, et (repris du TODO v1) l'amplitude
du signal cassette dans le mixage audio. Sélection de fichier via `rfd`
(répond à la question subsidiaire : oui, agnostique du système, natif GTK
sous Linux).

**Vérification** : chaque champ modifié dans le panneau a un effet immédiat
identique à la commande console ou au réglage `config.toml` équivalent.

### M4 — Shader CRT (F5)

Portage en WGSL d'un shader CRT de référence déjà éprouvé (famille
Lottes/"easymode", scanlines + arrondi des pixels, sans distorsion en
barillet — non demandée). Le pitch des scanlines et l'arrondi des pixels
doivent se calculer à partir de la résolution de sortie réelle (uniforme
dérivée de la taille de fenêtre) plutôt que de constantes en pixels, pour un
rendu identique en x1/x2/x3/plein écran, comme demandé. Bascule F5,
repli sur le rendu actuel (pixel net) quand désactivé.

**Vérification** : captures PNG comparées à x1/x2/x3/plein écran, mêmes
proportions de scanlines dans les quatre cas.

### M5 — Clavier virtuel (F7)

Panneau egui affichant l'illustration stylisée du clavier 6128 AZERTY (à
préparer séparément, cf. note "Prompt clavier" du TODO v1). Chaque touche est
une zone cliquable mappée **directement** sur une position `(ligne, bit)` de
la matrice PSG (`psg.rs`), exactement comme le fait déjà `autotype.rs` pour
la frappe automatique — aucune couche de keymap supplémentaire, conformément
au refus explicite d'aller vers "le délire des keymaps".

**Vérification** : cliquer une touche du clavier virtuel produit le même
effet que la touche physique correspondante (test croisé avec les tests
`autotype` existants, qui connaissent déjà cette correspondance).

### Hors périmètre : les finitions de la V3

Plusieurs approximations connues du cœur émulé (écriture disquette par image
entière plutôt que par secteur, modèle de rotation du FDC, marques "Deleted
Data", capture VRAM par ligne plutôt que par caractère, lecture vidéo à
travers la commutation de banques) sont volontairement laissées en l'état :
aucune ne nuit au fonctionnement, elles sont documentées et regroupées dans
`Plan V3.md`. Elles ne font pas partie de ce plan, qui
ne touche que la couche de présentation — à une exception près, signalée
là-bas : l'amplitude du sifflement cassette a sa place dans la fenêtre de
configuration du jalon M3.

### M6 — Packaging (indépendant, peut se faire à tout moment)

PKGBUILD AUR, et correction des chemins en dur dans
`packaging/bytebox.desktop` (`Exec`/`Path` pointent actuellement vers
`~/Dev/amstrad_cpc`). Sans dépendance sur les jalons précédents ; peut être
traité dès que souhaité, y compris avant eux.

Un unique workflow GitHub Actions (`release.yml`, déclenché sur un tag),
matrice par OS, peut produire tous les formats retenus — GitHub fournit de
vrais runners Linux/Windows/macOS, donc pas de cross-compile nécessaire.

**Formats retenus, par ordre de priorité (effort/bénéfice) :**

1. **AUR** (`PKGBUILD`) — déjà acté.
2. **`.deb`** via `cargo-deb` — lit directement `Cargo.toml`, effort faible.
3. **AppImage** via `cargo-appimage` (ou `linuxdeploy` + `appimagetool` pour
   plus de contrôle) — un seul fichier, tourne sur n'importe quelle distro
   sans gestionnaire de paquets. Runner `ubuntu-22.04` minimum (une `glib`
   trop ancienne sur 20.04 fait échouer `linuxdeploy`).
4. **Windows `.msi`** via `cargo-wix` (WiX Toolset, déjà présent sur le
   runner `windows-latest`) — il faudra embarquer `SDL2.dll` /
   `SDL2_ttf.dll` à côté de l'exécutable dans l'installeur. Non signé :
   déclenchera l'avertissement SmartScreen, assumé (pas de coût à engager
   pour l'éviter).
5. **macOS via Homebrew** (formule/tap `bytebox`, pas un `.dmg`) — Homebrew
   gère la dépendance SDL2 nativement (`depends_on "sdl2"`), donc pas de
   `.dylib` à embarquer, et pas de question de notarisation Apple (le
   binaire est construit localement par `brew`, pas téléchargé puis
   double-cliqué) : ni le coût du compte Developer (99 $/an), ni
   l'avertissement Gatekeeper.

**Écartés délibérément :**
- **`.rpm`** — pas de cible Fedora/openSUSE visée, pas la peine d'ajouter
  `cargo-generate-rpm` à la matrice pour l'instant.
- **Flatpak** — build sandboxé sans réseau (vendoring de tout `Cargo.lock`
  via `flatpak-cargo-generator`), et son intérêt réel (visibilité) suppose
  une soumission à Flathub dans un dépôt séparé ; rapport effort/bénéfice
  mauvais pour ce projet, et format non désiré de toute façon.
- **`.dmg` autonome** — remplacé par la formule Homebrew ci-dessus, qui
  évite tout le sujet de la notarisation.

## Risques identifiés

- **M0** est le seul jalon à risque architectural réel (nouvelle dépendance
  graphique, comportement multiplateforme de wgpu à valider sur la machine
  de développement). Tout le reste (M1-M5) est de la construction de
  widgets sur une fondation déjà validée.
- **`sdl2` (0.38.0, déjà en dépendance) doit exposer le feature
  `raw-window-handle`** — à vérifier en tout début de M0, avant d'investir
  plus loin.
- Les crates egui-sdl2 citées ici sont multiples et proches (`egui-sdl2`,
  `egui-sdl2-event`, `egui_sdl2_platform`) : le choix précis entre elles est
  un détail d'implémentation à trancher au démarrage de M1, pas une décision
  d'architecture.
