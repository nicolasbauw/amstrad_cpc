use sdl2::keyboard::{Keycode, Scancode};

// Émulation de la puce sonore AY-3-8910 (PSG) et de la matrice du clavier de l'Amstrad CPC.
pub struct Psg {
    pub selected_register: u8,
    pub registers: [u8; 16],
    pub keyboard_matrix: [u8; 10],
    pub selected_keyboard_line: u8,
    pub controller_state: [u8; 8], // 0-7: bits de la manette (Up, Down, Left, Right, Fire1, Fire2, Fire3, Fire4)
}

impl Psg {
    pub fn new() -> Self {
        Self {
            selected_register: 0,
            registers: [0; 16],
            keyboard_matrix: [0xFF; 10],
            selected_keyboard_line: 0,
            controller_state: [0; 8],
        }
    }

    /// Met à jour l'état d'un bouton de la manette.
    /// Mappe les boutons manette vers la matrice du clavier du CPC :
    /// - Ligne 9, Bits 0-4 sont utilisés pour les entrées manette (Joystick A) sur le CPC.
    ///   Note : le CPC attend 0 = pressé, 1 = relâché (logique inversée).
    pub fn set_controller_button(&mut self, button_index: usize, pressed: bool) {
        if button_index < 8 {
            self.controller_state[button_index] = if pressed { 1 } else { 0 };

            // Mappage manette vers matrice clavier CPC (Joystick A)
            // Ligne 9: Bit 0=Up, Bit 1=Down, Bit 2=Left, Bit 3=Right, Bit 4=Fire 1, Bit 5=Fire 2, Bit 6=Fire 3
            // Le bit est à 0 si le bouton est pressé, 1 sinon.
            let bit = match button_index {
                0 => Some(0), // Up
                1 => Some(1), // Down
                2 => Some(2), // Left
                3 => Some(3), // Right
                4 => Some(4), // Fire 1
                5 => Some(5), // Fire 2
                6 => Some(6), // Fire 3
                _ => None,
            };

            if let Some(b) = bit {
                if pressed {
                    self.keyboard_matrix[9] &= !(1 << b);
                } else {
                    self.keyboard_matrix[9] |= 1 << b;
                }
            }
        }
    }

    /// Applique un ensemble de bits de la matrice clavier CPC (permet de simuler un
    /// SHIFT virtuel pour les touches du pavé numérique, qui n'ont pas de modificateur
    /// physique mais doivent parfois atteindre un caractère "shifté" sur le CPC).
    fn apply_bits(&mut self, bits: &[(usize, u8)], pressed: bool) {
        for &(line, bit) in bits {
            if pressed {
                self.keyboard_matrix[line] &= !(1 << bit);
            } else {
                self.keyboard_matrix[line] |= 1 << bit;
            }
        }
    }

    /// Touches dont le Keycode macOS n'est pas exploitable de façon fiable : touche
    /// morte "^/¨" et caractères hors table SDLK ("ù", "#/@"). On se base ici sur le
    /// Scancode, qui reflète la position PHYSIQUE de la touche et ignore la disposition
    /// clavier active (donc insensible aux touches mortes). À appeler en PLUS (ou à la
    /// place, pour ces trois touches précises) du traitement par Keycode dans la boucle
    /// d'événements.
    ///
    /// Retourne `true` si la touche a été prise en charge ici.
    pub fn set_key_state_scancode(&mut self, scancode: Scancode, pressed: bool) -> bool {
        let cpc_key: Option<(usize, u8)> = match scancode {
            // "ù / %" -> position du "'" en disposition US
            Scancode::Apostrophe => Some((3, 4)),
            // touche morte "^ / ¨" -> position du "[" en disposition US
            Scancode::LeftBracket => Some((3, 2)),
            // touche ISO supplémentaire "# / @" (en haut à gauche) -> position du "`" en disposition US
            Scancode::Grave => Some((2, 3)),
            _ => None,
        };

        if let Some(bit) = cpc_key {
            self.apply_bits(&[bit], pressed);
            true
        } else {
            false
        }
    }

