/// Émulation du contrôleur vidéo CRTC 6845 de l'Amstrad CPC.
///
/// Le CRTC est responsable de la génération des adresses de la VRAM,
/// de la synchronisation d'affichage (HSYNC, VSYNC), et du dimensionnement de l'écran.
pub struct Crtc {
    pub selected_register: u8, // Registre actuellement sélectionné (0 à 17)
    pub registers: [u8; 18],   // Les 18 registres de configuration du CRTC

    // --- Compteurs de balayage vertical ---
    pub char_row: u8,     // Ligne de caractères courante (comparée à R4)
    pub raster: u8,       // Scanline dans la ligne de caractères (comparée à R9)
    pub scanline: u32,    // Scanline absolue depuis le début de la trame
    pub vsync: bool,      // VSYNC actif (lu par le firmware via le bit 0 du port B du PPI)
    vsync_remaining: u8,  // Scanlines de VSYNC restantes
    adjust_remaining: u8, // Scanlines d'ajustement vertical (R5) restantes
}

/// Valeurs programmées par la ROM du 6128 au démarrage. Les registres d'un vrai
/// CRTC sont indéterminés à la mise sous tension, mais le firmware les programme
/// dans les toutes premières instructions : partir de ces valeurs évite une
/// trame dégénérée (R4 = R9 = 0, donc une trame d'une seule ligne avec un VSYNC
/// permanent) pendant cet intervalle, et donne un affichage cohérent au débogueur.
const DEFAULT_REGISTERS: [u8; 18] = [
    63,   // R0  : total horizontal
    40,   // R1  : caractères affichés
    46,   // R2  : position HSYNC
    0x8E, // R3  : largeurs VSYNC/HSYNC
    38,   // R4  : total vertical (lignes de caractères)
    0,    // R5  : ajustement vertical (scanlines)
    25,   // R6  : lignes de caractères affichées
    30,   // R7  : position VSYNC
    0,    // R8  : interlace
    7,    // R9  : hauteur d'une ligne de caractères - 1
    0, 0, // R10-R11 : curseur (inutilisé sur CPC)
    0x30, 0x00, // R12-R13 : adresse de départ de l'écran (0xC000, mode standard)
    0, 0, 0, 0, // R14-R17 : curseur / light pen
];

impl Crtc {
    /// Crée un CRTC initialisé.
    pub fn new() -> Self {
        Self {
            selected_register: 0,
            registers: DEFAULT_REGISTERS,
            char_row: 0,
            raster: 0,
            scanline: 0,
            vsync: false,
            vsync_remaining: 0,
            adjust_remaining: 0,
        }
    }

    /// Écriture d'adresse (sélection du registre actif)
    /// Se produit lorsque le port d'I/O a le bit 14 à 0 et le bit 9 à 1.
    pub fn select_register(&mut self, reg: u8) {
        // Le CRTC possède 18 registres valides (0 à 17)
        if reg < 18 {
            self.selected_register = reg;
        }
    }

    /// Écriture de données dans le registre actuellement sélectionné.
    /// Se produit lorsque le port d'I/O a le bit 14 à 0 et le bit 9 à 0.
    pub fn write_data(&mut self, val: u8) {
        let reg = self.selected_register as usize;
        if reg < 18 {
            self.registers[reg] = val;
        }
    }

    /// Lecture de données du registre actuellement sélectionné.
    pub fn read_data(&self) -> u8 {
        let reg = self.selected_register as usize;
        if reg < 18 { self.registers[reg] } else { 0 }
    }

    /// Nombre de scanlines d'une trame complète, tel que programmé dans les
    /// registres : (R4 + 1) lignes de caractères de (R9 + 1) scanlines, plus
    /// l'ajustement vertical R5.
    pub fn frame_scanlines(&self) -> u32 {
        (self.registers[4] as u32 + 1) * ((self.registers[9] & 0x1F) as u32 + 1)
            + (self.registers[5] & 0x1F) as u32
    }

    /// Scanline (relative au début de trame) sur laquelle démarre le VSYNC.
    /// C'est la référence sur laquelle le moniteur cale l'image verticalement.
    pub fn vsync_scanline(&self) -> u32 {
        self.registers[7] as u32 * ((self.registers[9] & 0x1F) as u32 + 1)
    }

    /// Nombre total de caractères d'une ligne de balayage horizontal (R0 + 1).
    pub fn line_chars(&self) -> u32 {
        self.registers[0] as u32 + 1
    }

