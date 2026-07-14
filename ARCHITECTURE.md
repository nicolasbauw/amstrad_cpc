# Architecture de l'Émulateur Amstrad CPC 6128

## Objectif
Créer un émulateur Amstrad CPC 6128 fonctionnel en Rust.

## Contraintes Techniques
- CPU : Utilisation exclusive de la crate `zilog_z80` (branche `cpc`).
- L'architecture doit implémenter le Bus système du CPC pour interconnecter le CPU avec les 128 Ko de RAM, les ROMs (Système, BASIC, AMSDOS), le Gate Array, le CRTC 6845, et le contrôleur de disquette.

## Structure du Code (Modularité Stricte)
Chaque composant matériel de l'Amstrad CPC doit être isolé dans son propre module Rust (un fichier `.rs` dédié) :
- `src/memory.rs` : Gestion de la mémoire (128 Ko de RAM répartis en 8 banques de 16 Ko, ROM Système, ROM BASIC, ROM AMSDOS).
- `src/gate_array.rs` : Gestion des couleurs, de la palette, des interruptions et de la sélection des banques mémoire RAM/ROM (configuration E/S `0x7F00`).
- `src/crtc.rs` : Émulation du contrôleur vidéo 6845 (synchronisation, timings écran).
- `src/psg.rs` : Émulation du générateur de son AY-3-8910 et gestion du clavier.
- `src/fdc.rs` : Émulation du contrôleur de disquette uPD765A (fichiers .DSK).
- `src/bus.rs` : L'interconnexion centrale qui implémente le Trait de notre CPU.

## Roadmap du projet
- [x] Étape 1 : Analyse de l'interface de la struct `CPU` et du trait `Bus`.
- [x] Étape 2 : Implémentation du système de Memory Banking de base (64 Ko). *(À étendre à 128 Ko via le registre de banking du Gate Array)*
- [x] Étape 3 : Routage des ports d'I/O (Gate Array & CRTC).
- [X] Étape 4 : Boucle d'émulation de base (Fetch/Execute) et timings fins.
- [ ] Étape 5 : Intégration graphique (Rendu VRAM de base via SDL3).
- [ ] Étape 6 : Émulation du PSG (src/psg.rs) pour la gestion du clavier et de l'audio.
- [ ] Étape 7 : Émulation du contrôleur de disquette FDC (src/fdc.rs) pour charger des fichiers d'extension `.DSK`.
- [ ] Étape 8 : Extension du Memory Banking à 128 Ko pour supporter les logiciels spécifiques au 6128.
