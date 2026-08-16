//! Panneau de configuration/médias (F6), superposé à l'image émulée par
//! `renderer.rs`, sur le même mécanisme que la barre de commande rapide
//! (F10, `console_panel.rs`) : les deux partagent le contexte wgpu de la
//! fenêtre principale (Plan V2.md, jalon M3).
//!
//! Reprend, derrière des champs qu'on peut cliquer plutôt que taper, ce qui
//! ne vivait jusqu'ici que dans `config.toml` ou les commandes console
//! (`disk`, `tape`, `blank`, `driveb`, `ram`, `vol`, `tapevol`). Chaque
//! champ pousse sur le même canal `MonitorCmd` que la console — `Machine`
//! ne voit aucune différence entre les deux façades.

use crate::renderer::CrtSettings;
use bytebox_core::app_log;
use bytebox_core::machine::Machine;
use bytebox_core::monitor::{MonitorCmd, MonitorMessage};
use std::sync::mpsc::Sender;

/// Niveau de zoom demandé depuis le panneau. Le zoom est un état de
/// présentation (la fenêtre SDL2), pas un état de la machine émulée : il ne
/// passe donc pas par `MonitorCmd` comme le reste de ce panneau — c'est
/// `sdl::run` qui l'applique, exactement comme pour F1-F4.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoomChoice {
    X1,
    X2,
    X3,
    Fullscreen,
}

pub struct ConfigPanel {
    /// Nom de fichier pour une nouvelle disquette vierge ("blank") : pure
    /// saisie utilisateur, sans état côté machine dont le repartir — les
    /// autres champs de ce panneau (volume, banques RAM...) n'ont pas besoin
    /// d'un tel champ persistant, ils relisent l'état courant de `Machine`
    /// à chaque trame.
    blank_disk_name: String,
    blank_disk_drive_b: bool,
}

impl ConfigPanel {
    pub fn new() -> Self {
        Self {
            blank_disk_name: String::new(),
            blank_disk_drive_b: false,
        }
    }

