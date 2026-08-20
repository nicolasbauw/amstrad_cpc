//! Facteur d'agrandissement du contenu des panneaux egui superposés à la
//! fenêtre principale (F6, F10 — pas F11/F12, fenêtres SDL2 séparées avec
//! leur propre contexte egui, voir `egui_gpu.rs`), en fonction du niveau de
//! zoom courant (F1-F4).
//!
//! Partagé plutôt que dupliqué dans chaque panneau : `scaled_style` fait une
//! quinzaine de lignes, et la formule de `content_scale` doit rester
//! identique pour que F6 et F10 grossissent au même rythme.

/// `scale = 1.0` (taille d'origine, inchangée) tant que la fenêtre CPC
/// reste sous une largeur de 3840px (4K) — pas seulement en x1 : x2/x3 sur
/// un écran Full HD ou 1440p restent en dessous de ce seuil, et n'ont donc
/// pas besoin d'être grossis. Une ancienne version basait le calcul sur
/// 800px, ce qui grossissait déjà fortement le panneau en x2/x3/plein écran
/// sur n'importe quel écran, 4K ou non — proportionné à la taille de la
/// fenêtre en pixels bruts, pas à la résolution réelle de l'écran. Au-delà
/// de 3840px (5K, 6K, 8K...), grossit progressivement jusqu'à 2.5x au
/// maximum, au même rythme qu'avant (un point de facteur par 800px
/// supplémentaires).
pub fn content_scale(window_size: egui::Vec2) -> f32 {
    (1.0 + (window_size.x - 3840.0) / 800.0).clamp(1.0, 2.5)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Full HD et 1440p, même en x3/plein écran (donc la pleine largeur de
    /// l'écran), ne doivent jamais grossir le contenu : la 4K seule justifie
    /// une échelle au-delà de 1.0.
    #[test]
    fn resolutions_below_4k_never_scale() {
        assert_eq!(content_scale(egui::vec2(1920.0, 1080.0)), 1.0);
        assert_eq!(content_scale(egui::vec2(2560.0, 1440.0)), 1.0);
        assert_eq!(content_scale(egui::vec2(3840.0, 2160.0)), 1.0, "exactement 4K : encore 1.0");
    }

    #[test]
    fn above_4k_scales_up_to_the_2_5_cap() {
        assert_eq!(content_scale(egui::vec2(4640.0, 2610.0)), 2.0);
        assert_eq!(content_scale(egui::vec2(5040.0, 2835.0)), 2.5);
        assert_eq!(content_scale(egui::vec2(10000.0, 5625.0)), 2.5, "plafonne, ne depasse jamais 2.5");
    }
}
