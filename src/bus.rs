use crate::crtc::Crtc;
use crate::gate_array::GateArray;
use crate::memory::Memory;
use crate::ppi::Ppi;
use crate::psg::Psg;
use zilog_z80::bus::Bus;

use std::collections::HashSet;

/// Le Bus système du CPC qui interconnecte tous les composants matériels.
pub struct CpcBus {
    pub memory: Memory,
    pub gate_array: GateArray,
    pub crtc: Crtc,
    pub psg: Psg,
    pub ppi: Ppi,
    pub watchpoints: HashSet<u16>,
    pub watchpoint_hit: Option<u16>,
}

impl CpcBus {
    /// Crée un nouveau Bus système CPC complet.
    pub fn new(memory: Memory) -> Self {
        Self {
            memory,
            gate_array: GateArray::new(),
            crtc: Crtc::new(),
            psg: Psg::new(),
            ppi: Ppi::new(),
            watchpoints: HashSet::new(),
            watchpoint_hit: None,
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
        if self.watchpoints.contains(&address) {
            self.watchpoint_hit = Some(address);
        }
        self.memory.write_byte(address, value)
    }

    /// Lecture d'un port I/O.
    fn read_io(&self, port: u16) -> u8 {
        // 1. Décodage du PPI (Bit 11 = 0, soit port & 0x0800 == 0)
        if (port & 0x0800) == 0 {
            return self.ppi.read_register(port, &self.psg);
        }

        // 2. Décodage du CRTC (Bit 14 = 0, soit port & 0x4000 == 0)
        if (port & 0x4000) == 0 {
            if (port & 0x0100) == 0 {
                return self.crtc.read_data();
            }
        }
        0xFF
    }

    /// Écriture sur un port I/O.
    fn write_io(&mut self, port: u16, value: u8) {
        // Décodage du Gate Array (Bit 15 = 0, Bit 14 = 1, soit port & 0xC000 == 0x4000)
        if (port & 0xC000) == 0x4000 {
            // COMPORTEMENT ÉLECTRONIQUE PARALLÈLE :
            // 1. Le Gate Array standard traite TOUJOURS l'écriture pour configurer ses registres (Rom, palette, etc.)
            self.gate_array.write_register(
                value,
                &mut self.memory.rom_low_enabled,
                &mut self.memory.rom_high_enabled,
            );

            // 2. En parallèle, si le bit 5 d'adresse est à 1 (ligne A5 active) et qu'il s'agit d'une commande
            //    de configuration RAM étendu (bits 7-6 à 11, soit value & 0xC0 == 0xC0) :
            //    On applique également la configuration de banking RAM !
            if (port & 0x0020) != 0 && (value & 0xC0) == 0xC0 {
                self.memory.ram_config = value & 0x07;
            }
        }

        // 2. Décodage du CRTC (Bit 14 = 0, soit port & 0x4000 == 0)
        if (port & 0x4000) == 0 {
            if (port & 0x0100) != 0 {
                self.crtc.select_register(value);
            } else {
                self.crtc.write_data(value);
            }
        }

        // 3. Décodage du PPI (Bit 11 = 0, soit port & 0x0800 == 0)
        if (port & 0x0800) == 0 {
            self.ppi.write_register(port, value, &mut self.psg);
        }

        // 4. Sélection de la ROM haute (Bit 13 = 0, soit port & 0x2000 == 0)
        if (port & 0x2000) == 0 {
            self.memory.select_high_rom(value);
        }
    }
}
