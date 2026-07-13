use crate::memory::Memory;
use zilog_z80::bus::Bus;

/// Le Bus système du CPC qui interconnecte tous les composants.
pub struct CpcBus {
    pub memory: Memory,
}

impl CpcBus {
    /// Crée un nouveau Bus système CPC.
    pub fn new(memory: Memory) -> Self {
        Self { memory }
    }
}

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
        // Pour l'instant, retourne la valeur du bus flottant par défaut (0xFF)
        0xFF
    }

    /// Écriture sur un port I/O.
    /// C'est ici que l'on décode les adresses d'I/O pour configurer la mémoire (banking).
    fn write_io(&mut self, port: u16, value: u8) {
        // 1. Décodage du Gate Array (Adresse I/O : bit 15 = 0, bit 14 = 1, soit port & 0xC000 == 0x4000)
        if (port & 0xC000) == 0x4000 {
            // Un octet envoyé au Gate Array peut configurer plusieurs registres :
            // Si les bits 7 et 6 de la valeur écrite valent '10' (0x80), c'est une configuration mémoire.
            if (value & 0xC0) == 0x80 {
                self.memory.configure_banking(value);
            }
        }

        // 2. Sélection de la ROM haute (Adresse I/O : bit 13 = 0, soit port & 0x2000 == 0)
        // Généralement écrit à $DF00
        if (port & 0x2000) == 0 {
            self.memory.select_high_rom(value);
        }
    }
}
