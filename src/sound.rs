//! Synthèse sonore du PSG AY-3-8912 du CPC.
//!
//! Le composant contient trois générateurs de tons carrés, un générateur de
//! bruit, un générateur d'enveloppe et un mélangeur. Le module ne s'occupe que
//! de la partie son : les registres appartiennent au `Psg` (qui est aussi la
//! porte du clavier) et sont passés à chaque avance de temps. La sortie est
//! une suite d'échantillons mono normalisés, consommée par `audio.rs`.

use std::collections::VecDeque;

/// Horloge du PSG sur CPC : l'horloge Z80 de 4 MHz divisée par 4.
pub const PSG_CLOCK: u32 = 1_000_000;

/// Rapport entre l'horloge du CPU et celle du PSG.
const CPU_TO_PSG: u32 = 4;

/// Prédiviseur interne du PSG. Les compteurs de période sont cadencés à
/// PSG_CLOCK / 8, ce qui donne bien la formule du constructeur pour un ton :
/// f = horloge / (16 * période), le compteur inversant la sortie (donc une
/// demi-période) à chaque expiration.
const PRESCALER: u32 = 8;

/// Fréquence d'échantillonnage produite par le mélangeur.
pub const SAMPLE_RATE: u32 = 44_100;

/// Plafond du tampon d'échantillons (1 seconde). Sans lui, une émulation qui
/// tourne sans que personne ne vienne consommer le son ferait grossir le
/// tampon indéfiniment (émulation en pause côté SDL, tests, etc.).
const MAX_BUFFERED_SAMPLES: usize = SAMPLE_RATE as usize;

/// Amplitude de chacun des 16 niveaux de volume du PSG. L'échelle est
/// logarithmique (environ -3 dB par pas), et non linéaire : une rampe
/// linéaire donnerait des enveloppes au son nettement faux.
pub const VOLUME_TABLE: [f32; 16] = [
    0.0000, 0.0137, 0.0205, 0.0291, 0.0423, 0.0618, 0.0847, 0.1369, 0.1691, 0.2647, 0.3527, 0.4499,
    0.5704, 0.6873, 0.8482, 1.0000,
];

/// État complet de la partie sonore du PSG.
pub struct Sound {
    // --- Générateurs de tons ---
    tone_counter: [u16; 3],
    tone_state: [bool; 3],

    // --- Générateur de bruit ---
    noise_counter: u16,
    /// Division par deux supplémentaire : le registre à décalage du bruit
    /// n'avance qu'une expiration de compteur sur deux.
    noise_prescaler: bool,
    /// Registre à décalage à rebouclage linéaire de 17 bits (prises 0 et 3).
    lfsr: u32,

    // --- Générateur d'enveloppe ---
    env_counter: u32,
    env_prescaler: bool,
    /// Position dans la rampe, décomptée de 15 à 0.
    env_step: i32,
    /// Masque d'inversion de la rampe : 0x0F en montée, 0x00 en descente.
    env_attack: u8,
    env_alternate: bool,
    env_hold: bool,
    env_holding: bool,
    env_volume: u8,

    // --- Conversion de temps et échantillonnage ---
    /// Cycles CPU pas encore convertis en cycles PSG.
    cpu_remainder: u32,
    prescaler: u32,
    /// Accumulateur de phase de l'échantillonnage, en unités de SAMPLE_RATE.
    sample_phase: u32,
    /// Somme et longueur de la fenêtre d'intégration de l'échantillon courant.
    /// Moyenner tous les cycles PSG d'un échantillon fait office de filtre
    /// anti-repliement : sans lui, les tons aigus produisent des harmoniques
    /// parasites très audibles.
    sample_sum: f32,
    sample_len: u32,
    samples: VecDeque<f32>,
}

/// Vue décodée des registres, calculée une fois par appel plutôt qu'à chaque
/// cycle PSG.
struct Params {
    tone_period: [u16; 3],
    noise_period: u16,
    mixer: u8,
    amplitude: [u8; 3],
    env_period: u32,
}

