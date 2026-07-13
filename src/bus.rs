use crate::memory::Memory;
use zilog_z80::bus::Bus;

/// Le Bus système du CPC qui interconnecte tous les composants.
pub struct CpcBus {
    pub memory: Memory,
    // Plus tard, nous ajouterons ici les autres composants matériels :
    // - gate_array (Gate Array)
    // - crtc (CRTC 6845)
    // - psg (AY-3-8910)
}

impl CpcBus {
    /// Crée un nouveau Bus système CPC.
    pub fn new(memory: Memory) -> Self {
        Self { memory }
    }
}

// Implémentation du trait Bus de la crate zilog_z80
impl Bus for CpcBus {
    /// Lecture mémoire routée vers la structure Memory
    fn read_byte(&self, address: u16) -> u8 {
        self.memory.read_byte(address)
    }

    /// Écriture mémoire routée vers la structure Memory
    fn write_byte(&mut self, address: u16, value: u8) {
        self.memory.write_byte(address, value)
    }

    /// Lecture d'un port I/O
    fn read_io(&self, _port: u16) -> u8 {
        // Pour l'instant, retourne la valeur du bus flottant par défaut
        0xFF
    }

    /// Écriture sur un port I/O
    fn write_io(&mut self, _port: u16, _value: u8) {
        // À implémenter lors de l'intégration du Gate Array, du CRTC, et du PSG
    }
}
