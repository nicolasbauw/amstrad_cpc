use crate::sound::Sound;
use sdl2::keyboard::{Keycode, Scancode};
use std::collections::HashSet;

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
    /// Position CPC choisie au dernier appui des touches Mac "$ / * / €" et
    /// "< / >", qui dépend du SHIFT Mac à cet instant précis. Mémorisée pour
    /// que le relâchement vise la MÊME position, même si le SHIFT Mac a
    /// changé d'état entre les deux événements : relâcher SHIFT avant la
    /// touche elle-même est un ordre de frappe parfaitement normal, et sans
    /// cette mémoire le relâchement recalculait une position différente de
    /// celle posée à l'appui — le bit d'origine restait alors bloqué "pressé"
    /// indéfiniment, générant des répétitions du caractère via le balayage
    /// clavier du firmware (bug constaté sur clavier réel).
    dollar_asterisk_target: Option<(usize, u8)>,
    /// Position CPC ciblée par la touche "< / >", et si SON relâchement doit
    /// aussi relâcher le SHIFT du CPC (uniquement si cette touche l'a
    /// elle-même synthétisé — voir le commentaire de `Scancode::NonUsBackslash`).
    less_greater_target: Option<((usize, u8), bool)>,
    /// Écritures de bit matrice différées de quelques cycles Z80 (voir
    /// `DEFER_TICKS` et `tick`) : jamais présenter au firmware deux
    /// changements de bit dans la même scrutation clavier. Un vrai clavier
    /// ne produit jamais ça (même en tapant vite, il y a toujours quelques
    /// cycles où un seul doigt a bougé), et l'anti-rebond du firmware
    /// interprète mal ce cas — bug constaté sur clavier réel : SHIFT+$
    /// donnait systématiquement "<" au lieu de "*", à chaque appui, pas
    /// seulement au démarrage. Étaler le relâchement/engagement du SHIFT du
    /// CPC et l'engagement de la position sur deux scrutations distinctes,
    /// comme le ferait naturellement une vraie combinaison SHIFT+touche,
    /// règle le problème.
    deferred: Vec<DeferredBit>,
    /// Partie génératrice de son : les registres restent ici (ils sont aussi la
    /// porte du clavier), la synthèse vit dans son propre module.
    pub sound: Sound,
}

/// Voir le commentaire de `Psg::deferred`.
struct DeferredBit {
    line: usize,
    bit: u8,
    pressed: bool,
    ticks_left: u32,
}

/// ~10 ms à 4 MHz : plusieurs interruptions clavier du CPC (300 Hz, soit
/// ~13 333 cycles chacune) tiennent largement dans ce délai, avec de la
/// marge, tout en restant totalement imperceptible à la frappe humaine.
const DEFER_TICKS: u32 = 40_000;

impl Default for Psg {
    fn default() -> Self {
        Self::new()
    }
}

impl Psg {
    pub fn new() -> Self {
        Self {
            selected_register: 0,
            registers: [0; 16],
            keyboard_matrix: [0xFF; 10],
            selected_keyboard_line: 0,
            controller_state: [0; 8],
            dollar_asterisk_target: None,
            less_greater_target: None,
            deferred: Vec::new(),
            sound: Sound::new(),
        }
    }

    /// Pose ou relâche une position `(ligne, bit)` de la matrice
    /// immédiatement, sans passer par un `Keycode`/`Scancode` — c'est ce que
    /// le clavier virtuel (F7, `bytebox::keyboard_panel`) utilise pour
    /// presser directement une position connue à l'avance, exactement comme
    /// le ferait la touche physique correspondante.
    pub fn set_matrix_bit(&mut self, line: usize, bit: u8, pressed: bool) {
        self.set_bit_now(line, bit, pressed);
    }