impl Params {
    fn from_registers(regs: &[u8; 16]) -> Self {
        // Une période nulle se comporte comme une période de 1 : le compteur
        // expire à chaque cycle du prédiviseur.
        let period = |fine: u8, coarse: u8| {
            let p = (fine as u16) | ((coarse as u16 & 0x0F) << 8);
            p.max(1)
        };
        Self {
            tone_period: [
                period(regs[0], regs[1]),
                period(regs[2], regs[3]),
                period(regs[4], regs[5]),
            ],
            noise_period: (regs[6] & 0x1F).max(1) as u16,
            mixer: regs[7],
            amplitude: [regs[8] & 0x1F, regs[9] & 0x1F, regs[10] & 0x1F],
            env_period: ((regs[11] as u32) | ((regs[12] as u32) << 8)).max(1),
        }
    }
}

impl Sound {
    pub fn new() -> Self {
        Self {
            tone_counter: [0; 3],
            tone_state: [false; 3],
            noise_counter: 0,
            noise_prescaler: false,
            lfsr: 1,
            env_counter: 0,
            env_prescaler: false,
            env_step: 15,
            env_attack: 0,
            env_alternate: false,
            env_hold: true,
            env_holding: false,
            env_volume: 15,
            cpu_remainder: 0,
            prescaler: 0,
            sample_phase: 0,
            sample_sum: 0.0,
            sample_len: 0,
            samples: VecDeque::new(),
        }
    }

    /// Avance la synthèse de `cpu_ticks` cycles Z80. Le reste de la division
    /// par 4 est conservé : perdre 1 à 3 cycles à chaque instruction
    /// désaccorderait lentement toute la machine.
    pub fn tick_cpu(&mut self, regs: &[u8; 16], cpu_ticks: u32) {
        self.cpu_remainder += cpu_ticks;
        let psg_cycles = self.cpu_remainder / CPU_TO_PSG;
        self.cpu_remainder %= CPU_TO_PSG;
        self.run(regs, psg_cycles);
    }

    /// Avance la synthèse de `cycles` cycles PSG (1 MHz).
    pub fn run(&mut self, regs: &[u8; 16], cycles: u32) {
        if cycles == 0 {
            return;
        }
        let p = Params::from_registers(regs);
        // Le niveau ne peut changer qu'à un cycle du prédiviseur (ou sur une
        // écriture de registre, donc entre deux appels) : le calculer à chaque
        // cycle PSG coûterait huit fois plus cher pour le même résultat.
        let mut level = self.mix(&p);

        for _ in 0..cycles {
            self.prescaler += 1;
            if self.prescaler >= PRESCALER {
                self.prescaler = 0;
                self.step_generators(&p);
                level = self.mix(&p);
            }

            self.sample_sum += level;
            self.sample_len += 1;

            self.sample_phase += SAMPLE_RATE;
            if self.sample_phase >= PSG_CLOCK {
                self.sample_phase -= PSG_CLOCK;
                let sample = self.sample_sum / self.sample_len as f32;
                self.sample_sum = 0.0;
                self.sample_len = 0;
                self.push_sample(sample);
            }
        }
    }

    /// Un cycle du prédiviseur : tons, bruit et enveloppe.
    fn step_generators(&mut self, p: &Params) {
        for channel in 0..3 {
            self.tone_counter[channel] += 1;
            if self.tone_counter[channel] >= p.tone_period[channel] {
                self.tone_counter[channel] = 0;
                self.tone_state[channel] = !self.tone_state[channel];
            }
        }

        // Bruit et enveloppe expirent deux fois plus lentement que les tons :
        // une expiration sur deux seulement les fait avancer.
        self.noise_counter += 1;
        if self.noise_counter >= p.noise_period {
            self.noise_counter = 0;
            self.noise_prescaler = !self.noise_prescaler;
            if !self.noise_prescaler {
                self.step_noise();
            }
        }

        self.env_counter += 1;
        if self.env_counter >= p.env_period {
            self.env_counter = 0;
            self.env_prescaler = !self.env_prescaler;
            if !self.env_prescaler {
                self.advance_envelope();
            }
        }
    }

    /// Un décalage du registre du bruit : séquence pseudo-aléatoire de
    /// 2^17 - 1 états, avec les prises en bits 0 et 3.
    fn step_noise(&mut self) {
        let feedback = (self.lfsr ^ (self.lfsr >> 3)) & 1;
        self.lfsr = (self.lfsr >> 1) | (feedback << 16);
    }

