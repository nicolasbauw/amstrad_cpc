/// Table des 27 couleurs physiques réelles de l'Amstrad CPC au format RGB (r, g, b).
pub const CPC_COLORS_RGB: [(u8, u8, u8); 27] = [
    (0, 0, 0),       // 0: Noir
    (0, 0, 128),     // 1: Bleu
    (0, 0, 255),     // 2: Bleu vif
    (128, 0, 0),     // 3: Rouge
    (128, 0, 128),   // 4: Magenta
    (128, 0, 255),   // 5: Mauve
    (255, 0, 0),     // 6: Rouge vif
    (255, 0, 128),   // 7: Rose
    (255, 0, 255),   // 8: Magenta vif
    (0, 128, 0),     // 9: Vert
    (0, 128, 128),   // 10: Cyan
    (0, 128, 255),   // 11: Bleu ciel
    (128, 128, 0),   // 12: Jaune
    (128, 128, 128), // 13: Blanc cassé / Gris
    (128, 128, 255), // 14: Pastel Bleu
    (255, 128, 0),   // 15: Orange
    (255, 128, 128), // 16: Rose
    (255, 128, 255), // 17: Magenta pastel
    (0, 255, 0),     // 18: Vert vif
    (0, 255, 128),   // 19: Vert d'eau
    (0, 255, 255),   // 20: Cyan vif / Turquoise
    (128, 255, 0),   // 21: Vert lime
    (128, 255, 128), // 22: Vert pastel
    (128, 255, 255), // 23: Cyan pastel
    (255, 255, 0),   // 24: Jaune vif
    (255, 255, 128), // 25: Jaune pastel
    (255, 255, 255), // 26: Blanc brillant
];

/// Conversion entre l'index de couleur matériel du Gate Array (0 à 31, soit les
/// valeurs &40 à &5F écrites sur son port) et l'index de la couleur physique.
///
/// Les 27 valeurs canoniques sont celles que le firmware émet ; elles sont
/// reprises telles quelles des constantes de la ROM Amstrad Diagnostics
/// (Colors.asm), et le test `hardware_palette_matches_the_reference_table` les
/// revérifie une par une.
///
/// Cinq codes (&41, &48, &49, &50, &51) ne correspondent à aucune des 27
/// couleurs et ne sont jamais émis par le firmware : ce sont des doublons du
/// Gate Array, marqués ci-dessous, dont la valeur ici est une approximation qui
/// ne repose pas sur la source de référence.
pub const HARDWARE_TO_PHYSICAL: [usize; 32] = [
    13, // &40 Blanc / Gris
    13, // &41 (doublon, non documenté)
    19, // &42 Vert d'eau
    25, // &43 Jaune pastel
    1,  // &44 Bleu
    7,  // &45 Pourpre
    10, // &46 Cyan
    16, // &47 Rose
    7,  // &48 (doublon, non documenté)
    25, // &49 (doublon, non documenté)
    24, // &4A Jaune vif
    26, // &4B Blanc brillant
    6,  // &4C Rouge vif
    8,  // &4D Magenta vif
    15, // &4E Orange
    17, // &4F Magenta pastel
    0,  // &50 (doublon, non documenté)
    2,  // &51 (doublon, non documenté)
    18, // &52 Vert vif
    20, // &53 Cyan vif
    0,  // &54 Noir
    2,  // &55 Bleu vif
    9,  // &56 Vert
    11, // &57 Bleu ciel
    4,  // &58 Magenta
    22, // &59 Vert pastel
    21, // &5A Vert lime
    23, // &5B Cyan pastel
    3,  // &5C Rouge
    5,  // &5D Mauve
    12, // &5E Jaune
    14, // &5F Bleu pastel
];

/// Instantané des réglages du Gate Array qui déterminent l'image.
///
/// Le mode vidéo et la palette peuvent être reprogrammés en cours de trame :
/// c'est la technique dite de "rupture", couramment employée pour afficher un
/// bandeau de score dans un mode ou des couleurs différents du reste de l'écran.
/// Le rendu doit donc s'appuyer sur l'état ligne par ligne, et non sur l'état
/// courant du Gate Array, qui n'est que le dernier de la trame.
#[derive(Clone, Copy)]
pub struct GateArrayState {
    pub video_mode: u8,
    pub palette: [u8; 17],
}

impl GateArrayState {
    /// Couleur RGB d'une encre (0 à 15 pour les stylos, 16 pour la bordure).
    pub fn rgb(&self, index: usize) -> (u8, u8, u8) {
        match self.palette.get(index) {
            Some(&hw) => CPC_COLORS_RGB[HARDWARE_TO_PHYSICAL[(hw & 0x1F) as usize]],
            None => (0, 0, 0),
        }
    }
}

