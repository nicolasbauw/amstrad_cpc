/// Émulation de la puce sonore AY-3-8910 (PSG) et de la matrice du clavier de l'Amstrad CPC.
///
/// Le PSG gère :
/// 1. La génération de sons (3 canaux de tonalité + 1 canal de bruit, enveloppes).
/// 2. La lecture du clavier (via un port d'I/O connecté à la matrice de touches).
pub struct Psg {
    pub selected_register: u8, // Registre PSG actuellement sélectionné (0 à 15)
    pub registers: [u8; 16],   // Les 16 registres internes du PSG
    pub keyboard_matrix: [u8; 10], // Matrice du clavier : 10 lignes de 8 colonnes (0 = touche pressée)
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
