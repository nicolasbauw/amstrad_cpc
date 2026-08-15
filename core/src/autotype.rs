//! Frappe automatique d'une commande au démarrage (`--autocmd`), comme le
//! propose Caprice32.
//!
//! Utile pour deux choses : contourner un clavier ou une manette qui ne
//! répondent pas (c'est ainsi qu'on a pu lancer `RUN"BARBA.I` sur Barbarian
//! avant que le PPI ne soit corrigé), et surtout accélérer les allers-retours
//! de débogage — plus besoin de retaper la même commande à la main à chaque
//! lancement.

use crate::app_log;
use crate::psg::Psg;
use std::collections::VecDeque;

/// Une commande `--autocmd`/`-a` sans retour à la ligne resterait tapée mais
/// jamais validée : BASIC ne l'exécute qu'après ENTRÉE. Ajoute cette ENTRÉE
/// systématiquement, sauf si l'appelant l'a déjà fournie — pour ne pas
/// envoyer un second ENTRÉE parasite, qui pourrait interagir avec ce que le
/// jeu affiche juste après (un menu, un choix de touche...).
pub fn ensure_validated(command: &str) -> String {
    if command.ends_with('\n') {
        command.to_string()
    } else {
        format!("{command}\n")
    }
}

/// Position d'une touche dans la matrice clavier du CPC (ligne, bit), et si
/// SHIFT doit être maintenu pour produire ce caractère.
///
/// La ligne des chiffres du clavier français est inversée par rapport à un
/// clavier US : sans SHIFT elle tape des symboles (`&` `é` `"` `'` `(` `-`
/// `è` `_` `ç` `à`), c'est SHIFT qui donne les chiffres. Vérifié
/// empiriquement sur cet émulateur : `PRINT 1` sans SHIFT tape `PRINT &`,
/// une erreur de syntaxe ; avec SHIFT, `PRINT 0123456789` affiche bien
/// `123456789`.
pub(crate) fn key_for_char(c: char) -> Option<((usize, u8), bool)> {
    let (position, shift) = match c.to_ascii_uppercase() {
        'A' => ((8, 3), false),
        'B' => ((6, 6), false),
        'C' => ((7, 6), false),
        'D' => ((7, 5), false),
        'E' => ((7, 2), false),
        'F' => ((6, 5), false),
        'G' => ((6, 4), false),
        'H' => ((5, 4), false),
        'I' => ((4, 3), false),
        'J' => ((5, 5), false),
        'K' => ((4, 5), false),
        'L' => ((4, 4), false),
        'M' => ((3, 5), false),
        'N' => ((5, 6), false),
        'O' => ((4, 2), false),
        'P' => ((3, 3), false),
        'Q' => ((8, 5), false),
        'R' => ((6, 2), false),
        'S' => ((7, 4), false),
        'T' => ((6, 3), false),
        'U' => ((5, 2), false),
        'V' => ((6, 7), false),
        'W' => ((8, 7), false),
        'X' => ((7, 7), false),
        'Y' => ((5, 3), false),
        'Z' => ((7, 3), false),
        '0' => ((4, 0), true),
        '1' => ((8, 0), true),
        '2' => ((8, 1), true),
        '3' => ((7, 1), true),
        '4' => ((7, 0), true),
        '5' => ((6, 1), true),
        '6' => ((6, 0), true),
        '7' => ((5, 1), true),
        '8' => ((5, 0), true),
        '9' => ((4, 1), true),
        ' ' => ((5, 7), false),
        '\n' | '\r' => ((2, 2), false),
        '"' => ((7, 1), false), // même touche que "3", mais sans SHIFT
        '.' => ((4, 7), true),  // touche ";" ; SHIFT donne "."
        ':' => ((3, 7), false),
        ',' => ((4, 6), false),
        '-' => ((3, 0), false),
        // Préfixe RSX sur cette ROM AZERTY ("ùtape", pas "|tape" —
        // confirmé sur clavier réel). Même position que la vraie frappe
        // clavier, voir `psg::Psg::set_key_state_scancode`.
        'ù' | 'Ù' => ((3, 4), false),
        _ => return None,
    };
    Some((position, shift))
}

/// Position de SHIFT dans la matrice clavier.
const SHIFT: (usize, u8) = (2, 5);

/// Durée d'appui d'une touche, en cycles Z80 (4 MHz). Éprouvée dans les
/// nombreux bancs d'essai qui ont piloté le clavier émulé pendant les
/// séances de mise au point : assez longue pour que le balayage clavier du
/// firmware la voie de façon fiable.
const PRESS_TICKS: u32 = 160_000;

/// Délai entre le relâchement d'une touche et l'appui de la suivante.
const RELEASE_TICKS: u32 = 240_000;