    /// Avance le balayage vertical d'une scanline (appelé à chaque HSYNC).
    /// Renvoie true si le VSYNC démarre sur cette scanline (front montant), ce
    /// dont le Gate Array a besoin pour recaler son compteur d'interruptions.
    pub fn step_scanline(&mut self) -> bool {
        let r4 = self.registers[4];
        let r5 = self.registers[5] & 0x1F;
        let r7 = self.registers[7];
        let r9 = self.registers[9] & 0x1F;
        let vsync_width = self.registers[3] >> 4;

        self.scanline = self.scanline.wrapping_add(1);

        if self.adjust_remaining > 0 {
            // Ajustement vertical (R5) : scanlines supplémentaires en fin de trame,
            // pendant lesquelles les compteurs de lignes de caractères sont figés.
            self.adjust_remaining -= 1;
            if self.adjust_remaining == 0 {
                self.char_row = 0;
                self.raster = 0;
                self.scanline = 0;
            }
        } else if self.raster >= r9 {
            self.raster = 0;
            if self.char_row >= r4 {
                if r5 > 0 {
                    self.adjust_remaining = r5;
                    self.char_row = self.char_row.wrapping_add(1);
                } else {
                    self.char_row = 0;
                    self.scanline = 0;
                }
            } else {
                self.char_row += 1;
            }
        } else {
            self.raster += 1;
        }

        if self.vsync_remaining > 0 {
            self.vsync_remaining -= 1;
        }

        // Le VSYNC démarre sur la première scanline de la ligne de caractères R7.
        let vsync_start = self.adjust_remaining == 0 && self.raster == 0 && self.char_row == r7;
        if vsync_start {
            // Un R3 dont le quartet haut vaut 0 signifie 16 scanlines de VSYNC.
            self.vsync_remaining = if vsync_width == 0 { 16 } else { vsync_width };
        }
        self.vsync = self.vsync_remaining > 0;

        vsync_start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trame standard du CPC : 39 lignes de caractères de 8 scanlines = 312.
    #[test]
    fn default_frame_is_312_scanlines() {
        let mut crtc = Crtc::new();
        assert_eq!(crtc.frame_scanlines(), 312);

        // La scanline absolue doit reboucler exactement sur la période annoncée.
        for _ in 0..312 {
            crtc.step_scanline();
        }
        assert_eq!(crtc.scanline, 0);
        assert_eq!(crtc.char_row, 0);
        assert_eq!(crtc.raster, 0);
    }

    /// Le VSYNC démarre à la ligne de caractères R7 (30 x 8 = scanline 240),
    /// et une seule fois par trame.
    #[test]
    fn vsync_starts_once_per_frame_at_r7() {
        let mut crtc = Crtc::new();
        let mut starts = Vec::new();
        for _ in 0..312 {
            if crtc.step_scanline() {
                starts.push(crtc.scanline);
            }
        }
        assert_eq!(starts, vec![240]);
    }

    /// Test de non-régression du bug d'origine : une interruption doit tomber
    /// pendant le VSYNC à chaque trame, sinon le firmware ne détecte jamais le
    /// retour de trame et ne committe jamais INK/BORDER vers le Gate Array.
    #[test]
    fn an_interrupt_lands_during_vsync_on_every_frame() {
        let mut crtc = Crtc::new();
        let mut ga = crate::gate_array::GateArray::new();

        for frame in 0..4 {
            let mut interrupts_during_vsync = 0;
            for _ in 0..crtc.frame_scanlines() {
                let vsync_start = crtc.step_scanline();
                if ga.step_hsync(vsync_start) {
                    ga.acknowledge_interrupt();
                    if crtc.vsync {
                        interrupts_during_vsync += 1;
                    }
                }
            }
            assert!(
                interrupts_during_vsync > 0,
                "trame {frame} : aucune interruption pendant le VSYNC"
            );
        }
    }

    /// L'ajustement vertical R5 allonge la trame du nombre de scanlines demandé.
    #[test]
    fn vertical_adjust_extends_the_frame() {
        let mut crtc = Crtc::new();
        crtc.registers[5] = 6;
        assert_eq!(crtc.frame_scanlines(), 318);

        for _ in 0..318 {
            crtc.step_scanline();
        }
        assert_eq!(crtc.scanline, 0);
        assert_eq!(crtc.char_row, 0);
    }
}
