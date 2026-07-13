# Architecture de l'Émulateur Amstrad CPC

## Objectif
Créer un émulateur Amstrad CPC 464 fonctionnel en Rust.

## Contraintes Techniques
- CPU : Utilisation exclusive de la crate `zilog_z80` (branche `cpc`).
- L'architecture doit implémenter le Bus système du CPC pour interconnecter le CPU avec la mémoire (Banking), le Gate Array et le CRTC 6845.

## Structure du Code (Modularité Stricte)
Chaque composant matériel de l'Amstrad CPC doit être isolé dans son propre module Rust (un fichier `.rs` dédié). Le rôle du `Bus` sera uniquement d'orchestrer et de faire communiquer ces composants, sans embarquer leur logique interne.
Les fichiers à créer au fur et à mesure :
- `src/memory.rs` : Gestion des 64 Ko de RAM et de la commutation des ROMs (Banking).
- `src/gate_array.rs` : Gestion des couleurs, de la palette, et des interruptions.
- `src/crtc.rs` : Émulation du contrôleur vidéo 6845 (génération des signaux de synchronisation, timings écran).
- `src/psg.rs` : Émulation de la puce sonore AY-3-8910 et lecture du clavier.
- `src/bus.rs` : L'interconnexion centrale qui implémente le Trait de votre CPU.
- `src/tape.rs` : Émulation du lecteur de cassettes (fichiers .CDT / .WAV) et gestion du moteur de lecture.

## Roadmap du projet
- [X] Étape 1 : Analyse de l'interface de la struct `CPU` et du trait `Bus` de la branche `cpc`.
- [X] Étape 1.5 : Boot sur la ROM de diagnostic
- [X] Étape 2 : Implémentation du système de Memory Banking du CPC (16 Ko ROM / 64 Ko RAM).
- [ ] Étape 3 : Routage des ports d'I/O (Gate Array & CRTC).
- [ ] Étape 4 : Boucle d'émulation de base (Fetch/Execute) et timings.
- [ ] Étape 5 : Intégration graphique (Rendu VRAM via SDL3).
