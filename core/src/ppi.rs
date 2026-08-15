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

impl Default for Ppi {
    fn default() -> Self {
        Self::new()
    }
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
    ///
    /// `tape_bit` est le niveau courant du signal cassette (bit 7 du port
    /// B — confirmé par lecture directe du firmware, ROM basse, boucle de
    /// scrutation en 0x2B55 : `LD B,$F5 / IN A,(C) / XOR L / AND $80`,
    /// donc bit 7, pas bit 6 comme le laissait supposer un commentaire
    /// antérieur qui n'était lui-même pas sûr de son fait) : contrairement
    /// à VSYNC/joystick (mis à jour une fois par scanline, voir
    /// `set_system_port_b`), il doit refléter l'état au moment exact de la
    /// lecture — un pulse de cassette dure souvent moins d'une scanline.
    pub fn read_register(&self, port: u16, psg: &Psg, tape_bit: bool) -> u8 {
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
                // Port B ($F5xx) : Lecture seule de l'état du système, avec
                // le bit 7 (cassette) recalé en temps réel.
                if tape_bit {
                    self.port_b_input | 0x80
                } else {
                    self.port_b_input & !0x80
                }
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
                    // Mode de configuration standard de direction des ports.
                    //
                    // Le 8255 remet à zéro ses registres de sortie à cette
                    // occasion, et du code s'appuie dessus : Barbarian écrit le
                    // mot de contrôle, relit le port C, puis y ajoute par un OU
                    // le numéro de ligne clavier à interroger. Sans cette remise
                    // à zéro, les numéros de ligne s'accumulent jusqu'à 15, le
                    // PSG répond 0xFF pour cette ligne inexistante, et le jeu
                    // conclut que rien n'est jamais pressé.
                    self.control_register = value;
                    self.port_a = 0;
                    self.port_c = 0;
                    psg.selected_keyboard_line = 0;
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

    /// Met à jour l'état du signal VSYNC, Joystick et configuration système sur le Port B (Bit 0).
    /// Sur le CPC 6128, le Port B contient les états système suivants (bits lus par le CPU) :
    /// - Bit 0 : VSYNC
    /// - Bit 1 : Sélection manette (0=Joystick B, 1=Joystick A)
    /// - Bit 2-4 : Configuration des périphériques
    /// - Bit 5 : Port parallèle (prêt)
    /// - Bit 6 : ?
    /// - Bit 7 : Cassette (données) — confirmé par lecture directe du
    ///   firmware (ROM basse, 0x2B55), pas géré ici mais dans
    ///   `read_register` (recalé au moment exact de la lecture).
    pub fn set_system_port_b(&mut self, vsync: bool, joystick_sel: bool) {
        // Bit 0: VSYNC (1 si actif, 0 sinon)
        if vsync {
            self.port_b_input |= 0x01;
        } else {
            self.port_b_input &= !0x01;
        }

        // Bit 1: Sélection Joystick (0 = Joy B, 1 = Joy A).
        // En forçant à 1, on indique au système de lire le Joystick A.
        if joystick_sel {
            self.port_b_input |= 0x02;
        } else {
            self.port_b_input &= !0x02;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT_A: u16 = 0xF400;
    const PORT_C: u16 = 0xF600;

    /// Reproduit la séquence qu'utilise le firmware pour écrire un registre du
    /// PSG : valeur du registre sur le port A, code de fonction sur le port C,
    /// puis retour à l'état inactif.
    fn psg_write(ppi: &mut Ppi, psg: &mut Psg, reg: u8, value: u8) {
        ppi.write_register(PORT_A, reg, psg);
        ppi.write_register(PORT_C, 0xC0, psg); // sélection de registre
        ppi.write_register(PORT_C, 0x00, psg); // inactif
        ppi.write_register(PORT_A, value, psg);
        ppi.write_register(PORT_C, 0x80, psg); // écriture
        ppi.write_register(PORT_C, 0x00, psg);
    }

    #[test]
    fn the_firmware_write_sequence_reaches_the_sound_registers() {
        let mut ppi = Ppi::new();
        let mut psg = Psg::new();

        psg_write(&mut ppi, &mut psg, 0, 0x34); // période A, poids faible
        psg_write(&mut ppi, &mut psg, 1, 0x02); // période A, poids fort
        psg_write(&mut ppi, &mut psg, 7, 0x3E); // ton A seul
        psg_write(&mut ppi, &mut psg, 8, 0x0F); // volume maximum

        assert_eq!(psg.registers[0], 0x34);
        assert_eq!(psg.registers[1], 0x02);
        assert_eq!(psg.registers[7], 0x3E);
        assert_eq!(psg.registers[8], 0x0F);

        // Et le son sort effectivement : une note doit moduler la sortie.
        psg.tick(4 * 100_000);
        let samples = psg.sound.take_samples();
        assert!(
            samples.iter().any(|&s| s > 0.3) && samples.iter().any(|&s| s == 0.0),
            "un ton audible etait attendu"
        );
    }

    /// La sélection de ligne clavier passe par les mêmes bits du port C que le
    /// dialogue avec le PSG : les deux doivent cohabiter.
    #[test]
    fn selecting_a_keyboard_line_does_not_disturb_the_psg_registers() {
        let mut ppi = Ppi::new();
        let mut psg = Psg::new();

        psg_write(&mut ppi, &mut psg, 8, 0x0F);
        ppi.write_register(PORT_C, 0x45, &mut psg); // lecture + ligne 5

        assert_eq!(psg.selected_keyboard_line, 5);
        assert_eq!(psg.registers[8], 0x0F, "le registre ne doit pas bouger");
    }

    /// Écrire le mot de contrôle remet les sorties du 8255 à zéro. Barbarian
    /// interroge son clavier en relisant le port C puis en y ajoutant le numéro
    /// de ligne par un OU : sans cette remise à zéro, les numéros s'accumulent
    /// et le jeu ne voit plus jamais aucune touche.
    #[test]
    fn setting_the_mode_clears_the_output_registers() {
        let mut ppi = Ppi::new();
        let mut psg = Psg::new();

        ppi.write_register(PORT_A, 0xFF, &mut psg);
        ppi.write_register(PORT_C, 0x0F, &mut psg);
        assert_eq!(psg.selected_keyboard_line, 0x0F);

        ppi.write_register(0xF700, 0x82, &mut psg);

        assert_eq!(ppi.port_c, 0, "le port C doit repartir de zero");
        assert_eq!(ppi.port_a, 0, "le port A aussi");
        assert_eq!(psg.selected_keyboard_line, 0);
        assert_eq!(ppi.control_register, 0x82, "le mot de controle est retenu");
    }

    /// La séquence exacte de Barbarian : mot de contrôle, relecture du port C,
    /// ajout du numéro de ligne par un OU. Chaque ligne demandée doit être
    /// celle voulue, et non le cumul des précédentes.
    #[test]
    fn a_scan_that_ors_the_line_number_still_selects_the_right_line() {
        let mut ppi = Ppi::new();
        let mut psg = Psg::new();
        psg.keyboard_matrix[2] = 0b1111_1011; // une touche pressee sur la ligne 2

        for line in 0..10u8 {
            ppi.write_register(0xF700, 0x82, &mut psg); // port A en sortie
            ppi.write_register(PORT_A, 14, &mut psg); // registre clavier
            let port_c = ppi.read_register(PORT_C, &psg, false);
            ppi.write_register(PORT_C, port_c | 0xC0 | line, &mut psg); // selection
            ppi.write_register(PORT_C, (port_c | 0xC0 | line) & 0x3F, &mut psg);
            ppi.write_register(0xF700, 0x92, &mut psg); // port A en entree
            ppi.write_register(PORT_C, line | 0x40, &mut psg); // lecture
            let value = ppi.read_register(PORT_A, &psg, false);

            assert_eq!(
                psg.selected_keyboard_line, line,
                "ligne {line} demandee, ligne {} selectionnee",
                psg.selected_keyboard_line
            );
            let expected = if line == 2 { 0b1111_1011 } else { 0xFF };
            assert_eq!(value, expected, "valeur lue pour la ligne {line}");
        }
    }

    #[test]
    fn the_bit_set_reset_mode_also_updates_the_keyboard_line() {
        let mut ppi = Ppi::new();
        let mut psg = Psg::new();

        // Mode "Bit Set/Reset" : bit 0 du port C à 1, puis bit 1 à 1.
        ppi.write_register(0xF700, 0x01, &mut psg);
        ppi.write_register(0xF700, 0x03, &mut psg);

        assert_eq!(psg.selected_keyboard_line, 3);
    }
}
