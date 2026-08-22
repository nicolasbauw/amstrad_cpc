# Un portage WebAssembly, à titre informatif

Question posée sans intention de s'y lancer : quel effort pour une web app
à partir de `core` compilé en WebAssembly ? Est-ce seulement faisable ?

Réponse basée sur un audit réel des dépendances, complété par des faits
établis de l'écosystème Rust/WASM qui n'ont **pas** été vérifiés par une
compilation effective : aucune cible `wasm32-unknown-unknown` n'était
installée sur la machine où cette question a été posée, et l'installer
(paquet `rust-wasm` sur Arch) est une action système, pas quelque chose à
faire sans y être invité pour une question purement informative.

## Verdict : faisable, mais c'est un troisième projet, pas une recompilation

## Le moteur (`core`) est étonnamment proche du portable

Un seul point d'usage de `sdl2` dans tout le crate : `psg.rs` importe
`Keycode`/`Scancode` comme simples types d'énumération pour la matrice
clavier (`set_key_state`/`set_key_state_scancode`), sans aucun appel SDL2
réel — pas de contexte, pas de fenêtrage, juste des valeurs d'enum utilisées
dans des `match`. Et `zilog_z80`, le CPU lui-même, n'a **aucune dépendance
externe** : Rust pur, portable par construction.

Trois autres dépendances de `core` posent en revanche un vrai problème pour
le web :

- **`sdl2`** — la crate ne compile pas pour `wasm32-unknown-unknown` : elle
  lie SDL2 natif en C via FFI (`sdl2-sys`), sans équivalent dans ce
  contexte. Retirer les deux types utilisés serait trivial en soi, et même
  une amélioration d'architecture indépendamment du web — `core` ne
  devrait pas dépendre d'une crate de présentation (`sdl2`) pour commencer.
- **`ureq`** (téléchargement des ROMs, `rom_installer.rs`) — HTTP bloquant
  natif, inutilisable tel quel dans un navigateur ; il faudrait passer par
  `fetch()` via `wasm-bindgen`.
- **`directories`** (chemins `~/.bytebox/...`) — un navigateur n'a pas de
  système de fichiers ; il faudrait `localStorage`/`IndexedDB` ou l'API
  File System Access.

`toml`, `serde`, `zip`, `crc32fast` sont du calcul pur, sans souci a priori.

## La bonne nouvelle : la pile graphique de la V2 s'y prête bien

`bytebox` utilise déjà `wgpu` + `egui` pour le rendu (Plan V2.md) — et les
deux ont un vrai support web : WebGPU/WebGL2 pour `wgpu`, `eframe` comme
coquille officielle web pour `egui`. C'est la pièce la plus dure d'un
portage web, et elle est déjà en place — un choix qui n'avait pas été fait
en pensant au web, mais qui le rend plus abordable après coup qu'avec SDL2
seul.

Le vrai obstacle est **SDL2 pour le fenêtrage et les événements** (clavier,
manette, audio) — rien de tout ça n'existe dans un navigateur. Il faudrait
remplacer cette couche par `eframe` (qui gère nativement le canvas web)
plutôt que par SDL2, avec ses propres bindings clavier/manette/audio (Web
Audio API pour le son, remplaçant `bytebox/src/audio.rs`).

## Effort réaliste

- **Rendre `core` compatible WASM** : modeste, quelques heures à une
  journée. Extraire `Keycode`/`Scancode` de `core` (les faire passer par
  une abstraction ou par des identifiants bruts, la couche de présentation
  faisant la traduction), passer `ureq`/`directories` derrière des feature
  flags conditionnels ou des traits.
- **Bâtir une coquille web** : un vrai projet, pas un portage — écrire une
  **troisième présentation**, à côté de celle SDL2 (V1, toujours en place)
  et de l'interface egui/wgpu desktop (V2), avec son propre fenêtrage
  (`eframe`), sa propre gestion clavier/manette/audio, et un flux de
  téléchargement des ROMs entièrement différent (webesque, avec le
  problème juridique déjà documenté dans `doc/roms-installation.md` qui se
  pose différemment en distribution web — un serveur qui sert des ROMs
  n'a pas la même exposition qu'un installeur qui les télécharge à la
  demande depuis le poste de l'utilisateur).

Comparable en ampleur à la V2 elle-même, pas un week-end — avec son propre
lot de questions non résolues ici : stockage des ROMs dans le navigateur,
performance de l'émulation cycle-exacte en WASM (à mesurer, pas supposée),
distribution/hébergement.

## Pour transformer ce raisonnement en fait constaté

Cette analyse s'appuie sur un audit de dépendances réel (vérifiable,
`grep -rn "sdl2::" core/src/*.rs` ne renvoie qu'un seul fichier) mais pas
sur une compilation effective vers `wasm32-unknown-unknown`. La première
étape concrète, si ce chantier est repris un jour, serait d'installer cette
cible (`rust-wasm` sur Arch, ou via `rustup target add
wasm32-unknown-unknown`) et de tenter une compilation de `core` telle
quelle, pour voir exactement où `sdl2` fait échouer le lien — un point de
départ empirique plutôt qu'une nouvelle supposition.
