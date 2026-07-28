use crate::psg::Psg;

/// Émulation du composant PPI 8255 (Peripheral Programmable Interface) du CPC.
///
/// Le PPI interconnecte le CPU avec le PSG, le clavier, et les signaux système (VSYNC, cassette).
pub struct Ppi {
    pub port_a: u8,           // Données d'I/O avec le PSG
    pub port_b_input: u8,     // Entrée d'état système (VSYNC, etc.)
    pub port_c: u8,           // Contrôle du PSG et sélection de ligne clavier
    pub control_register: u8, // Configuration de la direction des ports
}

impl Ppi {
    /// Crée un PPI initialisé.
    pub fn new() -> Self {
        Self {
            port_a: 0,
            port_b_input: 0x1E, // Valeur par défaut (VSYNC à 0, LK configuration d'usine)
            port_c: 0,
            control_register: 0x92, // Mode par défaut du CPC
        }
    }

    /// Lecture du PPI (port & 0x0800 == 0)
    /// L'adresse du PPI est déterminée par les bits 9 et 8 du port d'I/O.
    pub fn read_register(&self, port: u16, psg: &Psg) -> u8 {
        match (port >> 8) & 0x03 {
            0 => {
                // Port A ($F4xx) : Lire les données du PSG si configuré en entrée
                // Si le port C applique "Lire registre" (BDIR=0, BC1=1, soit 0x40)
                if (self.port_c & 0xC0) == 0x40 {
                    psg.read_current_register()
                } else {
                    self.port_a
                }
            }
            1 => {
                // Port B ($F5xx) : Lecture seule de l'état du système
                self.port_b_input
            }
            2 => {
                // Port C ($F6xx) : Lecture des sorties de contrôle
                self.port_c
            }
            _ => 0xFF,
        }
    }

    /// Écriture dans le PPI (port & 0x0800 == 0)
    pub fn write_register(&mut self, port: u16, value: u8, psg: &mut Psg) {
        match (port >> 8) & 0x03 {
            0 => {
                // Port A ($F4xx) : Données à envoyer au PSG
                self.port_a = value;
                self.sync_psg(psg);
            }
            1 => {
                // Port B ($F5xx) : Lecture seule sur le CPC (les écritures n'ont pas d'effet direct)
            }
            2 => {
                // Port C ($F6xx) : Contrôle du PSG et choix de la ligne clavier
                self.port_c = value;

                // Les bits 3-0 sélectionnent la ligne de clavier à interroger
                psg.selected_keyboard_line = value & 0x0F;

                self.sync_psg(psg);
            }
            3 => {
                // Registre de contrôle ($F7xx)
                if (value & 0x80) == 0 {
                    // Mode "Bit Set/Reset" pour le Port C
                    let bit_to_modify = (value >> 1) & 0x07;
                    let bit_value = value & 0x01;
                    if bit_value != 0 {
                        self.port_c |= 1 << bit_to_modify;
                    } else {
                        self.port_c &= !(1 << bit_to_modify);
                    }
                    // Mettre à jour la ligne de clavier et synchroniser le PSG
                    psg.selected_keyboard_line = self.port_c & 0x0F;
                    self.sync_psg(psg);
                } else {
                    // Mode de configuration standard de direction des ports
                    self.control_register = value;
                }
            }
            _ => {}
        }
    }

    /// Synchronise et applique les signaux de contrôle du Port C vers le PSG.
    /// Bits 7-6 du Port C :
    /// - 00 : Inactif / Prêt (PSG inactif)
    /// - 01 : Écriture de données (Le PSG applique la valeur du Port A dans le registre sélectionné)
    /// - 10 : Lecture de données (Non géré ici, géré lors de la lecture du Port A)
    /// - 11 : Sélection de registre (Le PSG utilise la valeur du Port A comme index de registre actif)
    fn sync_psg(&mut self, psg: &mut Psg) {
        let psg_control = self.port_c & 0xC0;
        match psg_control {
            0xC0 => {
                // 11 (0xC0) : Sélectionner le registre actif du PSG
                psg.selected_register = self.port_a;
            }
            0x80 => {
                // 10 (0x80) : Écrire dans le registre sélectionné du PSG
                psg.write_current_register(self.port_a);
            }
            _ => {}
        }
    }

    /// Met à jour l'état du signal VSYNC sur le Port B (Bit 0).
    pub fn set_vsync(&mut self, vsync_active: bool) {
        if vsync_active {
            self.port_b_input |= 0x01; // Bit 0 à 1
        } else {
            self.port_b_input &= !0x01; // Bit 0 à 0
        }
    }
}
