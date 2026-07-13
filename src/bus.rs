use crate::crtc::Crtc;
use crate::gate_array::GateArray;
use crate::memory::Memory;
use zilog_z80::bus::Bus;

/// Le Bus système du CPC qui interconnecte tous les composants matériels.
pub struct CpcBus {
    pub memory: Memory,
    pub gate_array: GateArray,
    pub crtc: Crtc,
}

impl CpcBus {
    /// Crée un nouveau Bus système CPC complet.
    pub fn new(memory: Memory) -> Self {
        Self {
            memory,
            gate_array: GateArray::new(),
            crtc: Crtc::new(),
        }
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

    /// Lecture d'un port I/O.
    /// On décode le port I/O pour savoir quel composant interroger.
    fn read_io(&self, port: u16) -> u8 {
        // 1. Décodage du CRTC (Bit 14 = 0, soit port & 0x4000 == 0)
        if (port & 0x4000) == 0 {
            // Lecture de données du CRTC si le bit 9 est à 0 (port & 0x0100 == 0)
            if (port & 0x0100) == 0 {
                return self.crtc.read_data();
            }
        }

        // Valeur par défaut si aucun composant ne répond (bus flottant)
        0xFF
    }

    /// Écriture sur un port I/O.
    /// Distribue l'information au Gate Array, au CRTC, ou au banking de mémoire.
    fn write_io(&mut self, port: u16, value: u8) {
        // 1. Décodage du Gate Array (Bit 15 = 0, Bit 14 = 1, soit port & 0xC000 == 0x4000)
        if (port & 0xC000) == 0x4000 {
            // On passe au Gate Array des références mutables vers l'état de banking de la mémoire
            self.gate_array.write_register(
                value,
                &mut self.memory.rom_low_enabled,
                &mut self.memory.rom_high_enabled,
            );
        }

        // 2. Décodage du CRTC (Bit 14 = 0, soit port & 0x4000 == 0)
        if (port & 0x4000) == 0 {
            if (port & 0x0100) != 0 {
                // Bit 9 à 1 : Sélection du registre actif
                self.crtc.select_register(value);
            } else {
                // Bit 9 à 0 : Écriture de données dans le registre sélectionné
                self.crtc.write_data(value);
            }
        }

        // 3. Sélection de la ROM haute (Bit 13 = 0, soit port & 0x2000 == 0)
        if (port & 0x2000) == 0 {
            self.memory.select_high_rom(value);
        }
    }
}
