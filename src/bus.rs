use crate::crtc::Crtc;
use crate::gate_array::GateArray;
use crate::memory::Memory;
use crate::ppi::Ppi;
use crate::psg::Psg;
use zilog_z80::bus::Bus;

/// Le Bus système du CPC qui interconnecte tous les composants matériels.
pub struct CpcBus {
    pub memory: Memory,
    pub gate_array: GateArray,
    pub crtc: Crtc,
    pub psg: Psg,
    pub ppi: Ppi,
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
    fn read_io(&self, port: u16) -> u8 {
        if (port & 0x0800) == 0 {
            return self.ppi.read_register(port, &self.psg);
        }
        if (port & 0x4000) == 0 {
            if (port & 0x0100) == 0 {
                return self.crtc.read_data();
            }
        }
        0xFF
    }

    /// Écriture sur un port I/O.
    fn write_io(&mut self, port: u16, value: u8) {
        // Détectons les écritures au Gate Array pour le debug
        if (port & 0xC000) == 0x4000 {
            println!(
                " >>> GATE ARRAY WRITE: port=0x{:04X}, value=0x{:02X}",
                port, value
            );
            self.gate_array.write_register(
                value,
                &mut self.memory.rom_low_enabled,
                &mut self.memory.rom_high_enabled,
                &mut self.memory.ram_config,
            );
        }

        if (port & 0x4000) == 0 {
            if (port & 0x0100) != 0 {
                self.crtc.select_register(value);
            } else {
                self.crtc.write_data(value);
            }
        }

        if (port & 0x0800) == 0 {
            self.ppi.write_register(port, value, &mut self.psg);
        }

        if (port & 0x2000) == 0 {
            self.memory.select_high_rom(value);
        }
    }
}
