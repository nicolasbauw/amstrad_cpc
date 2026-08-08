use crate::crtc::Crtc;
use crate::fdc::Fdc;
use crate::gate_array::GateArray;
use crate::memory::Memory;
use crate::ppi::Ppi;
use crate::psg::Psg;
use crate::tape::Tape;
use zilog_z80::bus::Bus;

use std::cell::RefCell;
use std::collections::HashSet;

/// L'interface disque du CPC répond quand les bits 10 et 7 du port sont à 0.
/// Les deux conditions comptent : ne tester que le bit 10 fait aussi répondre le
/// FDC à des ports destinés à d'autres composants, dont &79FF utilisé pour le
/// banking RAM.
fn fdc_selected(port: u16) -> bool {
    (port & 0x0480) == 0
}

/// Le Bus système du CPC qui interconnecte tous les composants matériels.
pub struct CpcBus {
    pub memory: Memory,
    pub gate_array: GateArray,
    pub crtc: Crtc,
    pub psg: Psg,
    pub ppi: Ppi,
    pub fdc: RefCell<Fdc>,
    pub tape: RefCell<Tape>,
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
            tape: RefCell::new(Tape::new()),
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
            return self
                .ppi
                .read_register(port, &self.psg, self.tape.borrow().read_bit());
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

        // 3. Décodage du FDC : l'interface disque répond quand les bits 10 ET 7
        // sont à 0. Les deux sont nécessaires, et le bit 7 vaut bien 0 dans les
        // trois adresses utilisées :
        //   &FA7E = 1111 1010 0111 1110  -> bit10=0, bit7=0, bit8=0 (moteur)
        //   &FB7E = 1111 1011 0111 1110  -> bit10=0, bit7=0, bit8=1, bit0=0 (MSR)
        //   &FB7F = 1111 1011 0111 1111  -> bit10=0, bit7=0, bit8=1, bit0=1 (DATA)
        // Omettre le bit 7 fait répondre le FDC à des ports qui ne le concernent
        // pas : &79FF, utilisé pour le banking RAM, a bit10=0 et tombait donc
        // dans son registre de données, ce qui détraquait sa machine à états.
        if fdc_selected(port) {
            // C'est le BIT 8 qui sépare le contrôle moteur du chip FDC.
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

            // Le MMU (bits 7-6 = 11) réagit à la valeur écrite ; le port
            // complet ne compte que pour une éventuelle extension mémoire
            // tierce (voir Memory::write_mmu_register).
            if (value & 0xC0) == 0xC0 {
                self.memory.write_mmu_register(port, value);
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
            // Bit 4 du port C : contrôle du moteur cassette. Confirmé par
            // désassemblage de la routine cassette du firmware (ROM basse,
            // ~0x29D2-0x2A0A) : elle établit ce bit tôt et le maintient à
            // travers une reconfiguration complète du PPI, jusqu'à l'état
            // observé à l'écran "Press PLAY then any key" (port C = 0x58,
            // soit bit 6 = mode PSG, bit 4 = 1, bits 3-0 = ligne clavier —
            // bit 4 est le seul bit qui n'a pas d'autre explication).
            // Répercuté après coup plutôt que testé sur `value`, pour rester
            // correct que le port C ait été écrit directement ou via le
            // mode "Bit Set/Reset" (voir `Ppi::write_register`, qui gère les
            // deux) — même principe que la synchronisation du moteur FDC
            // ci-dessous.
            self.tape.borrow_mut().motor_on = (self.ppi.port_c & 0x10) != 0;
        }

        // 4. Sélection de la ROM haute (Bit 13 = 0, soit port & 0x2000 == 0)
        if (port & 0x2000) == 0 {
            self.memory.select_high_rom(value);
        }

        // 5. Décodage du FDC et contrôle du moteur (bits 10 et 7 à 0).
        if fdc_selected(port) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Le bit du port C qui pilote le moteur cassette a d'abord été mal
    /// identifié (bit 5 au lieu du bit 4) — confirmé faux par une capture
    /// d'écran réelle où le firmware maintenait le port C à 0x58 (bit 4
    /// posé, bit 5 non posé) à l'écran "Press PLAY then any key", alors que
    /// le code affichait pourtant "Motor On: false". Ce test verrouille le
    /// bon bit pour ne pas régresser.
    #[test]
    fn writing_port_c_bit_4_turns_the_tape_motor_on() {
        let mut bus = CpcBus::new(Memory::new(0));
        assert!(!bus.tape.borrow().motor_on);

        // &F658, exactement la valeur observée en usage réel.
        bus.write_io(0xF600, 0x58);
        assert!(
            bus.tape.borrow().motor_on,
            "port C = 0x58 (bit 4 pose) doit activer le moteur cassette"
        );

        bus.write_io(0xF600, 0x48); // bit 4 retire (garde bit 6, ligne clavier 8)
        assert!(!bus.tape.borrow().motor_on);
    }

    /// Le trait `Bus` fournit des `read_io`/`write_io` par défaut qui ne font
    /// rien : si l'une de nos implémentations sort par mégarde du bloc
    /// `impl Bus for CpcBus`, le code compile toujours mais TOUTES les
    /// entrées-sorties disparaissent en silence — le clavier semble alors
    /// bloqué toutes touches enfoncées, et rien ne le signale. Ce test passe
    /// donc délibérément par le trait, pas par les méthodes inhérentes.
    #[test]
    fn io_actually_goes_through_the_bus_trait() {
        fn write_through_trait(bus: &mut impl Bus, port: u16, value: u8) {
            bus.write_io(port, value);
        }
        fn read_through_trait(bus: &impl Bus, port: u16) -> u8 {
            bus.read_io(port)
        }

        let mut bus = CpcBus::new(Memory::new(0));

        // &F6xx : port C du PPI (sélection de ligne clavier + dialogue PSG).
        write_through_trait(&mut bus, 0xF640, 0x40);
        assert_eq!(
            bus.ppi.port_c, 0x40,
            "un OUT vers &F6xx doit atteindre le port C du PPI"
        );

        // Et la lecture correspondante rend bien la ligne clavier demandée,
        // au repos (aucune touche enfoncée) plutôt qu'un octet nul.
        bus.psg.selected_register = 14;
        assert_eq!(
            read_through_trait(&bus, 0xF400),
            0xFF,
            "un IN sur &F4xx doit rendre la ligne clavier au repos"
        );
    }

    /// Les trois adresses de l'interface disque, et un échantillon de ports
    /// destinés aux autres composants qui ne doivent surtout pas l'atteindre.
    #[test]
    fn only_the_disc_interface_ports_reach_the_fdc() {
        for port in [0xFA7E, 0xFB7E, 0xFB7F] {
            assert!(fdc_selected(port), "{port:#06X} devrait atteindre le FDC");
        }

        for port in [
            0x79FF, // Gate Array / banking RAM : bit 10 à 0 mais bit 7 à 1
            0x7BFF, // idem
            0x7F00, // Gate Array
            0xBC00, // CRTC
            0xBD00, 0xBE00, 0xBF00, 0xDF00, // sélection de ROM haute
            0xF400, 0xF500, 0xF600, 0xF700, // PPI
        ] {
            assert!(
                !fdc_selected(port),
                "{port:#06X} ne devrait pas atteindre le FDC"
            );
        }
    }
}