    /// Écriture de R13 : la forme est décodée et l'enveloppe repart de zéro.
    ///
    /// Bits : 3 = CONT, 2 = ATT (montée), 1 = ALT, 0 = HOLD. Une forme sans
    /// CONT ne joue qu'une seule rampe puis se tait, ce qui revient exactement
    /// à HOLD avec ALT égal à ATT.
    pub fn write_envelope_shape(&mut self, shape: u8) {
        self.env_attack = if shape & 0x04 != 0 { 0x0F } else { 0x00 };
        if shape & 0x08 == 0 {
            self.env_hold = true;
            self.env_alternate = self.env_attack != 0;
        } else {
            self.env_hold = shape & 0x01 != 0;
            self.env_alternate = shape & 0x02 != 0;
        }
        self.env_step = 15;
        self.env_holding = false;
        self.env_counter = 0;
        self.env_prescaler = false;
        self.env_volume = self.env_step as u8 ^ self.env_attack;
    }

    /// Un pas d'enveloppe. La rampe est toujours décomptée de 15 à 0 ; c'est
    /// le masque `env_attack` qui la retourne pour obtenir une montée.
    fn advance_envelope(&mut self) {
        if self.env_holding {
            return;
        }
        self.env_step -= 1;
        if self.env_step < 0 {
            if self.env_hold {
                if self.env_alternate {
                    self.env_attack ^= 0x0F;
                }
                self.env_holding = true;
                self.env_step = 0;
            } else {
                if self.env_alternate {
                    self.env_attack ^= 0x0F;
                }
                self.env_step = 15;
            }
        }
        self.env_volume = self.env_step as u8 ^ self.env_attack;
    }

    /// Niveau instantané du mélangeur, dans [0, 1].
    ///
    /// Dans R7, un bit à 1 *coupe* la source correspondante : le ton coupé se
    /// comporte comme un niveau haut permanent, ce qui laisse passer le bruit
    /// seul (et inversement).
    fn mix(&self, p: &Params) -> f32 {
        let noise_bit = (self.lfsr & 1) != 0;
        let mut sum = 0.0;
        for channel in 0..3 {
            let tone_on = self.tone_state[channel] || (p.mixer >> channel) & 1 != 0;
            let noise_on = noise_bit || (p.mixer >> (channel + 3)) & 1 != 0;
            if tone_on && noise_on {
                let amplitude = p.amplitude[channel];
                let level = if amplitude & 0x10 != 0 {
                    self.env_volume
                } else {
                    amplitude & 0x0F
                };
                sum += VOLUME_TABLE[level as usize];
            }
        }
        sum / 3.0
    }

