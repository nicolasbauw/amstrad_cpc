use sdl2::keyboard::Keycode;

/// Émulation de la puce sonore AY-3-8910 (PSG) et de la matrice du clavier de l'Amstrad CPC.
pub struct Psg {
    pub selected_register: u8, // Registre PSG actuellement sélectionné (0 à 15)
    pub registers: [u8; 16],   // Les 16 registres internes du PSG
    pub keyboard_matrix: [u8; 10], // Matrice du clavier : 10 lignes de 8 colonnes (0 = touche pressée, logique négative)
    pub selected_keyboard_line: u8, // Ligne de clavier actuellement sélectionnée pour la lecture (0 à 9)
}

impl Psg {
    /// Crée un PSG initialisé avec aucune touche pressée (valeurs à 0xFF en logique négative).
    pub fn new() -> Self {
        Self {
            selected_register: 0,
            registers: [0; 16],
            keyboard_matrix: [0xFF; 10], // 0xFF signifie qu'aucune touche n'est enfoncée
            selected_keyboard_line: 0,
        }
    }

    /// Met à jour la matrice du clavier lorsqu'une touche est enfoncée ou relâchée.
    /// On mappe les touches physiques d'un clavier moderne PC (SDL2) vers la matrice CPC d'origine.
    pub fn set_key_state(&mut self, keycode: Keycode, pressed: bool) {
        // Recherche de la ligne et du bit (colonne) de la touche CPC
        // Matrice clavier CPC standard (ligne, bit de 0 à 7) :
        let cpc_key = match keycode {
            // Ligne 0
            Keycode::KpPeriod => Some((0, 7)),
            Keycode::KpEnter => Some((0, 6)),
            Keycode::F3 => Some((0, 5)),
            Keycode::F6 => Some((0, 4)),
            Keycode::F9 => Some((0, 3)),
            Keycode::F5 => Some((0, 2)),
            Keycode::F8 => Some((0, 1)),
            Keycode::F7 => Some((0, 0)),

            // Ligne 1
            Keycode::Kp0 => Some((1, 6)), // Mappage de f0 vers le 0 du pavé numérique !
            Keycode::F2 => Some((1, 5)),
            Keycode::F1 => Some((1, 4)),
            Keycode::F4 => Some((1, 2)),
            Keycode::LShift | Keycode::RShift => Some((1, 1)),
            Keycode::LCtrl | Keycode::RCtrl => Some((1, 0)),

            // Ligne 2
            Keycode::Left => Some((2, 5)),
            Keycode::Return => Some((2, 3)), // Touche Enter principale
            Keycode::Down => Some((2, 2)),
            Keycode::Right => Some((2, 1)),
            Keycode::Up => Some((2, 0)),

            // Ligne 3
            Keycode::Num3 => Some((3, 7)),
            Keycode::Num2 => Some((3, 6)),
            Keycode::Num1 => Some((3, 5)),
            Keycode::Num0 => Some((3, 3)),
            Keycode::Num9 => Some((3, 2)),
            Keycode::Num8 => Some((3, 1)),
            Keycode::Num7 => Some((3, 0)),

            // Ligne 4
            Keycode::P => Some((4, 7)),
            Keycode::O => Some((4, 3)),
            Keycode::I => Some((4, 2)),
            Keycode::U => Some((4, 1)),
            Keycode::Y => Some((4, 0)),

            // Ligne 5
            Keycode::L => Some((5, 7)),
            Keycode::K => Some((5, 6)),
            Keycode::J => Some((5, 5)),
            Keycode::H => Some((5, 4)),
            Keycode::M => Some((5, 3)),
            Keycode::N => Some((5, 2)),
            Keycode::B => Some((5, 1)),
            Keycode::V => Some((5, 0)),

            // Ligne 6
            Keycode::T => Some((6, 7)),
            Keycode::R => Some((6, 6)),
            Keycode::E => Some((6, 5)),
            Keycode::W => Some((6, 4)),
            Keycode::Q => Some((6, 3)),
            Keycode::A => Some((6, 2)),
            Keycode::S => Some((6, 1)),
            Keycode::D => Some((6, 0)),

            // Ligne 7
            Keycode::G => Some((7, 7)),
            Keycode::F => Some((7, 6)),
            Keycode::Z => Some((7, 5)),
            Keycode::X => Some((7, 4)),
            Keycode::C => Some((7, 3)),
            Keycode::Space => Some((7, 2)),
            Keycode::F11 => Some((7, 1)), // CAPS LOCK ou Tab
            Keycode::Tab => Some((7, 0)),

            // Ligne 8
            Keycode::Num4 => Some((8, 7)),
            Keycode::Num5 => Some((8, 6)),
            Keycode::Num6 => Some((8, 5)),
            Keycode::Escape => Some((8, 2)),

            _ => None,
        };

        if let Some((line, bit)) = cpc_key {
            if pressed {
                // En logique négative, un bit à 0 signifie que la touche est enfoncée !
                self.keyboard_matrix[line] &= !(1 << bit);
            } else {
                // Un bit à 1 signifie que la touche est relâchée
                self.keyboard_matrix[line] |= 1 << bit;
            }
        }
    }

    /// Écrit une valeur dans le registre PSG actuellement sélectionné.
    pub fn write_current_register(&mut self, val: u8) {
        let reg = self.selected_register as usize;
        if reg < 16 {
            self.registers[reg] = val;
        }
    }

    /// Lit la valeur du registre PSG actuellement sélectionné.
    /// Le registre 14 (Port A) est mappé sur la lecture de la matrice du clavier !
    pub fn read_current_register(&self) -> u8 {
        let reg = self.selected_register as usize;
        if reg == 14 {
            // Lecture du clavier : on renvoie l'état de la ligne sélectionnée
            let line = self.selected_keyboard_line as usize;
            if line < 10 {
                self.keyboard_matrix[line]
            } else {
                0xFF
            }
        } else if reg < 16 {
            self.registers[reg]
        } else {
            0xFF
        }
    }
}