    /// Dessine le panneau ; `open` reflète et contrôle sa visibilité (la
    /// petite croix de la fenêtre egui peut la fermer, en plus de F6).
    /// `crt_settings` est l'état courant du shader CRT (relu à chaque trame
    /// depuis `Renderer`, comme `machine` pour l'état de la machine) ;
    /// renvoie le zoom demandé cette trame s'il y en a un, et les réglages
    /// CRT à jour — inchangés si l'utilisateur n'a touché aucun curseur,
    /// donc toujours sûr à réappliquer sans condition (voir `sdl.rs`).
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        machine: &Machine,
        cmd_sender: &Sender<MonitorMessage>,
        open: &mut bool,
        crt_settings: CrtSettings,
    ) -> (Option<ZoomChoice>, CrtSettings) {
        let mut zoom = None;
        let mut crt_settings = crt_settings;
        egui::Window::new("Configuration")
            .open(open)
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.heading("Media");
                self.media_section(ui, machine, cmd_sender);
                ui.separator();
                ui.heading("Hardware");
                Self::hardware_section(ui, machine, cmd_sender);
                ui.separator();
                ui.heading("Display");
                Self::display_section(ui, &mut zoom);
                ui.separator();
                ui.heading("Audio");
                Self::audio_section(ui, machine, cmd_sender);
                ui.separator();
                ui.heading("Shader CRT (F5)");
                Self::crt_section(ui, &mut crt_settings);
            });
        (zoom, crt_settings)
    }

    fn media_section(
        &mut self,
        ui: &mut egui::Ui,
        machine: &Machine,
        cmd_sender: &Sender<MonitorMessage>,
    ) {
        let dsk_path = machine.dsk_path();
        let fdc = machine.bus.fdc.borrow();
        Self::disk_drive_row(
            ui,
            "Drive A",
            &fdc.drive_a.current_filename,
            "",
            dsk_path,
            cmd_sender,
        );
        if fdc.drive_b_enabled {
            Self::disk_drive_row(
                ui,
                "Drive B",
                &fdc.drive_b.current_filename,
                "b",
                dsk_path,
                cmd_sender,
            );
        } else {
            ui.label("Drive B is disabled (see Hardware below).");
        }
        drop(fdc);

        ui.horizontal(|ui| {
            ui.label("New blank disk:");
            ui.text_edit_singleline(&mut self.blank_disk_name);
            ui.checkbox(&mut self.blank_disk_drive_b, "drive B");
            if ui
                .add_enabled(!self.blank_disk_name.is_empty(), egui::Button::new("Create"))
                .clicked()
            {
                let arg2 = if self.blank_disk_drive_b { "b" } else { "" };
                let _ = cmd_sender.send((
                    MonitorCmd::Blank,
                    std::mem::take(&mut self.blank_disk_name),
                    arg2.to_string(),
                ));
            }
        });

        ui.separator();
        let tape = machine.bus.tape.borrow();
        let tape_label = tape.current_filename.as_deref().unwrap_or("(empty)");
        ui.horizontal(|ui| {
            ui.label(format!("Tape: {tape_label}"));
            if ui.button("Insert…").clicked()
                && let Some(path) = Self::file_dialog(machine.dsk_path())
                    .add_filter("Tape image", &["cdt"])
                    .pick_file()
            {
                let _ = cmd_sender.send((
                    MonitorCmd::Tape,
                    path.to_string_lossy().into_owned(),
                    String::new(),
                ));
            }
            if ui
                .add_enabled(tape.current_filename.is_some(), egui::Button::new("Eject"))
                .clicked()
            {
                let _ = cmd_sender.send((MonitorCmd::Tape, "eject".to_string(), String::new()));
            }
        });
    }

    /// Une ligne "Drive A"/"Drive B" : nom du fichier inséré (ou vide),
    /// bouton d'insertion (sélecteur natif) et d'éjection. `drive_arg2` vaut
    /// "" pour le lecteur A, "b" pour le lecteur B — c'est le deuxième
    /// argument attendu par `MonitorCmd::Disk`.
    fn disk_drive_row(
        ui: &mut egui::Ui,
        label: &str,
        current_filename: &str,
        drive_arg2: &str,
        dsk_path: Option<&str>,
        cmd_sender: &Sender<MonitorMessage>,
    ) {
        let loaded = current_filename != "None";
        let shown = if loaded { current_filename } else { "(empty)" };
        ui.horizontal(|ui| {
            ui.label(format!("{label}: {shown}"));
            if ui.button("Insert…").clicked()
                && let Some(path) = Self::file_dialog(dsk_path)
                    .add_filter("Disk image", &["dsk"])
                    .pick_file()
            {
                let _ = cmd_sender.send((
                    MonitorCmd::Disk,
                    path.to_string_lossy().into_owned(),
                    drive_arg2.to_string(),
                ));
            }
            if ui.add_enabled(loaded, egui::Button::new("Eject")).clicked() {
                let _ = cmd_sender.send((
                    MonitorCmd::Disk,
                    "eject".to_string(),
                    drive_arg2.to_string(),
                ));
            }
        });
    }

    /// Sélecteur de fichier natif, ouvert par défaut dans `[file] dsk_path`
    /// (config.toml) plutôt que dans le répertoire courant du processus —
    /// c'est là que vivent les images disque/cassette la plupart du temps.
    /// Absent, `rfd` retombe sur son propre choix par défaut (généralement
    /// le répertoire courant, ou le dernier utilisé).
    fn file_dialog(dsk_path: Option<&str>) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match dsk_path {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        }
    }

    fn hardware_section(
        ui: &mut egui::Ui,
        machine: &Machine,
        cmd_sender: &Sender<MonitorMessage>,
    ) {
        let mut drive_b_enabled = machine.bus.fdc.borrow().drive_b_enabled;
        if ui
            .checkbox(&mut drive_b_enabled, "Enable drive B")
            .changed()
        {
            let arg = if drive_b_enabled { "on" } else { "off" };
            let _ = cmd_sender.send((MonitorCmd::DriveB, arg.to_string(), String::new()));
        }

        let mut diagnostic_mode = machine.diagnostic_mode;
        if ui
            .checkbox(&mut diagnostic_mode, "Diagnostic ROM at slot 0F")
            .changed()
        {
            let arg = if diagnostic_mode { "on" } else { "off" };
            let _ = cmd_sender.send((MonitorCmd::DiagnosticMode, arg.to_string(), String::new()));
        }

        ui.horizontal(|ui| {
            ui.label("Extra RAM banks:");
            let mut banks = machine.extra_ram_banks();
            let response = ui.add(egui::DragValue::new(&mut banks).range(0..=56));
            if response.changed() {
                let _ = cmd_sender.send((
                    MonitorCmd::ExtraRamBanks,
                    banks.to_string(),
                    String::new(),
                ));
            }
        });

        ui.horizontal(|ui| {
            ui.label("Extra RAM banks and the Diagnostic ROM only apply at the next power cycle:");
            if ui.button("Power cycle now").clicked() {
                let _ = cmd_sender.send((MonitorCmd::PowerCycle, String::new(), String::new()));
            }
        });
    }

    fn display_section(ui: &mut egui::Ui, zoom: &mut Option<ZoomChoice>) {
        ui.horizontal(|ui| {
            if ui.button("x1").clicked() {
                *zoom = Some(ZoomChoice::X1);
            }
            if ui.button("x2").clicked() {
                *zoom = Some(ZoomChoice::X2);
            }
            if ui.button("x3").clicked() {
                *zoom = Some(ZoomChoice::X3);
            }
            if ui.button("Fullscreen").clicked() {
                *zoom = Some(ZoomChoice::Fullscreen);
            }
        });
    }

    /// Curseurs pour les six constantes de `renderer_crt.wgsl` — voir ses
    /// commentaires pour ce que chacune contrôle visuellement. Pas de
    /// `MonitorCmd` ici : ce sont des réglages de présentation, propres au
    /// `Renderer` de la fenêtre principale, au même titre que le zoom.
    fn crt_section(ui: &mut egui::Ui, settings: &mut CrtSettings) {
        ui.add(
            egui::Slider::new(&mut settings.mask_cell_px, 0.5..=4.0).text("Mask cell size (px)"),
        );
        ui.add(egui::Slider::new(&mut settings.mask_strength, 0.0..=1.0).text("Mask strength"));
        ui.add(egui::Slider::new(&mut settings.mask_min, 0.0..=1.0).text("Mask min brightness"));
        // Plage étendue jusqu'à 24 : 10 était atteint en butée sans que les
        // scanlines paraissent encore assez marquées — c'était en fait le
        // symptôme d'un bug de période (voir `line_height` dans le shader),
        // mais la marge reste utile maintenant qu'il est corrigé.
        ui.add(
            egui::Slider::new(&mut settings.scanline_beam, 1.0..=24.0).text("Scanline beam width"),
        );
        ui.add(
            egui::Slider::new(&mut settings.scanline_strength, 0.0..=1.0)
                .text("Scanline strength"),
        );
        ui.add(
            egui::Slider::new(&mut settings.beam_bloom, 0.05..=1.0).text("Beam bloom (bright)"),
        );
        ui.add(
            egui::Slider::new(&mut settings.bright_boost, 1.0..=2.5).text("Brightness boost"),
        );
        ui.horizontal(|ui| {
            if ui.button("Reset to defaults").clicked() {
                *settings = CrtSettings::default();
            }
            // Le seul réglage de ce panneau qui survive à la fermeture de
            // l'émulateur : tout le reste est soit un état de la machine
            // (rejoué par config.toml au prochain démarrage), soit un choix
            // de session assumé comme tel (zoom, activation du shader).
            if ui.button("Save to config.toml").clicked() {
                match bytebox_core::config::save_crt_config(&settings.to_config()) {
                    Ok(()) => app_log!("CRT settings saved to config.toml"),
                    Err(e) => app_log!("Could not save CRT settings: {e}"),
                }
            }
        });
    }

    fn audio_section(ui: &mut egui::Ui, machine: &Machine, cmd_sender: &Sender<MonitorMessage>) {
        let mut volume_pct = (machine.volume() * 100.0).round();
        ui.horizontal(|ui| {
            ui.label("Volume:");
            let response =
                ui.add(egui::Slider::new(&mut volume_pct, 0.0..=100.0).suffix(" %"));
            if response.changed() {
                let _ = cmd_sender.send((
                    MonitorCmd::Volume,
                    volume_pct.to_string(),
                    String::new(),
                ));
            }
        });

        let mut tape_pct = (machine.bus.psg.sound.tape_amplitude() * 100.0).round();
        ui.horizontal(|ui| {
            ui.label("Tape signal in mix:");
            let response =
                ui.add(egui::Slider::new(&mut tape_pct, 0.0..=100.0).suffix(" %"));
            if response.changed() {
                let _ = cmd_sender.send((
                    MonitorCmd::TapeAmplitude,
                    tape_pct.to_string(),
                    String::new(),
                ));
            }
        });
    }
}