    /// Applique en une seule fois le relâchement d'un loquet (SHIFT/CONTROL,
    /// F7) et l'appui d'une nouvelle touche — le cas d'une trame où le
    /// clavier virtuel relâche SHIFT et enfonce une autre touche
    /// *simultanément* (voir `KeyboardPanel::ui`, `ONE_SHOT_LATCHES`), plutôt
    /// que le clavier physique, où chaque évènement SDL arrive dans son
    /// propre appel, jamais groupé.
    ///
    /// Sans précaution, un relâchement et un appui sur la MÊME ligne
    /// matricielle dans le même appel reproduit exactement le saut simultané
    /// de deux bits documenté sur `deferred` ci-dessus (SHIFT vit en ligne 2,
    /// comme `#`/`$`/`*`/`<`/`>` — bug constaté sur clavier virtuel :
    /// SHIFT-clic puis clic sur `#`/`>` donnait `>` au lieu de `#` au premier
    /// essai). Sur une ligne où les deux se croisent, l'appui est donc
    /// différé (`set_bit_deferred`) au lieu d'immédiat, exactement comme pour
    /// les touches ISO Mac plus bas. Les autres lignes (lettres, chiffres...)
    /// ne courent pas ce risque : SHIFT et elles sont lues via des
    /// sélections de ligne distinctes, jamais dans la même scrutation.
    pub fn apply_matrix_diff(
        &mut self,
        released: impl IntoIterator<Item = (usize, u8)>,
        pressed: impl IntoIterator<Item = (usize, u8)>,
    ) {
        let released: Vec<(usize, u8)> = released.into_iter().collect();
        let pressed: Vec<(usize, u8)> = pressed.into_iter().collect();

        let risky_lines: HashSet<usize> = released
            .iter()
            .map(|&(line, _)| line)
            .filter(|line| pressed.iter().any(|&(l, _)| l == *line))
            .collect();

        for &(line, bit) in &released {
            self.cancel_deferred(line, bit);
            self.set_bit_now(line, bit, false);
        }
        for &(line, bit) in &pressed {
            self.cancel_deferred(line, bit);
            if risky_lines.contains(&line) {
                self.set_bit_deferred(line, bit, true);
            } else {
                self.set_bit_now(line, bit, true);
            }
        }
    }

    /// Pose ou relâche un bit de la matrice immédiatement.
    fn set_bit_now(&mut self, line: usize, bit: u8, pressed: bool) {
        if pressed {
            self.keyboard_matrix[line] &= !(1 << bit);
        } else {
            self.keyboard_matrix[line] |= 1 << bit;
        }
    }

    /// Programme un changement de bit `DEFER_TICKS` cycles Z80 plus tard.
    fn set_bit_deferred(&mut self, line: usize, bit: u8, pressed: bool) {
        self.deferred.push(DeferredBit {
            line,
            bit,
            pressed,
            ticks_left: DEFER_TICKS,
        });
    }

    /// Annule un changement différé en attente pour ce bit, s'il y en a un :
    /// une frappe assez brève pour être relâchée avant l'échéance ne doit
    /// pas laisser un appui fantôme s'appliquer après coup.
    fn cancel_deferred(&mut self, line: usize, bit: u8) {
        self.deferred
            .retain(|d| !(d.line == line && d.bit == bit));
    }

