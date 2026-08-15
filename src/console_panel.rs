//! Panneau console (F11), superposé à l'image émulée par `renderer.rs`
//! (Plan V2.md, jalon M2) : remplace la dépendance à un terminal externe pour
//! saisir les commandes (`disk`, `tape`, `pc`, `b`, `t`...), jusqu'ici
//! réservées au fil `stdin` de `console.rs`.
//!
//! Ce module ne connaît rien à wgpu ni à SDL2 : il construit juste l'UI
//! egui (`ui`) et pousse les commandes saisies sur le même canal
//! `MonitorCmd` que le fil `stdin` — `Machine` ne voit donc aucune
//! différence entre une commande tapée au clavier physique et une tapée ici.

use crate::monitor::{MonitorMessage, parse_command};
use std::sync::mpsc::Sender;

pub struct ConsolePanel {
    /// Lignes déjà affichées : commandes tapées (préfixées "> ") et sortie
    /// produite par `Machine::console_handle`.
    history: Vec<String>,
    input: String,
    /// Le focus doit être redemandé à chaque ouverture (F11) : `egui` ne le
    /// retient pas d'une trame sur l'autre quand le panneau est reconstruit.
    request_focus: bool,
}

impl ConsolePanel {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            input: String::new(),
            request_focus: true,
        }
    }

    /// À appeler quand le panneau redevient visible (F11), pour que la
    /// ligne de saisie récupère le focus clavier sans clic préalable.
    pub fn request_focus(&mut self) {
        self.request_focus = true;
    }

    /// Ajoute la sortie d'une commande déjà traitée (voir
    /// `Machine::console_handle`) à l'historique affiché. Les sorties vides
    /// (commandes qui ne produisent rien, comme `j` ou `pc`) n'ajoutent pas
    /// de ligne creuse.
    pub fn push_output(&mut self, output: &str) {
        for line in output.lines() {
            self.history.push(line.to_string());
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, cmd_sender: &Sender<MonitorMessage>) {
        egui::TopBottomPanel::bottom("console_panel")
            .resizable(true)
            .default_height(240.0)
            .frame(
                egui::Frame::default()
                    .fill(egui::Color32::from_rgba_unmultiplied(15, 15, 25, 235))
                    .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(">").monospace().color(egui::Color32::from_rgb(220, 220, 225)));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.input)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace)
                            .hint_text("commande (h pour l'aide)"),
                    );
                    if self.request_focus {
                        response.request_focus();
                        self.request_focus = false;
                    }
                    let submitted = response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if submitted {
                        let line = std::mem::take(&mut self.input);
                        if !line.trim().is_empty() {
                            self.history.push(format!("> {line}"));
                            let _ = cmd_sender.send(parse_command(&line));
                        }
                        // Redemande le focus : sans ça, ENTRÉE le fait perdre
                        // et la commande suivante exigerait un clic.
                        self.request_focus = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.history {
                            ui.label(
                                egui::RichText::new(line)
                                    .monospace()
                                    .color(egui::Color32::from_rgb(220, 220, 225)),
                            );
                        }
                    });
            });
    }
}
