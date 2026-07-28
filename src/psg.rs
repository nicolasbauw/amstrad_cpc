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

    pub fn set_key_state(&mut self, keycode: Keycode, pressed: bool) {
        // Mappage AZERTY adapté pour les claviers Mac (Swap A/Q et Z/W car SDL Mac envoie les codes QWERTY physiques)
        let cpc_key = match keycode {
            // Ligne 0
            Keycode::F7 => Some((0, 0)),
            Keycode::F8 => Some((0, 1)),
            Keycode::F9 => Some((0, 2)),
            Keycode::KpDivide => Some((0, 3)),
            Keycode::KpMultiply => Some((0, 4)),
            Keycode::KpMinus => Some((0, 5)),
            Keycode::KpPlus => Some((0, 6)),
            Keycode::KpPeriod => Some((0, 7)),

            // Ligne 1
            Keycode::Kp0 => Some((1, 0)),
            Keycode::Kp1 => Some((1, 1)),
            Keycode::Kp2 => Some((1, 2)),
            Keycode::Kp3 => Some((1, 3)),
            Keycode::Kp4 => Some((1, 4)),
            Keycode::Kp5 => Some((1, 5)),
            Keycode::Kp6 => Some((1, 6)),
            Keycode::F10 => Some((1, 7)),

            // Ligne 2
            Keycode::Kp7 => Some((2, 0)),
            Keycode::Kp8 => Some((2, 1)),
            Keycode::Kp9 => Some((2, 2)),
            Keycode::KpEnter => Some((2, 3)),
            Keycode::F4 => Some((2, 4)),
            Keycode::F5 => Some((2, 5)),
            Keycode::F6 => Some((2, 6)),
            Keycode::F3 => Some((2, 7)),

            // Ligne 3 : Flèches et Enter
            Keycode::Backspace => Some((3, 0)),
            Keycode::LeftBracket => Some((3, 1)),
            Keycode::Return => Some((3, 2)),
            Keycode::RightBracket => Some((3, 3)),
            Keycode::Down => Some((3, 4)),
            Keycode::Left => Some((3, 5)),
            Keycode::Right => Some((3, 6)),
            Keycode::Up => Some((3, 7)),

            // Ligne 4 : Modificateurs
            Keycode::Space => Some((4, 0)),
            Keycode::RShift => Some((4, 1)),
            Keycode::RCtrl => Some((4, 2)),
            Keycode::LCtrl => Some((4, 3)),
            Keycode::LShift => Some((4, 4)),
            Keycode::CapsLock => Some((4, 5)),
            Keycode::Tab => Some((4, 6)),
            Keycode::Insert => Some((4, 7)),

            // Ligne 5
            Keycode::V => Some((5, 0)),
            Keycode::B => Some((5, 1)),
            Keycode::N => Some((5, 2)),
            Keycode::M => Some((5, 3)),
            Keycode::Comma => Some((5, 4)),
            Keycode::Period => Some((5, 5)),
            Keycode::Slash => Some((5, 6)),
            Keycode::Backslash => Some((5, 7)),

            // Ligne 6
            Keycode::D => Some((6, 0)),
            Keycode::F => Some((6, 1)),
            Keycode::G => Some((6, 2)),
            Keycode::H => Some((6, 3)),
            Keycode::J => Some((6, 4)),
            Keycode::K => Some((6, 5)),
            Keycode::L => Some((6, 6)),

            // Ligne 7
            Keycode::R => Some((7, 0)),
            Keycode::T => Some((7, 1)),
            Keycode::Y => Some((7, 2)),
            Keycode::U => Some((7, 3)),
            Keycode::I => Some((7, 4)),
            Keycode::O => Some((7, 5)),
            Keycode::P => Some((7, 6)),

            // Ligne 8 : Les Swaps AZERTY pour Mac
            Keycode::E => Some((8, 0)),
            Keycode::W => Some((8, 1)), // Presser 'W' (Mac) -> CPC 'Z'
            Keycode::Q => Some((8, 2)), // Presser 'A' (Mac) -> CPC 'A' (Position AZERTY)
            Keycode::A => Some((8, 3)), // Presser 'Q' (Mac) -> CPC 'Q'
            Keycode::S => Some((8, 4)),
            Keycode::Z => Some((8, 5)), // Presser 'Z' (Mac) -> CPC 'W'
            Keycode::X => Some((8, 6)),
            Keycode::C => Some((8, 7)),

            // Ligne 9
            Keycode::Escape => Some((9, 0)),
            Keycode::Num1 => Some((9, 1)),
            Keycode::Num2 => Some((9, 2)),
            Keycode::Num3 => Some((9, 3)),
            Keycode::Num4 => Some((9, 4)),
            Keycode::Num5 => Some((9, 5)),
            Keycode::Num6 => Some((9, 6)),
            Keycode::Num7 => Some((9, 7)),

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
