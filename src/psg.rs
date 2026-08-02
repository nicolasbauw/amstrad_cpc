use crate::sound::Sound;
use sdl2::keyboard::{Keycode, Scancode};

/// Largeur réelle de chaque registre du PSG. Les bits en trop sont perdus à
/// l'écriture, et une relecture ne les rend donc jamais : du code qui teste la
/// présence du PSG écrit souvent &FF dans un registre étroit pour vérifier
/// qu'il se relit tronqué.
const REGISTER_MASKS: [u8; 16] = [
    0xFF, 0x0F, // R0/R1  : période canal A (12 bits)
    0xFF, 0x0F, // R2/R3  : période canal B
    0xFF, 0x0F, // R4/R5  : période canal C
    0x1F, // R6     : période du bruit (5 bits)
    0xFF, // R7     : mélangeur
    0x1F, 0x1F, 0x1F, // R8/R9/R10 : amplitudes (4 bits + bit 4 "enveloppe")
    0xFF, 0xFF, // R11/R12 : période d'enveloppe (16 bits)
    0x0F, // R13    : forme d'enveloppe
    0xFF, 0xFF, // R14/R15 : ports d'E/S (clavier sur CPC)
];

// Émulation de la puce sonore AY-3-8910 (PSG) et de la matrice du clavier de l'Amstrad CPC.
pub struct Psg {
    pub selected_register: u8,
    pub registers: [u8; 16],
    pub keyboard_matrix: [u8; 10],
    pub selected_keyboard_line: u8,
    pub controller_state: [u8; 8], // 0-7: bits de la manette (Up, Down, Left, Right, Fire1, Fire2, Fire3, Fire4)
    /// Partie génératrice de son : les registres restent ici (ils sont aussi la
    /// porte du clavier), la synthèse vit dans son propre module.
    pub sound: Sound,
}

impl Psg {
    pub fn new() -> Self {
        Self {
            selected_register: 0,
            registers: [0; 16],
            keyboard_matrix: [0xFF; 10],
            selected_keyboard_line: 0,
            controller_state: [0; 8],
            sound: Sound::new(),
        }
    }

    /// Fait avancer la synthèse sonore de `cpu_ticks` cycles Z80 (4 MHz). Le
    /// PSG est cadencé au quart de cette fréquence sur le CPC.
    pub fn tick(&mut self, cpu_ticks: u32) {
        self.sound.tick_cpu(&self.registers, cpu_ticks);
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
            self.registers[reg] = val & REGISTER_MASKS[reg];

            // R13 est le seul registre à effet de bord : toute écriture, même
            // de la valeur déjà en place, redémarre le générateur d'enveloppe.
            if reg == 13 {
                self.sound.write_envelope_shape(self.registers[13]);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write(psg: &mut Psg, reg: u8, value: u8) {
        psg.selected_register = reg;
        psg.write_current_register(value);
    }

    #[test]
    fn register_writes_are_masked_to_the_hardware_width() {
        let mut psg = Psg::new();
        for reg in 0..16u8 {
            write(&mut psg, reg, 0xFF);
        }

        // Ce que le composant rend n'est jamais plus large que le registre :
        // c'est ainsi que du code détecte un vrai PSG.
        assert_eq!(psg.registers[1] & 0xF0, 0, "R1 ne garde que 4 bits");
        assert_eq!(psg.registers[3] & 0xF0, 0, "R3 ne garde que 4 bits");
        assert_eq!(psg.registers[5] & 0xF0, 0, "R5 ne garde que 4 bits");
        assert_eq!(psg.registers[6], 0x1F, "R6 ne garde que 5 bits");
        assert_eq!(psg.registers[8], 0x1F, "R8 ne garde que 5 bits");
        assert_eq!(psg.registers[13], 0x0F, "R13 ne garde que 4 bits");
        // Les registres pleine largeur passent tels quels.
        assert_eq!(psg.registers[0], 0xFF);
        assert_eq!(psg.registers[7], 0xFF);
        assert_eq!(psg.registers[11], 0xFF);
    }

    #[test]
    fn writing_r13_restarts_the_envelope_even_with_the_same_value() {
        let mut psg = Psg::new();
        write(&mut psg, 11, 0x10); // période d'enveloppe
        write(&mut psg, 12, 0x00);
        write(&mut psg, 13, 0x08); // rampe descendante répétée

        psg.tick(4 * 16 * 0x10 * 5); // cinq pas de rampe
        assert_eq!(psg.sound.envelope_volume(), 10);

        write(&mut psg, 13, 0x08);
        assert_eq!(
            psg.sound.envelope_volume(),
            15,
            "reecrire R13 doit relancer l'enveloppe"
        );
    }

    #[test]
    fn writing_another_register_does_not_disturb_the_envelope() {
        let mut psg = Psg::new();
        write(&mut psg, 11, 0x10);
        write(&mut psg, 13, 0x08);
        psg.tick(4 * 16 * 0x10 * 5);

        write(&mut psg, 8, 15);
        assert_eq!(psg.sound.envelope_volume(), 10);
    }

    #[test]
    fn the_psg_advances_at_a_quarter_of_the_cpu_clock() {
        let mut psg = Psg::new();
        write(&mut psg, 7, 0x3E); // ton A seul
        write(&mut psg, 0, 100);
        write(&mut psg, 8, 15);

        // Une seconde de temps CPU, découpée en paquets irréguliers comme le
        // fait la boucle d'exécution (les instructions Z80 ne durent pas
        // toutes un multiple de 4 cycles).
        let mut ticks = 0u32;
        let mut i = 0;
        while ticks < 4 * crate::sound::PSG_CLOCK {
            let chunk = [4u32, 7, 11, 13][i % 4].min(4 * crate::sound::PSG_CLOCK - ticks);
            psg.tick(chunk);
            ticks += chunk;
            i += 1;
        }

        let produced = psg.sound.buffered_samples();
        assert!(
            produced.abs_diff(crate::sound::SAMPLE_RATE as usize) <= 2,
            "{produced} echantillons pour une seconde de CPU"
        );
    }

    /// Le générateur de son partage ses registres avec la porte du clavier :
    /// R14 doit continuer de rendre la ligne sélectionnée, pas la valeur
    /// écrite dans le registre.
    #[test]
    fn register_14_still_reads_the_selected_keyboard_line() {
        let mut psg = Psg::new();
        psg.set_key_state(Keycode::Space, true); // ligne 5, bit 7

        write(&mut psg, 14, 0xFF);
        psg.selected_keyboard_line = 5;
        psg.selected_register = 14;

        assert_eq!(psg.read_current_register(), 0b0111_1111);
    }
}