    fn push_sample(&mut self, sample: f32) {
        if self.samples.len() >= MAX_BUFFERED_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Retire et renvoie tous les échantillons produits depuis le dernier
    /// appel.
    pub fn take_samples(&mut self) -> Vec<f32> {
        self.samples.drain(..).collect()
    }

    pub fn buffered_samples(&self) -> usize {
        self.samples.len()
    }

    /// Volume courant de l'enveloppe (0-15), pour l'affichage d'état.
    pub fn envelope_volume(&self) -> u8 {
        self.env_volume
    }

    /// Sortie carrée courante de chaque canal.
    #[cfg(test)]
    pub fn tone_states(&self) -> [bool; 3] {
        self.tone_state
    }

    /// Registre à décalage du bruit.
    #[cfg(test)]
    pub fn noise_lfsr(&self) -> u32 {
        self.lfsr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registres muets : tout est coupé dans le mélangeur et les amplitudes
    /// sont nulles.
    fn silent_registers() -> [u8; 16] {
        let mut regs = [0u8; 16];
        regs[7] = 0x3F;
        regs
    }

    /// Un seul canal de ton actif (canal A), au volume maximum.
    fn tone_a_registers(period: u16, volume: u8) -> [u8; 16] {
        let mut regs = silent_registers();
        regs[0] = period as u8;
        regs[1] = (period >> 8) as u8 & 0x0F;
        regs[7] = 0x3E; // bit 0 à 0 : ton A activé, tout le reste coupé
        regs[8] = volume;
        regs
    }

    /// Compte les changements d'état de la sortie carrée du canal A sur une
    /// durée donnée, en cycles PSG.
    fn count_tone_edges(sound: &mut Sound, regs: &[u8; 16], cycles: u32) -> u32 {
        let mut edges = 0;
        let mut previous = sound.tone_states()[0];
        for _ in 0..cycles {
            sound.run(regs, 1);
            let current = sound.tone_states()[0];
            if current != previous {
                edges += 1;
                previous = current;
            }
        }
        edges
    }

    /// Relève la suite des volumes d'enveloppe, un relevé par pas.
    fn envelope_sequence(shape: u8, steps: usize) -> Vec<u8> {
        let mut sound = Sound::new();
        sound.write_envelope_shape(shape);
        let mut out = vec![sound.envelope_volume()];
        for _ in 1..steps {
            sound.advance_envelope();
            out.push(sound.envelope_volume());
        }
        out
    }

    // --- Conversion de temps -------------------------------------------------

    #[test]
    fn cpu_ticks_are_divided_by_four_without_losing_the_remainder() {
        let mut sound = Sound::new();
        let regs = silent_registers();

        // Quatre appels d'un seul cycle CPU doivent produire exactement un
        // cycle PSG : le reste doit survivre d'un appel à l'autre.
        for _ in 0..3 {
            sound.tick_cpu(&regs, 1);
            assert_eq!(
                sound.prescaler, 0,
                "aucun cycle PSG ne doit encore etre passe"
            );
        }
        sound.tick_cpu(&regs, 1);
        assert_eq!(sound.prescaler, 1, "le 4e cycle CPU vaut un cycle PSG");
    }

    #[test]
    fn one_second_of_cpu_time_produces_one_second_of_samples() {
        let mut sound = Sound::new();
        let regs = silent_registers();

        // 4 MHz de cycles CPU, découpés comme le ferait l'émulateur.
        for _ in 0..1_000_000 {
            sound.tick_cpu(&regs, 4);
        }

        let produced = sound.buffered_samples();
        assert!(
            produced.abs_diff(SAMPLE_RATE as usize) <= 1,
            "{produced} echantillons produits en une seconde au lieu de {SAMPLE_RATE}"
        );
    }

    #[test]
    fn taking_samples_empties_the_buffer() {
        let mut sound = Sound::new();
        let regs = silent_registers();
        sound.run(&regs, PSG_CLOCK / 100);

        let first = sound.take_samples();
        assert!(!first.is_empty());
        assert_eq!(sound.buffered_samples(), 0);
        assert!(sound.take_samples().is_empty());
    }

    #[test]
    fn the_buffer_never_grows_past_one_second() {
        let mut sound = Sound::new();
        let regs = silent_registers();
        sound.run(&regs, 3 * PSG_CLOCK);
        assert_eq!(sound.buffered_samples(), MAX_BUFFERED_SAMPLES);
    }

    // --- Générateurs de tons -------------------------------------------------

    #[test]
    fn tone_frequency_matches_the_datasheet_formula() {
        // f = horloge / (16 * période). Une période de 284 donne un la 220 Hz,
        // soit 440 fronts par seconde.
        for period in [1u16, 100, 284, 4095] {
            let mut sound = Sound::new();
            let regs = tone_a_registers(period, 15);
            let edges = count_tone_edges(&mut sound, &regs, PSG_CLOCK);
            let expected = 2 * PSG_CLOCK / (16 * period as u32);
            assert!(
                edges.abs_diff(expected) <= 1,
                "periode {period} : {edges} fronts au lieu de {expected}"
            );
        }
    }

    #[test]
    fn the_period_uses_the_twelve_bits_of_both_registers() {
        let mut sound = Sound::new();
        let mut regs = tone_a_registers(0, 15);
        regs[0] = 0x34;
        regs[1] = 0x02; // période = 0x234 = 564
        let edges = count_tone_edges(&mut sound, &regs, PSG_CLOCK);
        let expected = 2 * PSG_CLOCK / (16 * 0x234);
        assert!(
            edges.abs_diff(expected) <= 1,
            "{edges} fronts au lieu de {expected}"
        );
    }

    #[test]
    fn a_zero_period_behaves_like_a_period_of_one() {
        let mut zero = Sound::new();
        let mut one = Sound::new();
        let edges_zero = count_tone_edges(&mut zero, &tone_a_registers(0, 15), 10_000);
        let edges_one = count_tone_edges(&mut one, &tone_a_registers(1, 15), 10_000);
        assert_eq!(edges_zero, edges_one);
        assert!(edges_zero > 0);
    }

    #[test]
    fn a_square_wave_spends_half_its_time_high() {
        let mut sound = Sound::new();
        let regs = tone_a_registers(100, 15);
        sound.run(&regs, PSG_CLOCK);
        let samples = sound.take_samples();

        let mean: f32 = samples.iter().sum::<f32>() / samples.len() as f32;
        // Un seul canal sur trois, au volume maximum (1.0), rapport cyclique 1/2.
        let expected = 1.0 / 3.0 / 2.0;
        assert!(
            (mean - expected).abs() < 0.005,
            "moyenne {mean} au lieu de {expected}"
        );
    }

    #[test]
    fn the_three_channels_are_mixed_together() {
        let mut regs = silent_registers();
        regs[7] = 0x38; // les trois tons actifs, bruit coupé partout
        for (channel, period) in [100u16, 150, 200].into_iter().enumerate() {
            regs[channel * 2] = period as u8;
            regs[channel * 2 + 1] = (period >> 8) as u8;
            regs[8 + channel] = 15;
        }

        let mut sound = Sound::new();
        sound.run(&regs, PSG_CLOCK);
        let samples = sound.take_samples();

        let max = samples.iter().cloned().fold(f32::MIN, f32::max);
        let mean: f32 = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!(
            max > 0.99,
            "les trois canaux au maximum doivent saturer a 1.0, vu {max}"
        );
        assert!(
            (mean - 0.5).abs() < 0.01,
            "trois carres a rapport cyclique 1/2 : moyenne {mean} au lieu de 0.5"
        );
    }

    // --- Mélangeur -----------------------------------------------------------

    #[test]
    fn everything_disabled_gives_a_constant_level() {
        let mut sound = Sound::new();
        // Tout coupé dans R7 mais les amplitudes au maximum : sur le vrai
        // composant, une source coupée équivaut à un niveau haut permanent,
        // donc la sortie est une tension continue. Elle ne s'entend pas (le
        // filtre coupe-continu de la sortie audio l'élimine), mais elle ne
        // doit surtout pas varier : la moindre variation ici serait un
        // mélangeur qui laisse passer un ton ou du bruit qu'on a coupé.
        let mut regs = silent_registers();
        regs[0] = 100; // périodes bien vivantes malgré tout
        regs[6] = 1;
        regs[8] = 15;
        regs[9] = 15;
        regs[10] = 15;

        sound.run(&regs, PSG_CLOCK / 10);
        assert!(sound.take_samples().iter().all(|&s| s == 1.0));
    }

    #[test]
    fn a_zero_amplitude_is_silent_even_with_the_tone_enabled() {
        let mut sound = Sound::new();
        let regs = tone_a_registers(100, 0);
        sound.run(&regs, PSG_CLOCK / 10);
        assert!(sound.take_samples().iter().all(|&s| s == 0.0));
    }

    #[test]
    fn a_disabled_tone_stays_high_and_lets_the_noise_through() {
        // Ton coupé, bruit actif sur le canal A : la sortie doit varier au
        // rythme du bruit, et non rester figée.
        let mut regs = silent_registers();
        regs[6] = 1;
        regs[7] = 0x37; // bit 3 à 0 : bruit A actif ; bit 0 à 1 : ton A coupé
        regs[8] = 15;

        let mut sound = Sound::new();
        sound.run(&regs, PSG_CLOCK / 10);
        let samples = sound.take_samples();
        assert!(
            samples.iter().any(|&s| s > 0.0) && samples.iter().any(|&s| s < 0.3),
            "le bruit doit moduler la sortie"
        );
    }

    #[test]
    fn a_tone_alone_is_never_gated_by_the_noise() {
        // Bruit coupé sur le canal : la sortie ne doit dépendre que du carré,
        // donc atteindre exactement le niveau plein sur les demi-périodes.
        let mut sound = Sound::new();
        let regs = tone_a_registers(400, 15);
        sound.run(&regs, PSG_CLOCK / 10);
        let samples = sound.take_samples();
        let max = samples.iter().cloned().fold(f32::MIN, f32::max);
        assert!((max - 1.0 / 3.0).abs() < 0.001, "niveau haut {max}");
    }

    #[test]
    fn amplitude_bit_four_switches_the_channel_to_the_envelope() {
        let mut regs = tone_a_registers(400, 0);
        regs[8] = 0x10; // amplitude pilotée par l'enveloppe
        regs[11] = 0xFF; // enveloppe lente : un pas tous les 16 * 255 cycles
        regs[12] = 0x00;

        let mut sound = Sound::new();
        sound.write_envelope_shape(0x09); // \___ : une descente puis silence

        sound.run(&regs, 20_000);
        let loud = sound
            .take_samples()
            .iter()
            .cloned()
            .fold(f32::MIN, f32::max);
        assert!(loud > 0.3, "l'enveloppe demarre au volume maximum ({loud})");

        // Une fois la rampe terminée (16 pas), le canal doit être muet, alors
        // même que R8 n'a pas changé.
        sound.run(&regs, 16 * 16 * 255);
        sound.take_samples();
        sound.run(&regs, 20_000);
        assert!(
            sound.take_samples().iter().all(|&s| s == 0.0),
            "l'enveloppe terminee doit couper le canal"
        );
    }

    // --- Générateur de bruit -------------------------------------------------

    #[test]
    fn the_noise_shift_register_runs_at_half_the_counter_rate() {
        let mut regs = silent_registers();
        regs[6] = 5; // période de bruit

        let mut sound = Sound::new();
        let start = sound.noise_lfsr();

        // Une expiration de compteur sur deux décale le registre : il faut
        // 2 * période cycles de prédiviseur pour un décalage.
        sound.run(&regs, PRESCALER * 5 * 2 - 1);
        assert_eq!(sound.noise_lfsr(), start, "decalage trop tot");

        sound.run(&regs, 1);
        assert_ne!(sound.noise_lfsr(), start, "decalage attendu");
    }

    #[test]
    fn a_zero_noise_period_behaves_like_a_period_of_one() {
        let mut zero = silent_registers();
        zero[6] = 0;
        let mut one = silent_registers();
        one[6] = 1;

        let mut a = Sound::new();
        let mut b = Sound::new();
        a.run(&zero, 10_000);
        b.run(&one, 10_000);
        assert_eq!(a.noise_lfsr(), b.noise_lfsr());
    }

    #[test]
    fn the_noise_sequence_is_a_maximal_length_17_bit_register() {
        let mut sound = Sound::new();
        let start = sound.noise_lfsr();
        let mut period = 0u32;
        for i in 1..=(1u32 << 17) {
            sound.step_noise();
            if sound.noise_lfsr() == start {
                period = i;
                break;
            }
        }
        assert_eq!(period, (1 << 17) - 1, "sequence de 2^17 - 1 etats attendue");
    }

    // --- Générateur d'enveloppe ---------------------------------------------

    #[test]
    fn envelope_shapes_follow_the_datasheet() {
        // Les formes 0-3 sont toutes équivalentes à 0x09 (\___) et les formes
        // 4-7 à 0x0F (/___) : sans CONT, une seule rampe est jouée.
        for shape in 0x00..=0x03 {
            assert_eq!(
                envelope_sequence(shape, 20),
                envelope_sequence(0x09, 20),
                "forme {shape:#04X} : une seule rampe descendante attendue"
            );
        }
        for shape in 0x04..=0x07 {
            assert_eq!(
                envelope_sequence(shape, 20),
                envelope_sequence(0x0F, 20),
                "forme {shape:#04X} : une seule rampe montante attendue"
            );
        }

        let down: Vec<u8> = (0..16).rev().collect();
        let up: Vec<u8> = (0..16).collect();

        // 0x09 : \___ descente puis silence
        assert_eq!(
            envelope_sequence(0x09, 20),
            [&down[..], &[0; 4][..]].concat()
        );
        // 0x0F : /___ montée puis silence
        assert_eq!(envelope_sequence(0x0F, 20), [&up[..], &[0; 4][..]].concat());
        // 0x08 : \\\\ dents de scie descendantes
        assert_eq!(envelope_sequence(0x08, 32), [&down[..], &down[..]].concat());
        // 0x0C : //// dents de scie montantes
        assert_eq!(envelope_sequence(0x0C, 32), [&up[..], &up[..]].concat());
        // 0x0A : \/\/ triangle démarrant en descente
        assert_eq!(envelope_sequence(0x0A, 32), [&down[..], &up[..]].concat());
        // 0x0E : /\/\ triangle démarrant en montée
        assert_eq!(envelope_sequence(0x0E, 32), [&up[..], &down[..]].concat());
        // 0x0B : \‾‾‾ descente puis maintien au maximum
        assert_eq!(
            envelope_sequence(0x0B, 20),
            [&down[..], &[15; 4][..]].concat()
        );
        // 0x0D : /‾‾‾ montée puis maintien au maximum
        assert_eq!(
            envelope_sequence(0x0D, 20),
            [&up[..], &[15; 4][..]].concat()
        );
    }

    #[test]
    fn envelope_period_matches_the_datasheet_formula() {
        // Un cycle complet de 16 pas dure 256 * période cycles PSG.
        let period: u32 = 0x0100;
        let mut regs = silent_registers();
        regs[11] = period as u8;
        regs[12] = (period >> 8) as u8;

        let mut sound = Sound::new();
        sound.write_envelope_shape(0x08); // rampe descendante répétée

        // Après un pas, le volume doit être passé de 15 à 14.
        sound.run(&regs, 16 * period);
        assert_eq!(sound.envelope_volume(), 14);

        // Après un cycle complet, la rampe est revenue à son départ.
        sound.run(&regs, 15 * 16 * period);
        assert_eq!(sound.envelope_volume(), 15);
    }

    #[test]
    fn a_zero_envelope_period_behaves_like_a_period_of_one() {
        let mut zero = silent_registers();
        zero[11] = 0;
        zero[12] = 0;
        let mut one = silent_registers();
        one[11] = 1;
        one[12] = 0;

        let mut a = Sound::new();
        let mut b = Sound::new();
        a.write_envelope_shape(0x08);
        b.write_envelope_shape(0x08);
        a.run(&zero, 1000);
        b.run(&one, 1000);
        assert_eq!(a.envelope_volume(), b.envelope_volume());
    }

    #[test]
    fn writing_the_shape_restarts_the_envelope() {
        let mut regs = silent_registers();
        regs[11] = 0x10;

        let mut sound = Sound::new();
        sound.write_envelope_shape(0x08);
        sound.run(&regs, 16 * 0x10 * 5);
        assert_eq!(sound.envelope_volume(), 10);

        // Réécrire la même forme relance la rampe depuis le début : c'est ce
        // qui permet aux musiques de redéclencher une enveloppe à chaque note.
        sound.write_envelope_shape(0x08);
        assert_eq!(sound.envelope_volume(), 15);
    }

    #[test]
    fn a_held_envelope_stops_counting() {
        let mut regs = silent_registers();
        regs[11] = 0x01;

        let mut sound = Sound::new();
        sound.write_envelope_shape(0x0B); // \‾‾‾ : maintien au maximum
        sound.run(&regs, 100_000);
        assert_eq!(sound.envelope_volume(), 15);
        sound.run(&regs, 100_000);
        assert_eq!(sound.envelope_volume(), 15);
    }

    // --- Table de volumes ----------------------------------------------------

    #[test]
    fn the_volume_table_is_silent_at_zero_full_at_fifteen_and_monotonic() {
        assert_eq!(VOLUME_TABLE[0], 0.0);
        assert_eq!(VOLUME_TABLE[15], 1.0);
        for pair in VOLUME_TABLE.windows(2) {
            assert!(pair[1] > pair[0], "table non monotone : {pair:?}");
        }
        // Échelle logarithmique : le pas 7 est loin de la moitié du maximum.
        assert!(VOLUME_TABLE[7] < 0.25, "l'echelle doit etre logarithmique");
    }
}
