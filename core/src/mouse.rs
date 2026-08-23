/// Émulation d'une souris logicielle propre à ByteBox — pas la
/// reproduction d'un adaptateur matériel réel (AMX Mouse, SYMBiFACE II/
/// X-MEM...). Décision prise après recherche pour le projet Dune-CPC : la
/// doc fiable de ces standards a disparu, et le public possédant
/// physiquement ce hardware est quasi inexistant ; comme ByteBox et le jeu
/// qui l'utilise sont développés par la même équipe, il n'y a aucun gain de
/// compatibilité à chasser un protocole externe non fiabilisé. Voir
/// `doc/souris.md` du dépôt `dune-cpc`.
///
/// Le protocole choisi reste inspiré des souris relatives classiques
/// (deltas X/Y + état des boutons), par choix de conception général — pas
/// pour imiter un standard existant.
pub struct Mouse {
    pub enabled: bool,
    /// Déplacement horizontal accumulé depuis la dernière lecture du
    /// registre X, en points d'écran. Saturé (pas enroulé) à `i8::MIN`/`MAX`
    /// pour qu'un mouvement plus rapide que le taux de lecture du Z80 ne
    /// bascule pas silencieusement de signe.
    dx: i32,
    dy: i32,
    /// Bit 0 = bouton gauche, bit 1 = bouton droit, bit 2 = bouton du
    /// milieu. État courant, pas consommé à la lecture (contrairement aux
    /// deltas) : un bouton maintenu doit continuer à se lire enfoncé tant
    /// qu'il ne remonte pas.
    buttons: u8,
}

impl Default for Mouse {
    fn default() -> Self {
        Self::new()
    }
}

impl Mouse {
    pub fn new() -> Self {
        Self {
            enabled: false,
            dx: 0,
            dy: 0,
            buttons: 0,
        }
    }

    /// Accumule un mouvement relatif (appelé par la façade de présentation
    /// à chaque évènement souris de l'OS/SDL). Sans effet si la souris est
    /// désactivée : évite d'accumuler un delta qui ne sera jamais lu et
    /// surprendrait au prochain `enabled = true`.
    pub fn on_motion(&mut self, dx: i32, dy: i32) {
        if !self.enabled {
            return;
        }
        self.dx = self.dx.saturating_add(dx);
        self.dy = self.dy.saturating_add(dy);
    }

    /// Bit correspondant à `button` (0 = gauche, 1 = droit, 2 = milieu).
    /// Les index hors de ces trois boutons sont ignorés plutôt que de
    /// paniquer : une façade qui reçoit un évènement de bouton exotique
    /// (molette cliquable, boutons latéraux...) n'a pas à filtrer elle-même
    /// avant d'appeler cette fonction.
    pub fn set_button(&mut self, button: u8, pressed: bool) {
        if button > 2 {
            return;
        }
        if pressed {
            self.buttons |= 1 << button;
        } else {
            self.buttons &= !(1 << button);
        }
    }

    /// Lit et consomme le delta X accumulé, saturé sur un octet signé. La
    /// lecture remet le compteur à zéro : c'est ce qui permet au Z80 de
    /// lire "le mouvement depuis la dernière fois" sans registre de contrôle
    /// séparé pour accuser réception.
    pub fn read_dx(&mut self) -> u8 {
        let value = self.dx.clamp(i8::MIN as i32, i8::MAX as i32) as i8;
        self.dx = 0;
        value as u8
    }

    /// Voir `read_dx`.
    pub fn read_dy(&mut self) -> u8 {
        let value = self.dy.clamp(i8::MIN as i32, i8::MAX as i32) as i8;
        self.dy = 0;
        value as u8
    }

    /// Lit l'état courant des boutons — pas consommé, contrairement aux
    /// deltas (voir le champ `buttons`).
    pub fn read_buttons(&self) -> u8 {
        self.buttons
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_is_ignored_while_disabled() {
        let mut mouse = Mouse::new();
        mouse.on_motion(5, -3);
        assert_eq!(mouse.read_dx(), 0);
        assert_eq!(mouse.read_dy(), 0);
    }

    #[test]
    fn reading_a_delta_consumes_it() {
        let mut mouse = Mouse::new();
        mouse.enabled = true;
        mouse.on_motion(5, -3);

        assert_eq!(mouse.read_dx(), 5);
        assert_eq!(mouse.read_dx(), 0, "une deuxieme lecture doit rendre 0");

        assert_eq!(mouse.read_dy() as i8, -3);
    }

    #[test]
    fn several_motions_accumulate_before_being_read() {
        let mut mouse = Mouse::new();
        mouse.enabled = true;
        mouse.on_motion(2, 1);
        mouse.on_motion(3, 1);

        assert_eq!(mouse.read_dx(), 5);
        assert_eq!(mouse.read_dy(), 2);
    }

    #[test]
    fn a_delta_faster_than_the_read_rate_saturates_instead_of_wrapping() {
        let mut mouse = Mouse::new();
        mouse.enabled = true;
        mouse.on_motion(1000, -1000);

        assert_eq!(mouse.read_dx() as i8, i8::MAX);
        assert_eq!(mouse.read_dy() as i8, i8::MIN);
    }

    #[test]
    fn buttons_reflect_current_state_and_are_not_consumed_by_reading() {
        let mut mouse = Mouse::new();
        mouse.set_button(0, true);
        mouse.set_button(2, true);

        assert_eq!(mouse.read_buttons(), 0b101);
        assert_eq!(mouse.read_buttons(), 0b101, "la lecture ne doit pas consommer l'etat");

        mouse.set_button(0, false);
        assert_eq!(mouse.read_buttons(), 0b100);
    }

    #[test]
    fn an_out_of_range_button_index_is_ignored() {
        let mut mouse = Mouse::new();
        mouse.set_button(7, true);
        assert_eq!(mouse.read_buttons(), 0);
    }
}
