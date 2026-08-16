//! Facteur d'agrandissement du contenu des panneaux egui superposés à la
//! fenêtre principale (F6, F10 — pas F11/F12, fenêtres SDL2 séparées avec
//! leur propre contexte egui, voir `egui_gpu.rs`), en fonction du niveau de
//! zoom courant (F1-F4).
//!
//! Partagé plutôt que dupliqué dans chaque panneau : `scaled_style` fait une
//! quinzaine de lignes, et la formule de `content_scale` doit rester
//! identique pour que F6 et F10 grossissent au même rythme.

/// `scale = 1.0` en x1 (taille d'origine, inchangée), jusqu'à 2.5x au-delà
/// (x2 et plus) — au-delà, le texte deviendrait disproportionné même sur un
/// grand écran.
pub fn content_scale(window_size: egui::Vec2) -> f32 {
    (window_size.x / 800.0).clamp(1.0, 2.5)
}

/// Grossit tout ce qui détermine la taille visuelle du contenu — tailles de
/// police et espacements — plutôt que la seule largeur d'un panneau.
/// `egui::Context::set_pixels_per_point` ferait la même chose, mais pour
/// TOUS les panneaux en même temps (F6/F7/F10 partagent le même contexte
/// egui, voir `renderer.rs`) : une bascule propre à un seul panneau passe
/// donc par le style de son propre `Ui`, qui ne fuit pas vers les autres.
pub fn scaled_style(base: &egui::Style, scale: f32) -> egui::Style {
    let mut style = base.clone();
    for font_id in style.text_styles.values_mut() {
        font_id.size *= scale;
    }
    let spacing = &mut style.spacing;
    spacing.item_spacing *= scale;
    spacing.button_padding *= scale;
    spacing.interact_size *= scale;
    spacing.slider_width *= scale;
    spacing.combo_width *= scale;
    spacing.text_edit_width *= scale;
    spacing.icon_width *= scale;
    spacing.icon_width_inner *= scale;
    spacing.icon_spacing *= scale;
    spacing.indent *= scale;
    style
}
