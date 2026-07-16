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
    pub selected_pen: u8,   // Stylo sélectionné (0-15, ou 0x10 pour la bordure)
    pub palette: [u8; 17],  // Palette des 16 encres + la bordure (valeurs matérielles 0-31)
    pub video_mode: u8,     // Mode vidéo actuel (0, 1, 2)
    pub hsync_counter: u32, // Compteur de lignes HSYNC pour générer les interruptions
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
        ram_config: &mut u8,
    ) {
        match val >> 6 {
            0 => {
                // Bit 7=0, Bit 6=0 : Sélection du stylo (Pen Selection)
                if (val & 0x10) != 0 {
                    self.selected_pen = 16; // Bordure (mappée à l'index 16)
                } else {
                    self.selected_pen = val & 0x0F; // Stylos 0 à 15
                }
            }
            1 => {
                // Bit 7=0, Bit 6=1 : Sélection de la couleur (Color Selection)
                if (self.selected_pen as usize) < self.palette.len() {
                    self.palette[self.selected_pen as usize] = val & 0x1F;
                }
            }
            2 => {
                // Bit 7=1, Bit 6=0 : Configuration mémoire (Banking ROM)
                *rom_low_enabled = (val & 0x02) == 0;
                *rom_high_enabled = (val & 0x01) == 0;
            }
            3 => {
                // Bit 7=1, Bit 6=1 : Configuration RAM 128 Ko
                *ram_config = val & 0x07;

                self.video_mode = val & 0x03;
                let interrupt_reset = (val & 0x08) != 0;

                if interrupt_reset {
                    self.hsync_counter = 0;
                    self.interrupt_requested = false;
                }
            }
            _ => unreachable!(),
        }
    }

    /// Avance le compteur de cycles (Ticks) du Gate Array.
    pub fn step_hsync(&mut self) -> bool {
        self.hsync_counter += 1;
        if self.hsync_counter >= 52 {
            self.hsync_counter = 0;
            self.interrupt_requested = true;
            return true; // Demande d'interruption levée
        }
        false
    }
}
