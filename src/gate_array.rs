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
    (0, 128, 255),   // 11: Cyan vif
    (128, 128, 0),   // 12: Jaune
    (128, 128, 128), // 13: Blanc cassé / Gris
    (128, 128, 255), // 14: Pastel Bleu
    (255, 0, 0),     // 15: Orange (utilisé comme index 15 physique sur CPC)
    (255, 128, 128), // 16: Pastel Rouge
    (255, 128, 255), // 17: Pastel Rose
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

/// Table de conversion officielle complète entre les index de couleurs matériels du Gate Array (0 à 31)
/// et l'index de la couleur physique réelle (0 à 26).
pub const HARDWARE_TO_PHYSICAL: [usize; 32] = [
    13, 17, 19, 26, 1, 10, 21, 22, // 0-7
    15, 4, 24, 25, 6, 22, 3, 5, // 8-15
    14, 16, 18, 20, 2, 9, 0, 7, // 16-23
    12, 21, 10, 13, 13, 13, 13, 13, // 24-31
];

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

    /// Récupère la couleur RGB (r, g, b) d'une encre de la palette (0 à 15 pour stylos, 16 pour la bordure).
    pub fn get_rgb_color(&self, index: usize) -> (u8, u8, u8) {
        if index < 17 {
            let hw_color = self.palette[index] as usize & 0x1F;
            let physical_color = HARDWARE_TO_PHYSICAL[hw_color];
            CPC_COLORS_RGB[physical_color]
        } else {
            (0, 0, 0)
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
            3 => {
                // Bit 7=1, Bit 6=1 : Ce n'est PAS une fonction du Gate Array.
                // Sur les modèles 128 Ko, cette plage est interceptée par le MMU
                // séparé qui gère le banking RAM étendu (voir bus.rs).
                // Le Gate Array lui-même n'a rien à faire ici.
            }
            _ => unreachable!(),
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
                if (self.hsync_counter & 0x20) != 0 {
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

    /// Compteur < 32 au moment du contrôle : le VSYNC le remet à zéro sans
    /// lever d'interruption supplémentaire.
    #[test]
    fn vsync_resets_counter_without_interrupt_when_bit5_clear() {
        let mut ga = GateArray::new();
        ga.hsync_counter = 10;
        assert!(!ga.step_hsync(true));
        assert!(!ga.step_hsync(false));
        assert!(!ga.step_hsync(false));
        assert_eq!(ga.hsync_counter, 0);
    }

    /// Compteur >= 32 au moment du contrôle : interruption forcée puis remise à zéro.
    #[test]
    fn vsync_forces_interrupt_when_bit5_set() {
        let mut ga = GateArray::new();
        ga.hsync_counter = 40;
        assert!(!ga.step_hsync(true));
        assert!(!ga.step_hsync(false));
        assert!(ga.step_hsync(false));
        assert_eq!(ga.hsync_counter, 0);
    }
}
