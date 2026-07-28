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
        // Mappage basé sur la vraie matrice matérielle du CPC (formule officielle
        // "code = ligne*8 + bit"). La matrice physique est FIXE quel que soit le
        // clavier (AZERTY/QWERTY) : seules certaines touches ont été physiquement
        // échangées sur le clavier AZERTY (Q/A, W/Z, M/virgule/point-virgule).
        let cpc_key = match keycode {
            // Ligne 0
            Keycode::Up => Some((0, 0)),
            Keycode::Right => Some((0, 1)),
            Keycode::Down => Some((0, 2)),
            Keycode::F9 => Some((0, 3)),
            Keycode::F6 => Some((0, 4)),
            Keycode::F3 => Some((0, 5)),
            Keycode::KpEnter => Some((0, 6)),
            Keycode::KpPeriod => Some((0, 7)),

            // Ligne 1
            Keycode::Left => Some((1, 0)),
            Keycode::F7 => Some((1, 2)),
            Keycode::F8 => Some((1, 3)),
            Keycode::F5 => Some((1, 4)),
            Keycode::F1 => Some((1, 5)),
            Keycode::F2 => Some((1, 6)),
            Keycode::Kp0 => Some((1, 7)),

            // Ligne 2
            Keycode::Backspace => Some((2, 0)), // "Clr"
            Keycode::LeftBracket => Some((2, 1)),
            Keycode::Return => Some((2, 2)),
            Keycode::RightBracket => Some((2, 3)),
            Keycode::F4 => Some((2, 4)),
            Keycode::LShift | Keycode::RShift => Some((2, 5)),
            Keycode::Backslash => Some((2, 6)),
            Keycode::LCtrl | Keycode::RCtrl => Some((2, 7)),

            // Ligne 3
            Keycode::P => Some((3, 3)),
            Keycode::M => Some((3, 4)), // touche physique ";" -> caractère M en AZERTY
            Keycode::Period => Some((3, 7)),

            // Ligne 4
            Keycode::Num0 => Some((4, 0)),
            Keycode::Num9 => Some((4, 1)),
            Keycode::O => Some((4, 2)),
            Keycode::I => Some((4, 3)),
            Keycode::L => Some((4, 4)),
            Keycode::K => Some((4, 5)),
            Keycode::Comma => Some((4, 6)), // touche physique "M" -> caractère , en AZERTY
            Keycode::Semicolon => Some((4, 7)), // touche physique "," -> caractère ; en AZERTY

            // Ligne 5
            Keycode::Num8 => Some((5, 0)),
            Keycode::Num7 => Some((5, 1)),
            Keycode::U => Some((5, 2)),
            Keycode::Y => Some((5, 3)),
            Keycode::H => Some((5, 4)),
            Keycode::J => Some((5, 5)),
            Keycode::N => Some((5, 6)),
            Keycode::Space => Some((5, 7)),

            // Ligne 6
            Keycode::Num6 => Some((6, 0)),
            Keycode::Num5 => Some((6, 1)),
            Keycode::R => Some((6, 2)),
            Keycode::T => Some((6, 3)),
            Keycode::G => Some((6, 4)),
            Keycode::F => Some((6, 5)),
            Keycode::B => Some((6, 6)),
            Keycode::V => Some((6, 7)),

            // Ligne 7
            Keycode::Num4 => Some((7, 0)),
            Keycode::Num3 => Some((7, 1)),
            Keycode::E => Some((7, 2)),
            Keycode::Z => Some((7, 3)), // position physique "W" -> caractère Z en AZERTY
            Keycode::S => Some((7, 4)),
            Keycode::D => Some((7, 5)),
            Keycode::C => Some((7, 6)),
            Keycode::X => Some((7, 7)),

            // Ligne 8
            Keycode::Num1 => Some((8, 0)),
            Keycode::Num2 => Some((8, 1)),
            Keycode::Escape => Some((8, 2)),
            Keycode::A => Some((8, 3)), // position physique "Q" -> caractère A en AZERTY
            Keycode::Tab => Some((8, 4)),
            Keycode::Q => Some((8, 5)), // position physique "A" -> caractère Q en AZERTY
            Keycode::CapsLock => Some((8, 6)),
            Keycode::W => Some((8, 7)), // position physique "Z" -> caractère W en AZERTY

            _ => None,
        };

        if let Some((line, bit)) = cpc_key {
            if pressed {
                self.keyboard_matrix[line] &= !(1 << bit);
            } else {
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
