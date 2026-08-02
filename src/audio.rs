//! Sortie audio hôte : convoie les échantillons produits par le PSG vers la
//! carte son via SDL2.
//!
//! Ce module ne contient rien de spécifique au CPC ; il ne s'occupe que du
//! passage du flux émulé vers le matériel réel, avec ce que cela suppose de
//! régulation de latence et de mise en forme du signal.

use crate::sound::SAMPLE_RATE;
use sdl2::audio::{AudioQueue, AudioSpecDesired};

/// Taille du bloc demandé à SDL. 512 échantillons à 44,1 kHz font environ
/// 11 ms, soit un bon compromis entre latence et risque de sous-alimentation.
const DEVICE_BUFFER: u16 = 512;

/// Latence maximale tolérée avant de jeter des échantillons. L'émulateur cale
/// ses trames sur une échéance temporelle, mais la moindre dérive entre son
/// horloge et celle de la carte son ferait sinon grossir la file sans fin,
/// jusqu'à un retard audible de plusieurs secondes.
const MAX_LATENCY_SAMPLES: u32 = SAMPLE_RATE / 10;

/// Coussin visé quand la file s'est vidée. L'émulateur ne l'alimente qu'une
/// fois par trame, par salves de 20 ms, alors que la carte son la vide en
/// continu : sans réserve d'avance, la moindre trame un peu longue laisse la
/// carte son à sec, ce qui s'entend comme un craquement.
const TARGET_LATENCY_SAMPLES: u32 = SAMPLE_RATE * 3 / 50; // 3 trames, 60 ms

/// Seuil en dessous duquel on reconstitue le coussin. Au-dessus, on ne touche
/// à rien : rembourrer à chaque trame ne ferait qu'ajouter de la latence.
const MIN_LATENCY_SAMPLES: u32 = SAMPLE_RATE / 50; // 1 trame, 20 ms

/// Décision de régulation pour une trame d'échantillons.
#[derive(Debug, PartialEq, Eq)]
enum Regulation {
    /// Trop de retard accumulé : la trame est jetée.
    Drop,
    /// Trame envoyée, précédée de `padding` échantillons de silence pour
    /// reconstituer le coussin.
    Send { padding: u32 },
}

/// Décide du sort d'une trame en fonction de ce qui reste à jouer.
fn regulate(queued: u32) -> Regulation {
    if queued > MAX_LATENCY_SAMPLES {
        Regulation::Drop
    } else if queued < MIN_LATENCY_SAMPLES {
        Regulation::Send {
            padding: TARGET_LATENCY_SAMPLES - queued,
        }
    } else {
        Regulation::Send { padding: 0 }
    }
}

/// Constante du filtre coupe-continu, pour un pôle vers 20 Hz à 44,1 kHz.
const DC_BLOCKER_R: f32 = 0.9972;

pub struct Audio {
    queue: AudioQueue<f32>,
    volume: f32,
    /// État du filtre coupe-continu (entrée et sortie précédentes).
    last_input: f32,
    last_output: f32,
    /// Tampon de conversion réutilisé d'une trame à l'autre.
    scratch: Vec<f32>,
}

impl Audio {
    /// Ouvre le périphérique audio. Une machine sans carte son utilisable ne
    /// doit pas empêcher l'émulateur de tourner : l'appelant est libre de
    /// continuer sans son en cas d'erreur.
    pub fn new(sdl: &sdl2::Sdl) -> Result<Self, String> {
        let subsystem = sdl.audio()?;
        let desired = AudioSpecDesired {
            freq: Some(SAMPLE_RATE as i32),
            channels: Some(1),
            samples: Some(DEVICE_BUFFER),
        };
        let queue: AudioQueue<f32> = subsystem.open_queue(None, &desired)?;
        queue.resume();

        Ok(Self {
            queue,
            volume: 0.5,
            last_input: 0.0,
            last_output: 0.0,
            scratch: Vec::new(),
        })
    }

    /// Nombre d'échantillons déjà en attente de lecture par la carte son.
    pub fn queued_samples(&self) -> u32 {
        self.queue.size() / std::mem::size_of::<f32>() as u32
    }

    /// Envoie une trame d'échantillons du PSG vers la carte son.
    pub fn push(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        let padding = match regulate(self.queued_samples()) {
            // Trop de retard accumulé : on saute cette trame plutôt que de
            // laisser le son dériver durablement derrière l'image. Le filtre
            // est tout de même avancé pour éviter une discontinuité au retour
            // à la normale.
            Regulation::Drop => {
                self.last_input = *samples.last().unwrap();
                return;
            }
            Regulation::Send { padding } => padding,
        };

        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.reserve(padding as usize + samples.len());
        scratch.resize(padding as usize, 0.0);
        for &sample in samples {
            let filtered = self.filter(sample);
            scratch.push(filtered);
        }
        let _ = self.queue.queue_audio(&scratch);
        self.scratch = scratch;
    }