/// Émulation du Gate Array de l'Amstrad CPC.
pub struct GateArray {
    pub selected_pen: u8,          // Stylo sélectionné (0-15, ou 16 pour la bordure)
    pub palette: [u8; 17],         // Palette des 16 encres + la bordure (valeurs matérielles 0-31)
    pub video_mode: u8,            // Mode vidéo actuel (0, 1, 2)
    pub hsync_counter: u32,        // Compteur 6 bits de lignes HSYNC générant les interruptions
    pub vsync_delay: u8,           // HSYNC restants avant le contrôle post-VSYNC (0 = inactif)
    pub interrupt_requested: bool, // Indique si une interruption est en attente
}

impl GateArray {
    /// Crée un Gate Array initialisé aux valeurs par défaut.
    pub fn new() -> Self {
        let mut ga = Self {
            selected_pen: 0,
            palette: [0; 17],
            video_mode: 1, // Mode 1 par défaut (utilisé par la ROM de diagnostic)
            hsync_counter: 0,
            vsync_delay: 0,
            interrupt_requested: false,
        };
        for i in 0..17 {
            ga.palette[i] = 13; // Blanc cassé / Gris
        }
        ga
    }

    /// Instantané des réglages qui déterminent l'image, à mémoriser à chaque
    /// scanline pour que le rendu suive les changements en cours de trame.
    pub fn state(&self) -> GateArrayState {
        GateArrayState {
            video_mode: self.video_mode,
            palette: self.palette,
        }
    }

    /// Reçoit une écriture d'I/O (lorsque port & 0xC000 == 0x4000).
    /// Le Gate Array décode la valeur écrite via ses deux bits de poids fort (7 et 6).
    pub fn write_register(
        &mut self,
        val: u8,
        rom_low_enabled: &mut bool,
        rom_high_enabled: &mut bool,
    ) {
        match val >> 6 {
            0 => {
                // Bit 7=0, Bit 6=0 : Sélection du stylo (Pen Selection)
                if (val & 0x10) != 0 {
                    self.selected_pen = 16; // Bordure
                } else {
                    self.selected_pen = val & 0x0F;
                }
            }
            1 => {
                // Bit 7=0, Bit 6=1 : Sélection de la couleur
                if (self.selected_pen as usize) < self.palette.len() {
                    self.palette[self.selected_pen as usize] = val & 0x1F;
                }
            }
            2 => {
                // Bit 7=1, Bit 6=0 : Mode vidéo + Configuration ROM + délai d'interruption
                // (les 3 réglages partagent le MÊME octet sur le vrai Gate Array)
                // - Bits 1-0 : Mode vidéo (0, 1, 2)
                // - Bit 2     : ROM basse (0 = activée, 1 = désactivée)
                // - Bit 3     : ROM haute (0 = activée, 1 = désactivée)
                // - Bit 4     : Délai d'interruption (1 = reset du compteur HSYNC)
                self.video_mode = val & 0x03;
                *rom_low_enabled = (val & 0x04) == 0;
                *rom_high_enabled = (val & 0x08) == 0;

                if (val & 0x10) != 0 {
                    self.hsync_counter = 0;
                    self.vsync_delay = 0;
                    self.interrupt_requested = false;
                }
            }
            // Bit 7=1, Bit 6=1 (valeur 3, seul cas restant puisque `val >> 6`
            // tient sur deux bits) : ce n'est PAS une fonction du Gate Array.
            // Sur les modèles 128 Ko, cette plage est interceptée par le MMU
            // séparé qui gère le banking RAM étendu (voir bus.rs).
            // Le Gate Array lui-même n'a rien à faire ici.
            _ => {}
        }
    }

