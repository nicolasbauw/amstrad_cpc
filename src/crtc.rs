/// Émulation du contrôleur vidéo CRTC 6845 de l'Amstrad CPC.
///
/// Le CRTC est responsable de la génération des adresses de la VRAM,
/// de la synchronisation d'affichage (HSYNC, VSYNC), et du dimensionnement de l'écran.
pub struct Crtc {
    pub selected_register: u8, // Registre actuellement sélectionné (0 à 17)
    pub registers: [u8; 18],   // Les 18 registres de configuration du CRTC
}

impl Crtc {
    /// Crée un CRTC initialisé.
    pub fn new() -> Self {
        Self {
            selected_register: 0,
            registers: [0; 18],
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
}