    /// Volume de sortie, dans [0, 1].
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// Filtre coupe-continu et mise au volume. La sortie du PSG est un signal
    /// positif : sans retrait de la composante continue, chaque note gagnerait
    /// et perdrait un décalage brutal, entendu comme un claquement.
    fn filter(&mut self, sample: f32) -> f32 {
        let output = sample - self.last_input + DC_BLOCKER_R * self.last_output;
        self.last_input = sample;
        self.last_output = output;
        output * self.volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nombre d'échantillons que l'émulateur produit pour une trame standard.
    const FRAME: u32 = SAMPLE_RATE / 50;

    #[test]
    fn an_empty_queue_is_refilled_with_a_cushion() {
        // Au démarrage, et après tout sous-alimentation, il faut reconstituer
        // une réserve d'avance : sans elle la carte son retombe à sec dès la
        // trame suivante.
        assert_eq!(
            regulate(0),
            Regulation::Send {
                padding: TARGET_LATENCY_SAMPLES
            }
        );
        assert!(
            TARGET_LATENCY_SAMPLES > FRAME,
            "le coussin doit depasser une trame"
        );
    }

    #[test]
    fn a_healthy_queue_is_left_alone() {
        // Le régime normal ne doit rien ajouter, sinon la latence grimperait
        // d'une trame à l'autre jusqu'au décrochage entre le son et l'image.
        for queued in [MIN_LATENCY_SAMPLES, 2 * FRAME, MAX_LATENCY_SAMPLES] {
            assert_eq!(regulate(queued), Regulation::Send { padding: 0 });
        }
    }

    #[test]
    fn an_overfull_queue_drops_a_frame() {
        assert_eq!(regulate(MAX_LATENCY_SAMPLES + 1), Regulation::Drop);
    }

    /// La régulation doit converger : en régime stable, elle ne doit ni
    /// rembourrer ni jeter en boucle.
    #[test]
    fn the_regulation_settles_at_a_stable_latency() {
        let mut queued = 0u32;
        let mut refills = 0;
        for _ in 0..1000 {
            match regulate(queued) {
                Regulation::Drop => queued -= FRAME,
                Regulation::Send { padding } => {
                    if padding > 0 {
                        refills += 1;
                    }
                    queued += padding + FRAME;
                }
            }
            // La carte son consomme une trame par trame émulée.
            queued = queued.saturating_sub(FRAME);
        }
        assert_eq!(refills, 1, "un seul remplissage, au demarrage");
        assert!(
            (MIN_LATENCY_SAMPLES..=MAX_LATENCY_SAMPLES).contains(&queued),
            "latence stabilisee hors de la plage visee : {queued}"
        );
    }

    /// Le filtre est testé seul : ouvrir un vrai périphérique SDL n'a rien à
    /// faire dans une suite de tests.
    struct DcBlocker {
        last_input: f32,
        last_output: f32,
    }

    impl DcBlocker {
        fn new() -> Self {
            Self {
                last_input: 0.0,
                last_output: 0.0,
            }
        }
        fn filter(&mut self, sample: f32) -> f32 {
            let output = sample - self.last_input + DC_BLOCKER_R * self.last_output;
            self.last_input = sample;
            self.last_output = output;
            output
        }
    }

    #[test]
    fn a_constant_input_decays_to_zero() {
        let mut f = DcBlocker::new();
        let mut last = 0.0;
        for _ in 0..SAMPLE_RATE {
            last = f.filter(0.5);
        }
        assert!(last.abs() < 1e-3, "composante continue restante : {last}");
    }

    #[test]
    fn silence_stays_exactly_silent() {
        let mut f = DcBlocker::new();
        for _ in 0..1000 {
            assert_eq!(f.filter(0.0), 0.0);
        }
    }

    #[test]
    fn a_square_wave_keeps_its_peak_to_peak_amplitude() {
        // Un carré à 1 kHz : le filtre ne doit retirer que le décalage, pas
        // écraser le signal utile.
        let mut f = DcBlocker::new();
        let half = SAMPLE_RATE as usize / 2000;
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for i in 0..SAMPLE_RATE as usize {
            let input = if (i / half) % 2 == 0 { 1.0 } else { 0.0 };
            let out = f.filter(input);
            // On ignore le régime transitoire du début.
            if i > SAMPLE_RATE as usize / 2 {
                min = min.min(out);
                max = max.max(out);
            }
        }
        // Le léger dépassement de 1 est le fléchissement du filtre pendant
        // chaque demi-période, inévitable pour un coupe-continu du premier
        // ordre ; il reste très en deçà de ce qui s'entendrait.
        assert!(
            (0.95..1.05).contains(&(max - min)),
            "amplitude crete a crete {} attendue proche de 1",
            max - min
        );
        assert!(max > 0.0 && min < 0.0, "le signal doit etre recentre");
    }
}