    /// Avance le compteur d'interruptions du Gate Array d'une ligne (appelé à
    /// chaque HSYNC). Renvoie true si une interruption doit être levée.
    ///
    /// Le compteur 6 bits n'est pas un simple compteur libre : il est recalé sur
    /// le VSYNC du CRTC, ce qui garantit qu'une interruption tombe toujours juste
    /// après le début du retour de trame. Le firmware s'en sert pour détecter le
    /// "frame flyback" (en lisant le bit 0 du port B du PPI depuis le handler) et
    /// n'y committe la table d'encres vers le matériel qu'à ce moment-là : sans ce
    /// recalage, INK et BORDER restent définitivement sans effet.
    ///
    /// - à 52 HSYNC : interruption, compteur remis à zéro ;
    /// - 2 HSYNC après le début du VSYNC : interruption si le bit 5 du compteur
    ///   est armé, puis compteur remis à zéro dans les deux cas.
    ///
    /// `vsync_start` doit être vrai uniquement sur la ligne où le VSYNC démarre
    /// (front montant), pas pendant toute sa durée.
    pub fn step_hsync(&mut self, vsync_start: bool) -> bool {
        let mut interrupt = false;

        self.hsync_counter += 1;
        if self.hsync_counter >= 52 {
            self.hsync_counter = 0;
            interrupt = true;
        }

        if vsync_start {
            self.vsync_delay = 2;
        } else if self.vsync_delay > 0 {
            self.vsync_delay -= 1;
            if self.vsync_delay == 0 {
                // Une interruption n'est levée à ce recalage QUE si le bit 5
                // du compteur est à ZÉRO (compteur < 32) — pas l'inverse.
                // C'est ce que décrivent les documents de référence
                // (cpctech.cpcwiki.de/docs/ints.html, repris par CPCWiki) :
                //
                //   "If the top bit of the 6-bit counter is set to '1' (i.e.
                //    the counter >=32), then there is no interrupt request,
                //    and the 6-bit counter is reset to '0'. If the top bit
                //    of the 6-bit counter is set to '0' (i.e. the counter
                //    <32), then a interrupt request is issued, and the
                //    6-bit counter is reset to '0'."
                //
                // C'est ce recalage qui rattrape la dérive de phase
                // introduite par `acknowledge_interrupt` (voir sa doc) :
                // inversé, il ne compensait rien et le nombre
                // d'interruptions par trame se mettait à varier (voir
                // doc/sprite-flicker.md).
                if (self.hsync_counter & 0x20) == 0 {
                    interrupt = true;
                }
                self.hsync_counter = 0;
            }
        }

        if interrupt {
            self.interrupt_requested = true;
        }
        interrupt
    }

    /// Acquittement de l'interruption par le Z80.
    ///
    /// Le Gate Array force alors le bit 5 du compteur à zéro, ce qui garantit
    /// qu'au moins 32 lignes s'écouleront avant l'interruption suivante : sans
    /// cela, une interruption levée par le recalage VSYNC pourrait être suivie
    /// presque immédiatement par celle du compteur libre.
    pub fn acknowledge_interrupt(&mut self) {
        self.hsync_counter &= 0x1F;
        self.interrupt_requested = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les 27 couleurs canoniques, reprises des constantes de Colors.asm de la
    /// ROM Amstrad Diagnostics (elles-mêmes issues de la table CPCWiki).
    /// Format : (valeur écrite sur le Gate Array, index de couleur physique).
    const REFERENCE_PALETTE: [(u8, usize); 27] = [
        (0x54, 0),  // Noir
        (0x44, 1),  // Bleu
        (0x55, 2),  // Bleu vif
        (0x5C, 3),  // Rouge
        (0x58, 4),  // Magenta
        (0x5D, 5),  // Mauve
        (0x4C, 6),  // Rouge vif
        (0x45, 7),  // Pourpre
        (0x4D, 8),  // Magenta vif
        (0x56, 9),  // Vert
        (0x46, 10), // Cyan
        (0x57, 11), // Bleu ciel
        (0x5E, 12), // Jaune
        (0x40, 13), // Blanc / Gris
        (0x5F, 14), // Bleu pastel
        (0x4E, 15), // Orange
        (0x47, 16), // Rose
        (0x4F, 17), // Magenta pastel
        (0x52, 18), // Vert vif
        (0x42, 19), // Vert d'eau
        (0x53, 20), // Cyan vif
        (0x5A, 21), // Vert lime
        (0x59, 22), // Vert pastel
        (0x5B, 23), // Cyan pastel
        (0x4A, 24), // Jaune vif
        (0x43, 25), // Jaune pastel
        (0x4B, 26), // Blanc brillant
    ];

    #[test]
    fn hardware_palette_matches_the_reference_table() {
        for (hw_value, expected) in REFERENCE_PALETTE {
            let hw = (hw_value & 0x1F) as usize;
            assert_eq!(
                HARDWARE_TO_PHYSICAL[hw], expected,
                "valeur materielle &{hw_value:02X}"
            );
        }
    }

    /// Les 27 couleurs du CPC sont toutes les combinaisons de R, V, B pris dans
    /// {0, 128, 255}, et chacune n'apparaît qu'une fois.
    #[test]
    fn physical_colors_are_the_27_distinct_rgb_combinations() {
        let mut seen = CPC_COLORS_RGB.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 27, "couleurs physiques dupliquees");

        for (r, g, b) in CPC_COLORS_RGB {
            for component in [r, g, b] {
                assert!(
                    matches!(component, 0 | 128 | 255),
                    "composante hors des trois niveaux du CPC dans {:?}",
                    (r, g, b)
                );
            }
        }
    }

