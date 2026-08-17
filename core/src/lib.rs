//! Cœur d'émulation de l'Amstrad CPC 6128 : CPU (via la crate `zilog_z80`),
//! périphériques matériels, formats disque/cassette/instantané, et le canal
//! de commandes qui les pilote. Aucune dépendance à SDL2/wgpu/egui — c'est
//! `bytebox` (la coquille de présentation, `../bytebox`) qui s'en charge.
//!
//! Séparé de la coquille de présentation dans un crate à part pour que ce
//! cœur reste réutilisable : l'idée, pour l'instant sans urgence, est de
//! réutiliser la même coquille (fenêtrage, pipeline de rendu wgpu, panneaux
//! egui, console F10/F11) pour un futur émulateur TRS-80, dont le matériel
//! n'a évidemment rien à voir. Cette séparation ne rend donc pas encore ce
//! crate générique (rien n'y définit d'interface abstraite de type
//! `trait Emulator`) — seulement autonome, ce qui est le préalable.

pub mod applog;
pub mod autotype;
pub mod bus;
pub mod config;
pub mod crtc;
pub mod fdc;
pub mod gate_array;
pub mod hexconversion;
pub mod machine;
pub mod memory;
pub mod monitor;
pub mod ppi;
pub mod psg;
pub mod rom_installer;
pub mod snapshot;
pub mod sound;
pub mod tape;
pub mod trace;
pub mod video;