/// Délai avant la première touche : le temps que la ROM affiche son bandeau
/// de démarrage et atteigne l'invite BASIC. Une commande tapée trop tôt se
/// perd dans le vide. Trois secondes à 4 MHz : la valeur qui s'est montrée
/// fiable dans les très nombreux bancs d'essai pilotant ce clavier émulé
/// pendant les séances de mise au point du projet.
const STARTUP_DELAY_TICKS: u32 = 12_000_000;

enum State {
    Waiting(u32),
    Pressed {
        key: (usize, u8),
        shift: bool,
        ticks_left: u32,
    },
    Released(u32),
    Done,
}

/// Pilote la frappe d'une commande, touche par touche, au rythme de
/// l'horloge Z80 plutôt que du temps réel : c'est ce qui permet à
/// l'injection de fonctionner aussi bien à vitesse normale qu'en avance
/// rapide de débogage.
pub struct AutoTyper {
    keys: VecDeque<((usize, u8), bool)>,
    state: State,
}

impl AutoTyper {
    /// Prépare la frappe de `command`. Les caractères sans équivalent connu
    /// sur le clavier du CPC sont ignorés, avec un avertissement immédiat :
    /// mieux vaut le savoir avant de chercher pourquoi la commande ne s'est
    /// pas exécutée comme prévu.
    pub fn new(command: &str) -> Self {
        let mut keys = VecDeque::new();
        let mut skipped: Vec<char> = Vec::new();
        for c in command.chars() {
            match key_for_char(c) {
                Some(k) => keys.push_back(k),
                None => skipped.push(c),
            }
        }
        if !skipped.is_empty() {
            app_log!("Autocmd: character(s) skipped, no keyboard equivalent: {skipped:?}");
        }
        Self {
            keys,
            state: State::Waiting(STARTUP_DELAY_TICKS),
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.state, State::Done)
    }

    /// Fait avancer la frappe de `elapsed_ticks` cycles CPU, en enfonçant ou
    /// relâchant des touches sur `psg` au besoin. À appeler après chaque
    /// `Machine::step()`.
    pub fn advance(&mut self, psg: &mut Psg, elapsed_ticks: u32) {
        // Le compte à rebours d'un état peut tomber à zéro (le passage de
        // "en attente" à "appuie la première touche" ne coûte aucun cycle) :
        // la boucle continue tant qu'il reste des transitions immédiates à
        // effectuer, même à budget épuisé. Elle termine forcément, puisque
        // chaque état à zéro cycle est suivi d'un état à durée non nulle.
        let mut budget = elapsed_ticks;
        while !self.is_done() {
            let countdown = match &self.state {
                State::Waiting(t) | State::Released(t) => *t,
                State::Pressed { ticks_left, .. } => *ticks_left,
                State::Done => return,
            };

            if countdown > budget {
                self.reduce_countdown(budget);
                return;
            }
            budget -= countdown;
            self.reduce_countdown(countdown);
            self.transition(psg);
        }
    }

    fn reduce_countdown(&mut self, amount: u32) {
        match &mut self.state {
            State::Waiting(t) | State::Released(t) => *t -= amount,
            State::Pressed { ticks_left, .. } => *ticks_left -= amount,
            State::Done => {}
        }
    }