    /// Fait avancer la synthèse sonore de `cpu_ticks` cycles Z80 (4 MHz). Le
    /// PSG est cadencé au quart de cette fréquence sur le CPC.
    pub fn tick(&mut self, cpu_ticks: u32) {
        self.sound.tick_cpu(&self.registers, cpu_ticks);

        let mut i = 0;
        while i < self.deferred.len() {
            if self.deferred[i].ticks_left <= cpu_ticks {
                let d = self.deferred.remove(i);
                self.set_bit_now(d.line, d.bit, d.pressed);
            } else {
                self.deferred[i].ticks_left -= cpu_ticks;
                i += 1;
            }
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
    /// morte "^/¨" et caractères hors table SDLK ("ù", "#/@", "$/*/€", "</>"). On se
    /// base ici sur le Scancode, qui reflète la position PHYSIQUE de la touche et
    /// ignore la disposition clavier active (donc insensible aux touches mortes et
    /// au SHIFT). À appeler en PLUS (ou à la place, pour ces touches précises) du
    /// traitement par Keycode dans la boucle d'événements.
    ///
    /// `shift_held` est l'état du SHIFT physique au moment de l'événement.
    ///
    /// Trois de ces touches (Grave, RightBracket, NonUsBackslash) ont chacune deux
    /// caractères Mac (SHIFT ou non) dont les cibles CPC ne sont PAS l'une la
    /// variante shiftée de l'autre (ou, pour NonUsBackslash, le sont mais sur une
    /// touche CPC différente de celle où le SHIFT Mac serait naturellement retombé).
    /// Le SHIFT Mac ne doit donc jamais fuiter tel quel vers le SHIFT du CPC : sans
    /// ce découplage, le bit de position posé ici se combine avec le bit SHIFT CPC
    /// posé en parallèle par la touche SHIFT elle-même (`set_key_state`), et produit
    /// la variante shiftée de la mauvaise touche CPC (bugs constatés : SHIFT+@
    /// donnait ">" au lieu de "#", SHIFT+< donnait "<" au lieu de ">").
    ///
    /// Quand la cible CPC a malgré tout besoin du SHIFT du CPC (cas de "<", plus
    /// bas), il est synthétisé plutôt que réutilisé tel quel : voir le commentaire
    /// de `Psg::deferred` pour pourquoi ce n'est pas un simple bit posé au même
    /// instant que la position.
    ///
    /// Retourne `true` si la touche a été prise en charge ici.
    pub fn set_key_state_scancode(
        &mut self,
        scancode: Scancode,
        pressed: bool,
        shift_held: bool,
    ) -> bool {
        match scancode {
            // "ù / %" -> position du "'" en disposition US
            Scancode::Apostrophe => {
                self.apply_bits(&[(3, 4)], pressed);
                true
            }
            // touche morte "^ / ¨" -> position du "[" en disposition US
            Scancode::LeftBracket => {
                self.apply_bits(&[(3, 2)], pressed);
                true
            }

            // Touche ISO supplémentaire "# / @" (en haut à gauche) -> position du "`"
            // en disposition US. "#" et "@" du Mac visent tous deux le "#" du CPC
            // (2,3), jamais shiftée.
            Scancode::Grave => {
                if pressed {
                    if shift_held {
                        self.set_bit_now(2, 5, false); // relache le SHIFT reel
                        self.set_bit_deferred(2, 3, true); // position, une scrutation plus tard
                    } else {
                        self.set_bit_now(2, 3, true);
                    }
                } else {
                    self.cancel_deferred(2, 3);
                    self.set_bit_now(2, 3, false);
                    self.set_bit_now(2, 5, false);
                }
                true
            }

            // Touche "$ / * / €" du Mac, juste après la touche morte "^/¨" en position
            // physique (donc juste après LeftBracket en scancode). "$" (non shiftée)
            // vise le "$" du CPC (2,6), "*" (shiftée) vise le "*" du CPC (2,1) — deux
            // touches CPC distinctes, ni l'une ni l'autre shiftée.
            //
            // La position n'est choisie qu'au premier appui, et mémorisée dans
            // `dollar_asterisk_target` (verrouillée, pas recalculée à chaque
            // répétition SDL) : le relâchement doit viser la MÊME position, pas en
            // recalculer une nouvelle à partir du SHIFT Mac courant, qui a pu changer
            // entre-temps (relâcher SHIFT avant la touche est un ordre de frappe
            // courant) — sans cette mémoire le bit posé à l'appui restait bloqué
            // "pressé" indéfiniment.
            Scancode::RightBracket => {
                if pressed {
                    if self.dollar_asterisk_target.is_none() {
                        let bit = if shift_held { (2, 1) } else { (2, 6) };
                        self.dollar_asterisk_target = Some(bit);
                        if shift_held {
                            self.set_bit_now(2, 5, false);
                            self.set_bit_deferred(bit.0, bit.1, true);
                        } else {
                            self.set_bit_now(bit.0, bit.1, true);
                        }
                    }
                } else if let Some(bit) = self.dollar_asterisk_target.take() {
                    self.cancel_deferred(bit.0, bit.1);
                    self.set_bit_now(bit.0, bit.1, false);
                    self.set_bit_now(2, 5, false);
                }
                true
            }

            // Touche ISO "< / >" du Mac (bas de clavier, à côté de SHIFT gauche). Le
            // CPC a ses propres touches "*/<" (2,1) et "#/>" (2,3), dont "<" et ">"
            // sont les variantes shiftées : ">" (Mac shifté) coïncide avec le SHIFT
            // réel déjà engagé par la touche SHIFT elle-même, rien à synthétiser, juste
            // la position (2,3). "<" (Mac SANS shift) demande au contraire de
            // synthétiser le SHIFT du CPC nous-mêmes, exactement comme pour le pavé
            // numérique — mais engagé en premier et la position seulement après un
            // court délai, pour reproduire l'ordre naturel d'une vraie combinaison
            // SHIFT+touche plutôt qu'un saut simultané des deux bits.
            //
            // `less_greater_target` retient aussi si CETTE touche a synthétisé le
            // SHIFT du CPC (cas "<") ou s'est appuyée sur celui déjà posé par la
            // touche SHIFT elle-même (cas ">") : au relâchement, ne toucher au bit
            // SHIFT que si on l'a soi-même engagé. Bug constaté sur clavier réel
            // sans cette distinction : relâcher ">" en gardant SHIFT physiquement
            // enfoncé (pour continuer à taper) relâchait quand même le SHIFT du CPC,
            // désynchronisé de la touche SHIFT réelle toujours tenue — l'appui
            // suivant sur cette touche héritait d'un SHIFT CPC déjà faux et
            // retombait sur "#" au lieu de ">".
            Scancode::NonUsBackslash => {
                if pressed {
                    if self.less_greater_target.is_none() {
                        let bit = if shift_held { (2, 3) } else { (2, 1) };
                        self.less_greater_target = Some((bit, !shift_held));
                        if shift_held {
                            self.set_bit_now(bit.0, bit.1, true);
                        } else {
                            self.set_bit_now(2, 5, true);
                            self.set_bit_deferred(bit.0, bit.1, true);
                        }
                    }
                } else if let Some((bit, synthesized_shift)) = self.less_greater_target.take() {
                    self.cancel_deferred(bit.0, bit.1);
                    self.set_bit_now(bit.0, bit.1, false);
                    if synthesized_shift {
                        self.set_bit_now(2, 5, false);
                    }
                }
                true
            }

            _ => false,
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
            // "#", "$", "*" et "</>": gérées par Scancode (Grave, RightBracket,
            // NonUsBackslash) dans `set_key_state_scancode`, pas ici — un Keycode
            // direct ne relâche pas le SHIFT CPC quand le SHIFT Mac est impliqué, et
            // n'est de toute façon pas fiable sur macOS pour ces touches (SDL rapporte
            // le même Keycode qu'il y ait SHIFT ou non), voir le commentaire de cette
            // fonction dans `set_key_state_scancode`.
            Keycode::KpMultiply => Some(&[(2, 1)]), // "*" (pavé numérique)

            // Pas de mapping pour "@" : vérifié sur clavier réel (voir KEYLOG), la
            // variante shiftée de la touche "$" (2,6) donne "à" sur le CPC, pas "@"
            // (l'hypothèse précédente, jamais vérifiée, était fausse). Reste sans
            // solution connue pour l'instant.

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

    /// Un vrai SHIFT physique appuyé en même temps que la touche ISO "# / @"
    /// (Mac) ne doit pas fuiter vers le SHIFT du CPC : les deux caractères Mac
    /// visent la même touche CPC non shiftée "#". Sans le relâchement forcé du
    /// bit SHIFT CPC, la combinaison produisait ">" (variante shiftée d'une
    /// touche voisine) au lieu de "#" — bug constaté sur clavier réel.
    #[test]
    fn the_iso_hash_at_key_never_engages_the_cpc_shift() {
        let mut psg = Psg::new();

        // SHIFT physique déjà enfoncé (ligne 2, bit 5 à 0 = pressé).
        psg.set_key_state(Keycode::LShift, true);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 5), 0, "SHIFT CPC pose");

        // Puis la touche ISO "#/@" (Grave en Scancode), avec SHIFT Mac enfoncé
        // (shift_held = true, ce qui produit "@" côté Mac). Le relâchement du
        // SHIFT est immédiat, seule la position est différée (voir
        // `Psg::deferred`) : on laisse le temps s'écouler pour l'appliquer.
        psg.set_key_state_scancode(Scancode::Grave, true, true);
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC doit etre relache immediatement, malgre le SHIFT Mac enfonce"
        );
        psg.tick(DEFER_TICKS);

        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 3),
            0,
            "le bit de position (2,3) du CPC doit etre pose apres le delai"
        );
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC doit rester relache"
        );
    }

    /// Retour du même phénomène que ci-dessus, mais via le clavier virtuel
    /// (F7) : `sdl.rs` relâche un loquet SHIFT et enfonce une nouvelle touche
    /// dans le MÊME appel (`Psg::apply_matrix_diff`), pas deux évènements SDL
    /// séparés dans le temps comme un clavier physique — reproduit donc le
    /// saut simultané de deux bits sur la ligne 2, avec le même symptôme :
    /// SHIFT-clic puis clic sur "#/>" (2,3) donnait ">" au lieu de "#" au
    /// premier essai, corrigé après coup au deuxième.
    #[test]
    fn virtual_keyboard_shift_release_and_hash_press_never_collide() {
        let mut psg = Psg::new();

        // Trame 1 : SHIFT seul, latché par un premier clic (F7).
        psg.apply_matrix_diff([], [(2, 5)]);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 5), 0, "SHIFT CPC pose");

        // Trame 2 : le loquet SHIFT se relâche ET la position "#/>" s'enfonce
        // dans le MÊME appel (voir `KeyboardPanel::ui`, `release_one_shot_latches`).
        psg.apply_matrix_diff([(2, 5)], [(2, 3)]);
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC doit etre relache immediatement"
        );
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 3),
            1 << 3,
            "la position ne doit PAS encore etre posee, le temps qu'une scrutation propre s'intercale"
        );

        psg.tick(DEFER_TICKS);
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 3),
            0,
            "la position doit etre posee une fois le delai ecoule"
        );
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC doit rester relache"
        );
    }

    /// Une ligne matricielle sans conflit (aucune position relâchée ET
    /// enfoncée dans le même appel) ne doit jamais être différée : ce serait
    /// une latence perceptible sans aucune raison, pour la quasi-totalité des
    /// touches du clavier virtuel.
    #[test]
    fn virtual_keyboard_unrelated_lines_apply_immediately() {
        let mut psg = Psg::new();

        // Ligne 5 (lettre), aucun relâchement ailleurs cette trame.
        psg.apply_matrix_diff([], [(5, 7)]);
        assert_eq!(
            psg.keyboard_matrix[5] & (1 << 7),
            0,
            "doit etre pose immediatement, sans attendre un tick"
        );
    }

    /// La touche Mac "$ / * / €" vise deux touches CPC distinctes et non
    /// shiftées selon le SHIFT Mac : "$" (2,6) sans SHIFT Mac, "*" (2,1) avec.
    /// Ni l'une ni l'autre n'implique le SHIFT du CPC. Sans SHIFT Mac, la
    /// position s'applique immédiatement (transition d'un seul bit, sûre) ;
    /// avec SHIFT Mac, elle est différée (voir `the_iso_hash_at_key_...`).
    #[test]
    fn the_dollar_asterisk_key_targets_two_unshifted_cpc_keys() {
        let mut psg = Psg::new();

        psg.set_key_state_scancode(Scancode::RightBracket, true, false);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 6), 0, "\"$\" du CPC pose");
        assert_eq!(psg.keyboard_matrix[2] & (1 << 5), 1 << 5, "SHIFT CPC relache");
        psg.set_key_state_scancode(Scancode::RightBracket, false, false);

        psg.set_key_state_scancode(Scancode::RightBracket, true, true);
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "SHIFT CPC relache immediatement"
        );
        psg.tick(DEFER_TICKS);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 1), 0, "\"*\" du CPC pose");
        assert_eq!(psg.keyboard_matrix[2] & (1 << 5), 1 << 5, "SHIFT CPC relache");
    }

    /// Relâcher SHIFT avant la touche "$/*/€" elle-même est un ordre de frappe
    /// courant. Bug constaté sur clavier réel : la position CPC ("*", posée à
    /// l'appui avec SHIFT Mac tenu) restait bloquée "pressée" après le
    /// relâchement, parce que celui-ci recalculait une position différente
    /// ("$") à partir du SHIFT Mac déjà retombé — d'où des lignes de "*" en
    /// rafale via le balayage clavier du firmware.
    #[test]
    fn releasing_shift_before_the_dollar_asterisk_key_does_not_leave_a_bit_stuck() {
        let mut psg = Psg::new();

        // Appui avec SHIFT Mac tenu : vise "*" (2,1), appliqué apres le delai.
        psg.set_key_state_scancode(Scancode::RightBracket, true, true);
        psg.tick(DEFER_TICKS);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 1), 0, "\"*\" pose a l'appui");

        // SHIFT deja retombe au moment du relachement de la touche.
        psg.set_key_state_scancode(Scancode::RightBracket, false, false);

        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 1),
            1 << 1,
            "\"*\" doit etre relache, pas rester bloque"
        );
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 6),
            1 << 6,
            "\"$\" ne doit jamais avoir ete engage par ce relachement"
        );
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC doit etre relache par ce relachement, pas engage"
        );
    }

    /// Bug constaté sur clavier réel, distinct du bit bloqué ci-dessus : le
    /// relâchement de "$/*/€" (et, de la même façon, de "#/@" et "</>")
    /// engageait par erreur le SHIFT du CPC au lieu de le relâcher (inversion
    /// de polarité : `set_bit_now(2, 5, true)` au lieu de `false`). La frappe
    /// suivante en héritait, sans lien apparent avec elle-même : un "$" tout
    /// seul, après un premier SHIFT+$, se retrouvait avec le SHIFT du CPC
    /// déjà engagé et affichait "à" au lieu de "$".
    #[test]
    fn releasing_the_dollar_asterisk_key_after_a_shifted_press_releases_the_cpc_shift() {
        let mut psg = Psg::new();

        // SHIFT+$ : vise "*", relache immediat du SHIFT reel, position differee.
        psg.set_key_state_scancode(Scancode::RightBracket, true, true);
        psg.set_key_state_scancode(Scancode::RightBracket, false, true); // SHIFT Mac encore tenu au relachement
        psg.tick(DEFER_TICKS);

        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC ne doit pas rester engage apres ce relachement"
        );

        // "$" seul, ensuite : ne doit pas heriter d'un SHIFT CPC fantome.
        psg.set_key_state_scancode(Scancode::RightBracket, true, false);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 6), 0, "\"$\" pose");
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "SHIFT CPC toujours relache : pas de \"a accent grave\" a la place de \"$\""
        );
    }

    /// Même bug que ci-dessus, sur la touche "#/@" : deux appuis SHIFT+@ puis
    /// @ seul de suite ne doivent pas faire dériver le second vers ">".
    #[test]
    fn releasing_the_hash_at_key_after_a_shifted_press_releases_the_cpc_shift() {
        let mut psg = Psg::new();

        psg.set_key_state_scancode(Scancode::Grave, true, true);
        psg.set_key_state_scancode(Scancode::Grave, false, true);

        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC ne doit pas rester engage apres ce relachement"
        );

        psg.set_key_state_scancode(Scancode::Grave, true, false);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 3), 0, "\"#\" pose");
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "SHIFT CPC toujours relache : pas de \">\" a la place de \"#\""
        );
    }

    /// Un relâchement très rapide (plus court que le délai) ne doit pas
    /// laisser un appui fantôme s'appliquer après coup : l'écriture différée
    /// doit être annulée, pas seulement contredite plus tard.
    #[test]
    fn a_very_short_press_cancels_the_deferred_write() {
        let mut psg = Psg::new();

        psg.set_key_state_scancode(Scancode::RightBracket, true, true);
        psg.set_key_state_scancode(Scancode::RightBracket, false, true);
        psg.tick(DEFER_TICKS);

        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 1),
            1 << 1,
            "\"*\" ne doit jamais s'engager apres un relachement aussi rapide"
        );
    }

    /// Même bug, provoqué par la répétition clavier plutôt que par le
    /// relâchement : SDL répète les événements KeyDown tant que la touche
    /// reste enfoncée. Si SHIFT retombe pendant ce temps, une répétition ne
    /// doit pas migrer vers une nouvelle position CPC sans avoir libéré la
    /// première.
    #[test]
    fn a_key_repeat_does_not_retarget_the_dollar_asterisk_key_mid_hold() {
        let mut psg = Psg::new();

        psg.set_key_state_scancode(Scancode::RightBracket, true, true); // appui initial
        psg.set_key_state_scancode(Scancode::RightBracket, true, false); // repetition, SHIFT retombe
        psg.tick(DEFER_TICKS);

        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 1),
            0,
            "\"*\" doit rester la cible verrouillee malgre la repetition"
        );
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 6),
            1 << 6,
            "\"$\" ne doit pas avoir ete engage par la repetition"
        );

        psg.set_key_state_scancode(Scancode::RightBracket, false, false);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 1), 1 << 1, "\"*\" relache");
    }

    /// "<" et ">" existent bel et bien sur le clavier CPC AZERTY (variantes
    /// shiftées de "*" et "#" respectivement). Keycode::Less/Greater ne sont
    /// pas fiables sur macOS (même défaut que "#/@" et "$/*/€" : SDL rapporte
    /// le même Keycode que SHIFT soit enfoncé ou non) — bug constaté sur
    /// clavier réel : SHIFT+< donnait "<" au lieu de ">". D'où le passage par
    /// Scancode::NonUsBackslash, avec shift_held pour distinguer les deux
    /// cibles.
    #[test]
    fn the_iso_less_greater_key_targets_the_right_cpc_key_via_scancode() {
        // ">" (Mac shifté) : le SHIFT reel est deja engage par la touche SHIFT
        // elle-meme, rien a synthetiser, la position s'applique immediatement.
        let mut psg = Psg::new();
        psg.set_key_state(Keycode::LShift, true);
        psg.set_key_state_scancode(Scancode::NonUsBackslash, true, true);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 3), 0, "\">\" position posee");
        assert_eq!(psg.keyboard_matrix[2] & (1 << 5), 0, "SHIFT CPC deja engage");

        // "<" (Mac SANS shift) : le SHIFT CPC est synthetise, engage
        // immediatement, position differee.
        let mut psg = Psg::new();
        psg.set_key_state_scancode(Scancode::NonUsBackslash, true, false);
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            0,
            "SHIFT CPC synthetise immediatement"
        );
        psg.tick(DEFER_TICKS);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 1), 0, "\"<\" position posee");
        assert_eq!(psg.keyboard_matrix[2] & (1 << 5), 0, "SHIFT CPC toujours engage");
    }

    /// Même bit bloqué que pour "$/*/€" si SHIFT Mac change d'état entre
    /// l'appui et le relâchement de cette touche.
    #[test]
    fn releasing_the_less_greater_key_targets_the_locked_position() {
        let mut psg = Psg::new();

        psg.set_key_state_scancode(Scancode::NonUsBackslash, true, false); // "<"
        psg.tick(DEFER_TICKS);
        psg.set_key_state_scancode(Scancode::NonUsBackslash, false, true); // SHIFT retombe autrement

        assert_eq!(psg.keyboard_matrix[2] & (1 << 1), 1 << 1, "\"<\" relache");
        assert_eq!(psg.keyboard_matrix[2] & (1 << 3), 1 << 3, "\">\" jamais engage");
        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            1 << 5,
            "le SHIFT CPC doit etre relache par ce relachement, pas engage"
        );
    }

    /// Bug constaté sur clavier réel, distinct des précédents : ">" ne
    /// synthétise pas le SHIFT du CPC (il s'appuie sur celui déjà posé par la
    /// touche SHIFT elle-même, encore tenue). Le relâchement de ">" ne doit
    /// donc PAS relâcher le SHIFT du CPC tant que SHIFT reste physiquement
    /// enfoncé, sous peine de désynchroniser le bit CPC de l'état réel : le
    /// prochain appui sur ">" retombait alors sur "#", faute de SHIFT CPC.
    #[test]
    fn releasing_greater_while_shift_is_still_held_keeps_the_cpc_shift_engaged() {
        let mut psg = Psg::new();
        psg.set_key_state(Keycode::LShift, true);

        psg.set_key_state_scancode(Scancode::NonUsBackslash, true, true); // ">"
        psg.set_key_state_scancode(Scancode::NonUsBackslash, false, true); // relache ">", SHIFT toujours tenu

        assert_eq!(
            psg.keyboard_matrix[2] & (1 << 5),
            0,
            "le SHIFT CPC doit rester engage : la touche SHIFT reelle est toujours tenue"
        );

        // Nouvel appui sur ">" : doit encore viser ">", pas retomber sur "#".
        psg.set_key_state_scancode(Scancode::NonUsBackslash, true, true);
        assert_eq!(psg.keyboard_matrix[2] & (1 << 3), 0, "\">\" pose de nouveau");
        assert_eq!(psg.keyboard_matrix[2] & (1 << 5), 0, "SHIFT CPC toujours engage");
    }
}