    pub fn set_key_state(&mut self, keycode: Keycode, pressed: bool) {
        // Mappage basé sur la vraie matrice matérielle du CPC (formule officielle
        // "code = ligne*8 + bit"), reconstituée à partir du patch clavier français
        // de la ROM de diagnostic (KeyboardLayout.asm / patchFrenchLabels).
        let cpc_key: Option<&[(usize, u8)]> = match keycode {
            // Ligne 0
            Keycode::Up => Some(&[(0, 0)]),
            Keycode::Right => Some(&[(0, 1)]),
            Keycode::Down => Some(&[(0, 2)]),
            Keycode::Kp9 => Some(&[(0, 3)]),
            Keycode::Kp6 => Some(&[(0, 4)]),
            Keycode::Kp3 => Some(&[(0, 5)]),
            Keycode::KpEnter => Some(&[(0, 6)]),
            Keycode::KpPeriod => Some(&[(0, 7)]),

            // Ligne 1
            Keycode::Left => Some(&[(1, 0)]),
            Keycode::LAlt | Keycode::RAlt => Some(&[(1, 1)]), // "Copy" via Option
            Keycode::Kp7 => Some(&[(1, 2)]),
            Keycode::Kp8 => Some(&[(1, 3)]),
            Keycode::Kp5 => Some(&[(1, 4)]),
            Keycode::Kp1 => Some(&[(1, 5)]),
            Keycode::Kp2 => Some(&[(1, 6)]),
            Keycode::Kp0 => Some(&[(1, 7)]),

            // Ligne 2
            Keycode::Backspace => Some(&[(9, 7)]), // "Del" CPC
            Keycode::Delete => Some(&[(2, 0)]),    // "Clr" CPC
            Keycode::Return => Some(&[(2, 2)]),
            Keycode::Kp4 => Some(&[(2, 4)]),
            Keycode::LShift | Keycode::RShift => Some(&[(2, 5)]),
            Keycode::LCtrl | Keycode::RCtrl => Some(&[(2, 7)]),

            // Symboles français directs
            Keycode::RightParen => Some(&[(3, 1)]), // ")"
            Keycode::Minus => Some(&[(3, 0)]),      // "-"
            Keycode::Equals | Keycode::Plus => Some(&[(3, 6)]), // "=" (Shift -> "+", via vrai SHIFT physique)
            Keycode::Colon | Keycode::Slash => Some(&[(3, 7)]), // ":" (Shift -> "/", via vrai SHIFT physique)
            Keycode::Percent => Some(&[(3, 4)]),                // "%" (variante shiftée de "ù")
            Keycode::Hash => Some(&[(2, 3)]), // "#" (filet de sécurité, cf. Scancode::Grave)
            Keycode::Dollar => Some(&[(2, 6)]), // "$"
            Keycode::Asterisk | Keycode::KpMultiply => Some(&[(2, 1)]), // "*"

            // Pavé numérique : ces touches n'ont pas de modificateur physique sur le Mac,
            // donc on simule nous-mêmes le SHIFT du CPC pour atteindre le caractère voulu.
            Keycode::KpDivide => Some(&[(2, 5), (3, 7)]), // "/" (shift de ":")
            Keycode::KpEquals => Some(&[(3, 6)]),
            Keycode::KpPlus => Some(&[(2, 5), (3, 6)]),
            Keycode::KpMinus => Some(&[(3, 0)]),

            // Ligne 3 (non-caractères directs)
            Keycode::P => Some(&[(3, 3)]),

            // Ligne 4
            Keycode::Num0 => Some(&[(4, 0)]),
            Keycode::Num9 => Some(&[(4, 1)]),
            Keycode::O => Some(&[(4, 2)]),
            Keycode::I => Some(&[(4, 3)]),
            Keycode::L => Some(&[(4, 4)]),
            Keycode::K => Some(&[(4, 5)]),
            Keycode::Comma => Some(&[(4, 6)]), // touche physique "M" -> "," en AZERTY
            Keycode::Semicolon | Keycode::Period => Some(&[(4, 7)]), // touche physique "," -> ";" en AZERTY
            Keycode::M => Some(&[(3, 5)]), // touche "M" du Mac -> "M" du CPC

            // NB : le CPC français n'a pas de touche "< >" dédiée comme sur le clavier
            // ISO du Mac ; on ne mappe donc plus Keycode::Less / Keycode::Greater
            // (c'est ce qui produisait à tort "," et "?").

            // Ligne 5
            Keycode::Num8 => Some(&[(5, 0)]),
            Keycode::Num7 => Some(&[(5, 1)]),
            Keycode::U => Some(&[(5, 2)]),
            Keycode::Y => Some(&[(5, 3)]),
            Keycode::H => Some(&[(5, 4)]),
            Keycode::J => Some(&[(5, 5)]),
            Keycode::N => Some(&[(5, 6)]),
            Keycode::Space => Some(&[(5, 7)]),

            // Ligne 6
            Keycode::Num6 => Some(&[(6, 0)]),
            Keycode::Num5 => Some(&[(6, 1)]),
            Keycode::R => Some(&[(6, 2)]),
            Keycode::T => Some(&[(6, 3)]),
            Keycode::G => Some(&[(6, 4)]),
            Keycode::F => Some(&[(6, 5)]),
            Keycode::B => Some(&[(6, 6)]),
            Keycode::V => Some(&[(6, 7)]),

            // Ligne 7
            Keycode::Num4 => Some(&[(7, 0)]),
            Keycode::Num3 => Some(&[(7, 1)]),
            Keycode::E => Some(&[(7, 2)]),
            Keycode::Z => Some(&[(7, 3)]), // position physique "W" -> "Z" en AZERTY
            Keycode::S => Some(&[(7, 4)]),
            Keycode::D => Some(&[(7, 5)]),
            Keycode::C => Some(&[(7, 6)]),
            Keycode::X => Some(&[(7, 7)]),

            // Ligne 8
            Keycode::Num1 => Some(&[(8, 0)]),
            Keycode::Num2 => Some(&[(8, 1)]),
            Keycode::Escape => Some(&[(8, 2)]),
            Keycode::A => Some(&[(8, 3)]), // position physique "Q" -> "A" en AZERTY
            Keycode::Tab => Some(&[(8, 4)]),
            Keycode::Q => Some(&[(8, 5)]), // position physique "A" -> "Q" en AZERTY
            Keycode::CapsLock => Some(&[(8, 6)]),
            Keycode::W => Some(&[(8, 7)]), // position physique "Z" -> "W" en AZERTY

            _ => None,
        };

        if let Some(bits) = cpc_key {
            self.apply_bits(bits, pressed);
        }
    }

    pub fn write_current_register(&mut self, val: u8) {
        let reg = self.selected_register as usize;
        if reg < 16 {
            self.registers[reg] = val;
        }
    }

    pub fn read_current_register(&self) -> u8 {
        let reg = self.selected_register as usize;
        if reg == 14 {
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