    /// Un changement d'état, une fois son compte à rebours écoulé : la
    /// touche en attente s'enfonce, celle enfoncée se relâche.
    fn transition(&mut self, psg: &mut Psg) {
        self.state = match &self.state {
            State::Waiting(_) => State::Released(0),
            State::Released(_) => match self.keys.pop_front() {
                None => State::Done,
                Some((key, shift)) => {
                    if shift {
                        psg.keyboard_matrix[SHIFT.0] &= !(1 << SHIFT.1);
                    }
                    psg.keyboard_matrix[key.0] &= !(1 << key.1);
                    State::Pressed {
                        key,
                        shift,
                        ticks_left: PRESS_TICKS,
                    }
                }
            },
            State::Pressed { key, shift, .. } => {
                psg.keyboard_matrix[key.0] |= 1 << key.1;
                if *shift {
                    psg.keyboard_matrix[SHIFT.0] |= 1 << SHIFT.1;
                }
                State::Released(RELEASE_TICKS)
            }
            State::Done => State::Done,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cas qui a échappé aux tests précédents : une commande donnée sans
    /// ENTRÉE final se tapait, mais ne validait jamais rien. Repéré à
    /// l'usage, pas par un test — d'où celui-ci.
    #[test]
    fn a_command_without_a_trailing_newline_gets_one() {
        assert_eq!(ensure_validated("RUN\"BARBA.I"), "RUN\"BARBA.I\n");
    }

    #[test]
    fn a_command_that_already_ends_with_a_newline_is_left_alone() {
        assert_eq!(ensure_validated("RUN\"BARBA.I\n"), "RUN\"BARBA.I\n");
    }

    #[test]
    fn an_empty_command_still_gets_a_newline() {
        assert_eq!(ensure_validated(""), "\n");
    }

    fn pressed_keys(psg: &Psg) -> Vec<(usize, u8)> {
        let mut v = Vec::new();
        for (line, mask) in psg.keyboard_matrix.iter().enumerate() {
            for bit in 0..8 {
                if mask & (1 << bit) == 0 {
                    v.push((line, bit));
                }
            }
        }
        v
    }

    #[test]
    fn nothing_is_pressed_before_the_startup_delay_elapses() {
        let mut psg = Psg::new();
        let mut typer = AutoTyper::new("A");
        typer.advance(&mut psg, STARTUP_DELAY_TICKS - 1);
        assert!(pressed_keys(&psg).is_empty());
        assert!(!typer.is_done());
    }

    #[test]
    fn a_single_character_is_pressed_then_released() {
        let mut psg = Psg::new();
        let mut typer = AutoTyper::new("A");

        typer.advance(&mut psg, STARTUP_DELAY_TICKS);
        assert_eq!(pressed_keys(&psg), vec![(8, 3)], "A doit etre enfoncee");

        typer.advance(&mut psg, PRESS_TICKS);
        assert!(pressed_keys(&psg).is_empty(), "puis relachee");

        typer.advance(&mut psg, RELEASE_TICKS);
        assert!(typer.is_done());
    }

    /// Les chiffres exigent SHIFT sur ce clavier : sans lui, on tape le
    /// symbole de la même touche (par ex. "&" au lieu de "1").
    #[test]
    fn digits_are_typed_with_shift_held() {
        let mut psg = Psg::new();
        let mut typer = AutoTyper::new("1");
        typer.advance(&mut psg, STARTUP_DELAY_TICKS);
        let mut keys = pressed_keys(&psg);
        keys.sort();
        assert_eq!(keys, vec![(2, 5), (8, 0)], "SHIFT et la touche du chiffre");
    }

    /// La casse ne change rien : "a" et "A" tapent la même touche. Les mots
    /// clés BASIC et les noms de fichiers sont insensibles à la casse sur
    /// CPC, et ce clavier ne fait de toute façon pas la distinction.
    #[test]
    fn lowercase_and_uppercase_letters_use_the_same_key() {
        assert_eq!(key_for_char('a'), key_for_char('A'));
    }

    #[test]
    fn an_unmapped_character_is_skipped_without_blocking_the_rest() {
        let mut psg = Psg::new();
        let mut typer = AutoTyper::new("A€B");

        typer.advance(&mut psg, STARTUP_DELAY_TICKS);
        assert_eq!(pressed_keys(&psg), vec![(8, 3)], "A");
        typer.advance(&mut psg, PRESS_TICKS + RELEASE_TICKS);
        assert_eq!(
            pressed_keys(&psg),
            vec![(6, 6)],
            "B, le caractere ignore n'a rien bloque"
        );
    }

    #[test]
    fn a_full_command_ends_up_done_and_releases_every_key() {
        let mut psg = Psg::new();
        let mut typer = AutoTyper::new("RUN\"A\n");
        let mut ticks = 0u32;
        let budget = STARTUP_DELAY_TICKS + 6 * (PRESS_TICKS + RELEASE_TICKS) + 1_000;
        while !typer.is_done() {
            typer.advance(&mut psg, 1000);
            ticks += 1000;
            assert!(ticks < budget, "la frappe ne doit pas s'eterniser");
        }
        assert!(
            pressed_keys(&psg).is_empty(),
            "aucune touche ne doit rester enfoncee"
        );
    }

    #[test]
    fn quote_and_digit_three_share_a_key_but_differ_by_shift() {
        assert_eq!(key_for_char('"'), Some(((7, 1), false)));
        assert_eq!(key_for_char('3'), Some(((7, 1), true)));
    }

    /// Le préfixe RSX sur cette ROM est "ù", pas "|" — confirmé sur clavier
    /// réel par l'utilisateur ("ùtape" charge la cassette, "|tape" ne fait
    /// rien). Testé bout en bout : vérifie qu'`AutoTyper` sait le taper.
    #[test]
    fn the_rsx_prefix_is_u_grave_not_pipe() {
        assert_eq!(key_for_char('ù'), Some(((3, 4), false)));
        assert_eq!(key_for_char('Ù'), Some(((3, 4), false)));
    }
}
