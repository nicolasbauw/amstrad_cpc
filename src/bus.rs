use crate::crtc::Crtc;
use crate::fdc::Fdc;
use crate::gate_array::GateArray;
use crate::memory::Memory;
use crate::ppi::Ppi;
use crate::psg::Psg;
use zilog_z80::bus::Bus;

use std::cell::RefCell;
use std::collections::HashSet;

/// Le Bus système du CPC qui interconnecte tous les composants matériels.
pub struct CpcBus {
    pub memory: Memory,
    pub gate_array: GateArray,
    pub crtc: Crtc,
    pub psg: Psg,
    pub ppi: Ppi,
    pub fdc: RefCell<Fdc>,
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
            fdc: RefCell::new(Fdc::new()),
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
            // Les bits 9-8 sélectionnent l'une des QUATRE fonctions du CRTC :
            //   &BCxx = 00 : sélection de registre (écriture seule)
            //   &BDxx = 01 : écriture de données
            //   &BExx = 10 : lecture du registre d'état
            //   &BFxx = 11 : lecture de données
            // Les confondre fait échouer la détection du type de CRTC, qui
            // compare précisément ce que renvoient &BExx et &BFxx.
            match (port >> 8) & 0x03 {
                2 => return self.crtc.read_status(),
                3 => return self.crtc.read_data(),
                _ => {}
            }
        }

        // 3. Décodage du FDC (Bit 10 = 0, soit port & 0x0400 == 0)
        if (port & 0x0400) == 0 {
            // Le vrai matériel distingue le contrôle moteur (&FA7E) du chip FDC
            // (&FB7E/&FB7F) via le BIT 8 de l'adresse, PAS le bit 7 :
            //   &FA7E = 1111 1010 0111 1110  -> bit8 = 0 (moteur)
            //   &FB7E = 1111 1011 0111 1110  -> bit8 = 1, bit0 = 0 (MSR)
            //   &FB7F = 1111 1011 0111 1111  -> bit8 = 1, bit0 = 1 (DATA)
            // Le bit 7 vaut 0 dans les TROIS cas : le tester ne permettait donc
            // jamais d'atteindre le chip FDC (bug corrigé ici).
            if (port & 0x0100) != 0 {
                if (port & 0x0001) != 0 {
                    return self.fdc.borrow_mut().read_data();
                } else {
                    return self.fdc.borrow().read_msr();
                }
            }
        }

        0xFF
    }

    /// Écriture sur un port I/O.
    fn write_io(&mut self, port: u16, value: u8) {
        // Décodage du Gate Array (Bit 15 = 0, Bit 14 = 1, soit port & 0xC000 == 0x4000)
        if (port & 0xC000) == 0x4000 {
            self.gate_array.write_register(
                value,
                &mut self.memory.rom_low_enabled,
                &mut self.memory.rom_high_enabled,
            );

            // Le MMU 128 Ko réagit uniquement à la VALEUR écrite (bits 7-6 = 11),
            // indépendamment du port exact utilisé pour l'accès au Gate Array.
            if (value & 0xC0) == 0xC0 {
                self.memory.ram_config = value & 0x07;
            }
        }

        // 2. Décodage du CRTC (Bit 14 = 0, soit port & 0x4000 == 0)
        if (port & 0x4000) == 0 {
            // Mêmes bits 9-8 qu'en lecture : seules les fonctions 00 et 01 sont
            // accessibles en écriture, &BExx et &BFxx sont des ports de lecture.
            match (port >> 8) & 0x03 {
                0 => self.crtc.select_register(value),
                1 => self.crtc.write_data(value),
                _ => {}
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

        // 5. Décodage du FDC et contrôle du moteur de disquette (Bit 10 = 0, soit port & 0x0400 == 0)
        if (port & 0x0400) == 0 {
            // Voir le commentaire détaillé dans read_io : c'est le BIT 8 qui sépare
            // le contrôle moteur (&FA7E, bit8=0) du chip FDC (&FB7E/&FB7F, bit8=1).
            if (port & 0x0100) != 0 {
                // Écriture DATA FDC (&FB7F, bit0=1). Une écriture sur &FB7E (MSR,
                // bit0=0) est ignorée : ce registre est en lecture seule sur le vrai
                // matériel.
                if (port & 0x0001) != 0 {
                    self.fdc.borrow_mut().write_data(value);
                }
            } else {
                // Écriture Contrôle Moteur (&FA7E)
                self.fdc.borrow_mut().motor_on = (value & 0x01) != 0;
            }
        }
    }
}