    /// Le noir doit être noir : c'est la valeur qu'un jeu écrit pour éteindre la
    /// bordure, et la faire virer au bleu vif se voit immédiatement.
    #[test]
    fn border_set_to_black_renders_black() {
        let mut ga = GateArray::new();
        let mut rom_low = true;
        let mut rom_high = true;

        ga.write_register(0x10, &mut rom_low, &mut rom_high); // stylo 16 = bordure
        ga.write_register(0x54, &mut rom_low, &mut rom_high); // couleur noire
        assert_eq!(ga.state().rgb(16), (0, 0, 0));
    }

    /// L'instantané doit être indépendant du Gate Array : c'est toute son
    /// utilité, mémoriser l'état d'une scanline avant qu'une rupture ne le
    /// modifie pour les suivantes.
    #[test]
    fn state_snapshot_is_independent_from_later_changes() {
        let mut ga = GateArray::new();
        let mut rom_low = true;
        let mut rom_high = true;

        ga.write_register(0x00, &mut rom_low, &mut rom_high); // stylo 0
        ga.write_register(0x54, &mut rom_low, &mut rom_high); // noir
        ga.write_register(0x8D, &mut rom_low, &mut rom_high); // mode 1
        let captured = ga.state();

        // Rupture : le programme change mode et encre en cours de trame.
        ga.write_register(0x4B, &mut rom_low, &mut rom_high); // stylo 0 -> blanc
        ga.write_register(0x8C, &mut rom_low, &mut rom_high); // mode 0

        assert_eq!(captured.video_mode, 1);
        assert_eq!(captured.rgb(0), (0, 0, 0));
        assert_eq!(ga.video_mode, 0);
        assert_eq!(ga.state().rgb(0), (255, 255, 255));
    }

    /// Numéros des lignes (relatives au début de trame) sur lesquelles une
    /// interruption est levée, pour une trame de 312 lignes dont le VSYNC
    /// démarre à `vsync_line`.
    fn interrupt_lines(ga: &mut GateArray, vsync_line: u32, frame_lines: u32) -> Vec<u32> {
        let mut lines = Vec::new();
        for line in 0..frame_lines {
            if ga.step_hsync(line == vsync_line) {
                lines.push(line);
            }
        }
        lines
    }

    #[test]
    fn interrupt_every_52_lines_without_vsync() {
        let mut ga = GateArray::new();
        assert_eq!(
            interrupt_lines(&mut ga, u32::MAX, 312),
            vec![51, 103, 155, 207, 259, 311]
        );
    }

    /// Le point qui cassait BORDER/INK : sans recalage, 312 = 6 x 52 fige la
    /// phase et aucune interruption ne tombe jamais dans la fenêtre VSYNC.
    #[test]
    fn interrupt_fires_two_lines_after_vsync_start() {
        let mut ga = GateArray::new();
        let vsync_line = 240;
        // Deuxième trame : le compteur a déjà été recalé par la première.
        interrupt_lines(&mut ga, vsync_line, 312);
        let lines = interrupt_lines(&mut ga, vsync_line, 312);
        assert!(
            lines.contains(&(vsync_line + 2)),
            "aucune interruption 2 lignes après le VSYNC : {lines:?}"
        );
    }

    /// Compteur < 32 au moment du contrôle : interruption levée, puis remise
    /// à zéro. C'est ce cas qui rattrape la dérive de phase laissée par un
    /// acquittement tardif (voir `acknowledge_interrupt`).
    #[test]
    fn vsync_forces_interrupt_when_bit5_clear() {
        let mut ga = GateArray::new();
        ga.hsync_counter = 10;
        assert!(!ga.step_hsync(true));
        assert!(!ga.step_hsync(false));
        assert!(ga.step_hsync(false));
        assert_eq!(ga.hsync_counter, 0);
    }

    /// Compteur >= 32 au moment du contrôle : le VSYNC le remet à zéro sans
    /// lever d'interruption supplémentaire — la prochaine arrivera d'elle-même
    /// assez tôt.
    #[test]
    fn vsync_resets_counter_without_interrupt_when_bit5_set() {
        let mut ga = GateArray::new();
        ga.hsync_counter = 40;
        assert!(!ga.step_hsync(true));
        assert!(!ga.step_hsync(false));
        assert!(!ga.step_hsync(false));
        assert_eq!(ga.hsync_counter, 0);
    }
}
