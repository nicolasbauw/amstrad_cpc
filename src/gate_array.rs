/// Émulation du Gate Array de l'Amstrad CPC.
///
/// Le Gate Array gère :
/// 1. La sélection des couleurs et de la palette (17 couleurs éditables parmi 27 physiques).
/// 2. La configuration de la mémoire (banking).
/// 3. Les interruptions matérielles (générées toutes les 52 lignes de balayage).
pub struct GateArray {
    pub selected_pen: u8, // Stylo sélectionné (0-15 pour les encres, 0x10 pour la bordure)
    pub palette: [u8; 17], // Palette des 16 encres + la bordure (valeurs matérielles 0-26)
    pub hsync_counter: u32, // Compteur de lignes HSYNC pour générer les interruptions
    pub interrupt_requested: bool, // Indique si une interruption est en attente
}

impl GateArray {
    /// Crée un Gate Array initialisé aux valeurs par défaut.
    pub fn new() -> Self {
        Self {
            selected_pen: 0,
            palette: [0; 17],
            hsync_counter: 0,
            interrupt_requested: false,
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
                // Le bit 4 détermine s'il s'agit de la bordure (1) ou d'une encre standard (0).
                if (val & 0x10) != 0 {
                    self.selected_pen = 0x10; // Bordure
                } else {
                    self.selected_pen = val & 0x0F; // Stylos 0 à 15
                }
            }
            1 => {
                // Bit 7=0, Bit 6=1 : Sélection de la couleur (Color Selection)
                // Attribue la couleur physique (0-26) au stylo actuellement sélectionné.
                if (self.selected_pen as usize) < self.palette.len() {
                    self.palette[self.selected_pen as usize] = val & 0x1F;
                }
            }
            2 => {
                // Bit 7=1, Bit 6=0 : Configuration mémoire (Banking)
                // Géré conjointement avec Memory. Le bus distribuera cette configuration.
                *rom_low_enabled = (val & 0x01) == 0;
                *rom_high_enabled = (val & 0x02) == 0;
            }
            3 => {
                // Bit 7=1, Bit 6=1 : Configuration du mode vidéo et des ROMs additionnelles
                // Bit 0 & 1 : Choix du Mode d'affichage vidéo (Mode 0, 1, 2)
                // Bit 2 : Reset du compteur de lignes HSYNC (remise à zéro de la demande d'interruption)
                let video_mode = val & 0x03;
                let interrupt_reset = (val & 0x08) != 0;

                if interrupt_reset {
                    self.hsync_counter = 0;
                    self.interrupt_requested = false;
                }

                // (Le mode vidéo sera utilisé par notre moteur de rendu graphique plus tard)
                _ = video_mode;
            }
            _ => unreachable!(),
        }
    }

    /// Avance le compteur de cycles (Ticks) du Gate Array.
    /// Sur Amstrad CPC, un signal HSYNC (balayage d'une ligne) se produit toutes les 64 microsecondes,
    /// soit tous les 64 cycles CPU (ticks) à 4 MHz (ou 64 cycles NOP d'une longueur de 4 cycles machine, soit 4 T-states par cycle machine).
    /// En réalité, le CRTC envoie un signal HSYNC qui incrémente le compteur du Gate Array.
    /// Toutes les 52 lignes HSYNC, le Gate Array lève une interruption CPU.
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
