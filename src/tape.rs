use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;

/// Nombre de cycles Z80 (T-states) par milliseconde, à l'horloge 4 MHz du
/// CPC. Les longueurs de pause du format CDT sont données en millisecondes ;
/// les longueurs de pulse (pilote, sync, bits) sont, elles, déjà exprimées
/// en T-states dans le fichier — convention adoptée par la communauté CPC
/// pour ce format (dérivé du TZX, pensé à l'origine pour le Spectrum à
/// 3,5 MHz), et reprise telle quelle plutôt que remise à l'échelle. À
/// vérifier empiriquement avec une cassette connue pour fonctionner sur un
/// vrai CPC si un chargement échoue de façon systématique.
const TICKS_PER_MS: u32 = 4_000;

/// Un bloc du fichier CDT, tel que nécessaire pour la relecture (lecture
/// seule : pas de bloc d'enregistrement/SAVE). Les blocs de métadonnées pures
/// (texte, groupes, info d'archive...) et les blocs reconnus mais non
/// interprétés (CSW, Direct Recording, Jump/Loop...) sont réduits à
/// `Block::Meta` : leur contenu est ignoré, mais leur longueur est
/// correctement décodée pour ne jamais désynchroniser le flux de blocs
/// suivants.
#[derive(Debug, Clone)]
enum Block {
    /// ID 0x10 : Standard Speed Data Block. Timing fixe (celui du chargeur
    /// ROM standard), longueur de pilote déduite du premier octet de
    /// donnée (< 0x80 : bloc d'en-tête, 8063 pulses ; sinon bloc de
    /// données, 3223 pulses).
    StandardSpeedData { pause_ms: u16, data: Vec<u8> },
    /// ID 0x11 : Turbo Speed Data Block. Timing entièrement paramétrable —
    /// c'est le bloc qu'utilisent les protections de copie non standard.
    TurboSpeedData {
        pilot_pulse: u16,
        sync1: u16,
        sync2: u16,
        zero_bit_pulse: u16,
        one_bit_pulse: u16,
        pilot_tone_length: u16,
        used_bits_last_byte: u8,
        pause_ms: u16,
        data: Vec<u8>,
    },
    /// ID 0x12 : Pure Tone (une tonalité seule, sans données).
    PureTone { pulse_length: u16, num_pulses: u16 },
    /// ID 0x13 : Pulse Sequence (suite de pulses de longueurs arbitraires).
    PulseSequence { pulses: Vec<u16> },
    /// ID 0x14 : Pure Data Block (données sans pilote ni sync).
    PureData {
        zero_bit_pulse: u16,
        one_bit_pulse: u16,
        used_bits_last_byte: u8,
        pause_ms: u16,
        data: Vec<u8>,
    },
    /// ID 0x20 : Pause/Stop the Tape. Une pause de 0 ms est, par convention
    /// du format, un arrêt définitif de la bande (utilisé entre les
    /// parties d'un chargeur multi-étapes) plutôt qu'une pause nulle.
    Pause { pause_ms: u16 },
    /// Bloc reconnu (métadonnée ou bloc non interprété en lecture) : sans
    /// effet sur le signal, sauté proprement.
    Meta,
}

/// Un fichier CDT entièrement décodé en une suite de blocs.
struct CdtImage {
    blocks: Vec<Block>,
}

fn read_u8(data: &[u8], offset: usize) -> Result<u8, String> {
    data.get(offset)
        .copied()
        .ok_or_else(|| format!("CDT tronqué à l'octet {offset}"))
}

fn read_u16le(data: &[u8], offset: usize) -> Result<u16, String> {
    let lo = read_u8(data, offset)? as u16;
    let hi = read_u8(data, offset + 1)? as u16;
    Ok(lo | (hi << 8))
}

fn read_u24le(data: &[u8], offset: usize) -> Result<u32, String> {
    let lo = read_u8(data, offset)? as u32;
    let mid = read_u8(data, offset + 1)? as u32;
    let hi = read_u8(data, offset + 2)? as u32;
    Ok(lo | (mid << 8) | (hi << 16))
}

fn read_u32le(data: &[u8], offset: usize) -> Result<u32, String> {
    let lo = read_u16le(data, offset)? as u32;
    let hi = read_u16le(data, offset + 2)? as u32;
    Ok(lo | (hi << 16))
}

fn read_slice(data: &[u8], offset: usize, len: usize) -> Result<Vec<u8>, String> {
    data.get(offset..offset + len)
        .map(|s| s.to_vec())
        .ok_or_else(|| format!("CDT tronqué : {len} octets attendus à l'offset {offset}"))
}

impl CdtImage {
    /// Décode un fichier .cdt entier. Comme `DskImage::parse`, ne panique
    /// jamais sur un fichier corrompu ou tronqué : toute lecture hors
    /// bornes renvoie une `Err` explicite.
    fn parse(data: &[u8]) -> Result<Self, String> {
        const SIGNATURE: &[u8] = b"ZXTape!\x1A";
        if data.len() < SIGNATURE.len() + 2 || &data[..SIGNATURE.len()] != SIGNATURE {
            return Err("Signature CDT absente ou invalide (attendu \"ZXTape!\")".to_string());
        }
        // data[7] = 0x1A, data[8]/data[9] = version majeure/mineure : non
        // utilisées, la structure des blocs ne dépend pas de la version.
        let mut pos = SIGNATURE.len() + 2;
        let mut blocks = Vec::new();

        while pos < data.len() {
            let id = read_u8(data, pos)?;
            pos += 1;
            let (block, consumed) = Self::parse_block(id, data, pos)?;
            blocks.push(block);
            pos += consumed;
        }

        Ok(CdtImage { blocks })
    }

    /// Décode un bloc à partir de son ID (déjà lu) et de son offset de
    /// contenu. Renvoie le bloc et le nombre d'octets consommés après l'ID.
    fn parse_block(id: u8, data: &[u8], pos: usize) -> Result<(Block, usize), String> {
        match id {
            0x10 => {
                let pause_ms = read_u16le(data, pos)?;
                let length = read_u16le(data, pos + 2)? as usize;
                let payload = read_slice(data, pos + 4, length)?;
                Ok((
                    Block::StandardSpeedData {
                        pause_ms,
                        data: payload,
                    },
                    4 + length,
                ))
            }
            0x11 => {
                let pilot_pulse = read_u16le(data, pos)?;
                let sync1 = read_u16le(data, pos + 2)?;
                let sync2 = read_u16le(data, pos + 4)?;
                let zero_bit_pulse = read_u16le(data, pos + 6)?;
                let one_bit_pulse = read_u16le(data, pos + 8)?;
                let pilot_tone_length = read_u16le(data, pos + 10)?;
                let used_bits_last_byte = read_u8(data, pos + 12)?;
                let pause_ms = read_u16le(data, pos + 13)?;
                let length = read_u24le(data, pos + 15)? as usize;
                let payload = read_slice(data, pos + 18, length)?;
                Ok((
                    Block::TurboSpeedData {
                        pilot_pulse,
                        sync1,
                        sync2,
                        zero_bit_pulse,
                        one_bit_pulse,
                        pilot_tone_length,
                        used_bits_last_byte,
                        pause_ms,
                        data: payload,
                    },
                    18 + length,
                ))
            }
            0x12 => {
                let pulse_length = read_u16le(data, pos)?;
                let num_pulses = read_u16le(data, pos + 2)?;
                Ok((
                    Block::PureTone {
                        pulse_length,
                        num_pulses,
                    },
                    4,
                ))
            }
            0x13 => {
                let count = read_u8(data, pos)? as usize;
                let mut pulses = Vec::with_capacity(count);
                for i in 0..count {
                    pulses.push(read_u16le(data, pos + 1 + i * 2)?);
                }
                Ok((Block::PulseSequence { pulses }, 1 + count * 2))
            }
            0x14 => {
                let zero_bit_pulse = read_u16le(data, pos)?;
                let one_bit_pulse = read_u16le(data, pos + 2)?;
                let used_bits_last_byte = read_u8(data, pos + 4)?;
                let pause_ms = read_u16le(data, pos + 5)?;
                let length = read_u24le(data, pos + 7)? as usize;
                let payload = read_slice(data, pos + 10, length)?;
                Ok((
                    Block::PureData {
                        zero_bit_pulse,
                        one_bit_pulse,
                        used_bits_last_byte,
                        pause_ms,
                        data: payload,
                    },
                    10 + length,
                ))
            }
            0x15 => {
                // Direct Recording : non interprété en lecture (échantillons
                // bruts), mais sauté correctement.
                let length = read_u24le(data, pos + 5)? as usize;
                Ok((Block::Meta, 8 + length))
            }
            0x18 | 0x19 => {
                // CSW Recording / Generalized Data Block : longueur totale du
                // reste du bloc sur 4 octets, non interprétée.
                let length = read_u32le(data, pos)? as usize;
                Ok((Block::Meta, 4 + length))
            }
            0x20 => {
                let pause_ms = read_u16le(data, pos)?;
                Ok((Block::Pause { pause_ms }, 2))
            }
            0x21 => {
                let length = read_u8(data, pos)? as usize;
                Ok((Block::Meta, 1 + length))
            }
            0x22 | 0x27 => Ok((Block::Meta, 0)),
            0x23 => Ok((Block::Meta, 2)),
            0x24 => Ok((Block::Meta, 2)),
            0x25 => Ok((Block::Meta, 0)),
            0x26 => {
                let count = read_u16le(data, pos)? as usize;
                Ok((Block::Meta, 2 + count * 2))
            }
            0x28 => {
                let length = read_u16le(data, pos)? as usize;
                Ok((Block::Meta, 2 + length))
            }
            0x2A => {
                let length = read_u32le(data, pos)? as usize;
                Ok((Block::Meta, 4 + length))
            }
            0x2B => {
                let length = read_u32le(data, pos)? as usize;
                Ok((Block::Meta, 4 + length))
            }
            0x30 => {
                let length = read_u8(data, pos)? as usize;
                Ok((Block::Meta, 1 + length))
            }
            0x32 => {
                let length = read_u16le(data, pos)? as usize;
                Ok((Block::Meta, 2 + length))
            }
            0x33 => {
                let count = read_u8(data, pos)? as usize;
                Ok((Block::Meta, 1 + count * 3))
            }
            0x35 => {
                let length = read_u32le(data, pos + 10)? as usize;
                Ok((Block::Meta, 14 + length))
            }
            0x5A => Ok((Block::Meta, 9)),
            other => Err(format!(
                "Bloc CDT inconnu (ID {other:#04X} à l'offset {pos}) : \
                 lecture arrêtée pour ne pas désynchroniser le flux"
            )),
        }
    }
}

/// Un évènement dans la file de restitution du signal cassette : soit un
/// front (le niveau bascule après la durée donnée), soit un maintien (le
/// niveau reste tel quel — utilisé pour les pauses entre blocs).
#[derive(Clone, Copy)]
enum PulseEvent {
    Toggle(u32),
    Hold(u32),
}

/// État d'exécution du lecteur de cassettes : position de lecture dans la
/// bande chargée, et signal actuellement présenté au PPI (bit 6 du port B).
///
/// Émulation par impulsions : contrairement à un piégeage des routines
/// firmware, le signal est reconstruit au niveau du timing exact des
/// impulsions du fichier CDT, et c'est le firmware du CPC (routines de
/// lecture cassette de la ROM basse) qui le décode normalement, comme sur
/// une vraie cassette. Conséquence assumée : le chargement prend le temps
/// réel de la bande.
pub struct Tape {
    pending_blocks: VecDeque<Block>,
    pulses: VecDeque<PulseEvent>,
    pulse_countdown: u32,
    current_level: bool,
    /// Vrai une fois la bande arrivée à sa fin, ou sur un bloc "Stop the
    /// Tape" (pause de 0 ms) : le signal reste alors figé jusqu'à un
    /// nouveau chargement.
    stopped: bool,
    /// Reflète le bit moteur du port C du PPI (bit 5) : la bande n'avance
    /// que si le firmware a mis le moteur en marche, exactement comme sur
    /// une vraie platine.
    pub motor_on: bool,
    pub current_filename: Option<String>,
}

impl Tape {
    pub fn new() -> Self {
        Self {
            pending_blocks: VecDeque::new(),
            pulses: VecDeque::new(),
            pulse_countdown: 0,
            current_level: false,
            stopped: true,
            motor_on: false,
            current_filename: None,
        }
    }

    /// Charge un fichier .cdt et remet la bande au début.
    pub fn load_tape(&mut self, filename: &str) -> Result<(), String> {
        let mut f = File::open(filename).map_err(|e| e.to_string())?;
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
        let image = CdtImage::parse(&buffer)?;

        self.pending_blocks = image.blocks.into();
        self.pulses.clear();
        self.pulse_countdown = 0;
        self.current_level = false;
        self.stopped = false;
        self.current_filename = Some(filename.to_string());
        println!("Tape CDT Loaded: {filename}");
        Ok(())
    }

    /// Éjecte la cassette.
    pub fn eject_tape(&mut self) {
        self.pending_blocks.clear();
        self.pulses.clear();
        self.pulse_countdown = 0;
        self.current_level = false;
        self.stopped = true;
        self.current_filename = None;
        println!("Tape Ejected");
    }

    /// Niveau actuel du signal cassette, tel que vu par le bit 6 du port B
    /// du PPI (lecture cassette).
    pub fn read_bit(&self) -> bool {
        self.current_level
    }

    /// Avance la lecture de la bande de `elapsed_ticks` T-states. Sans
    /// effet si le moteur est à l'arrêt ou si la bande est arrivée à sa
    /// fin — exactement comme une cassette qui n'avance pas quand la
    /// platine ne tourne pas.
    pub fn tick(&mut self, elapsed_ticks: u32) {
        if !self.motor_on || self.stopped {
            return;
        }
        let mut remaining = elapsed_ticks;
        while remaining > 0 {
            if self.pulse_countdown == 0 {
                match self.pulses.pop_front() {
                    Some(PulseEvent::Toggle(d)) => {
                        self.current_level = !self.current_level;
                        self.pulse_countdown = d.max(1);
                    }
                    Some(PulseEvent::Hold(d)) => {
                        self.pulse_countdown = d.max(1);
                    }
                    None => {
                        if !self.advance_block() {
                            return;
                        }
                        continue;
                    }
                }
            }
            let consume = remaining.min(self.pulse_countdown);
            self.pulse_countdown -= consume;
            remaining -= consume;
        }
    }

    /// Passe au bloc suivant (en sautant les blocs de métadonnées, qui ne
    /// produisent aucun pulse), et prépare sa file de pulses. Renvoie faux
    /// si la bande est arrivée à sa fin ou sur un bloc d'arrêt explicite.
    fn advance_block(&mut self) -> bool {
        loop {
            let Some(block) = self.pending_blocks.pop_front() else {
                self.stopped = true;
                return false;
            };
            match Self::build_pulses(&block) {
                None => {
                    self.stopped = true;
                    return false;
                }
                Some(queue) => {
                    if queue.is_empty() {
                        continue;
                    }
                    self.pulses = queue;
                    return true;
                }
            }
        }
    }

    /// Construit la file de pulses représentant un bloc. `None` signifie
    /// « arrêter la bande ici » (bloc Pause de 0 ms — "Stop the Tape").
    fn build_pulses(block: &Block) -> Option<VecDeque<PulseEvent>> {
        match block {
            Block::StandardSpeedData { pause_ms, data } => {
                // Timing standard du chargeur ROM. La longueur de pilote se
                // déduit du premier octet : un bloc d'en-tête (< 0x80) a un
                // pilote plus long qu'un bloc de données.
                let pilot_tone_length = match data.first() {
                    Some(flag) if *flag < 0x80 => 8063,
                    _ => 3223,
                };
                Some(Self::encode_data_block(
                    2168,
                    667,
                    735,
                    855,
                    1710,
                    pilot_tone_length,
                    8,
                    *pause_ms,
                    data,
                ))
            }
            Block::TurboSpeedData {
                pilot_pulse,
                sync1,
                sync2,
                zero_bit_pulse,
                one_bit_pulse,
                pilot_tone_length,
                used_bits_last_byte,
                pause_ms,
                data,
            } => Some(Self::encode_data_block(
                *pilot_pulse,
                *sync1,
                *sync2,
                *zero_bit_pulse,
                *one_bit_pulse,
                *pilot_tone_length,
                *used_bits_last_byte,
                *pause_ms,
                data,
            )),
            Block::PureTone {
                pulse_length,
                num_pulses,
            } => {
                let mut q = VecDeque::with_capacity(*num_pulses as usize);
                for _ in 0..*num_pulses {
                    q.push_back(PulseEvent::Toggle(*pulse_length as u32));
                }
                Some(q)
            }
            Block::PulseSequence { pulses } => Some(
                pulses
                    .iter()
                    .map(|p| PulseEvent::Toggle(*p as u32))
                    .collect(),
            ),
            Block::PureData {
                zero_bit_pulse,
                one_bit_pulse,
                used_bits_last_byte,
                pause_ms,
                data,
            } => {
                let mut q = VecDeque::new();
                Self::encode_data(
                    &mut q,
                    *zero_bit_pulse,
                    *one_bit_pulse,
                    *used_bits_last_byte,
                    data,
                );
                Self::encode_pause(&mut q, *pause_ms);
                Some(q)
            }
            Block::Pause { pause_ms } => {
                if *pause_ms == 0 {
                    None
                } else {
                    let mut q = VecDeque::new();
                    q.push_back(PulseEvent::Hold(*pause_ms as u32 * TICKS_PER_MS));
                    Some(q)
                }
            }
            Block::Meta => Some(VecDeque::new()),
        }
    }

    /// Construit la file de pulses commune aux blocs "avec pilote" (0x10 et
    /// 0x11) : tonalité pilote, deux pulses de synchronisation, puis les
    /// données bit à bit (deux pulses par bit, MSB en premier), suivies
    /// d'une pause.
    #[allow(clippy::too_many_arguments)]
    fn encode_data_block(
        pilot_pulse: u16,
        sync1: u16,
        sync2: u16,
        zero_bit_pulse: u16,
        one_bit_pulse: u16,
        pilot_tone_length: u16,
        used_bits_last_byte: u8,
        pause_ms: u16,
        data: &[u8],
    ) -> VecDeque<PulseEvent> {
        let mut q = VecDeque::new();
        for _ in 0..pilot_tone_length {
            q.push_back(PulseEvent::Toggle(pilot_pulse as u32));
        }
        q.push_back(PulseEvent::Toggle(sync1 as u32));
        q.push_back(PulseEvent::Toggle(sync2 as u32));
        Self::encode_data(
            &mut q,
            zero_bit_pulse,
            one_bit_pulse,
            used_bits_last_byte,
            data,
        );
        Self::encode_pause(&mut q, pause_ms);
        q
    }

    /// Encode les octets de données en pulses (deux pulses par bit, MSB en
    /// premier). Sur le dernier octet, seuls les `used_bits_last_byte` bits
    /// de poids fort sont émis — c'est ainsi que le format CDT représente
    /// un flux qui ne s'arrête pas nécessairement sur une frontière d'octet.
    fn encode_data(
        q: &mut VecDeque<PulseEvent>,
        zero_bit_pulse: u16,
        one_bit_pulse: u16,
        used_bits_last_byte: u8,
        data: &[u8],
    ) {
        let last_index = data.len().saturating_sub(1);
        for (i, byte) in data.iter().enumerate() {
            let bits = if i == last_index {
                used_bits_last_byte.clamp(1, 8)
            } else {
                8
            };
            for bit_pos in (8 - bits..8).rev() {
                let bit = (byte >> bit_pos) & 1;
                let pulse = if bit == 0 {
                    zero_bit_pulse
                } else {
                    one_bit_pulse
                } as u32;
                q.push_back(PulseEvent::Toggle(pulse));
                q.push_back(PulseEvent::Toggle(pulse));
            }
        }
    }

    fn encode_pause(q: &mut VecDeque<PulseEvent>, pause_ms: u16) {
        if pause_ms > 0 {
            q.push_back(PulseEvent::Hold(pause_ms as u32 * TICKS_PER_MS));
        }
    }
}

impl Default for Tape {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cdt_header() -> Vec<u8> {
        let mut v = b"ZXTape!\x1A".to_vec();
        v.push(1); // version majeure
        v.push(20); // version mineure
        v
    }

    /// Un bloc Standard Speed Data minimal, avec un octet de donnée choisi
    /// pour donner un pilote "bloc de données" (>= 0x80), plus facile à
    /// dénombrer dans le test.
    fn standard_block(data: &[u8], pause_ms: u16) -> Vec<u8> {
        let mut v = vec![0x10];
        v.extend_from_slice(&pause_ms.to_le_bytes());
        v.extend_from_slice(&(data.len() as u16).to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn a_truncated_file_is_rejected_without_panicking() {
        let mut data = cdt_header();
        data.push(0x10); // ID d'un bloc Standard Speed Data
        data.extend_from_slice(&[0x00, 0x00]); // pause
        data.extend_from_slice(&[0x05, 0x00]); // longueur annoncée : 5 octets
        // ... mais le fichier s'arrête avant de les fournir.
        assert!(CdtImage::parse(&data).is_err());
    }

    #[test]
    fn a_bad_signature_is_rejected() {
        let data = b"NOT A CDT FILE".to_vec();
        assert!(CdtImage::parse(&data).is_err());
    }

    #[test]
    fn an_unknown_block_id_is_rejected_rather_than_desyncing() {
        let mut data = cdt_header();
        data.push(0xFF); // ID jamais attribué par le format
        assert!(CdtImage::parse(&data).is_err());
    }

    #[test]
    fn a_minimal_tape_produces_a_pilot_tone_then_toggles_on_data() {
        let mut data = cdt_header();
        data.extend_from_slice(&standard_block(&[0xFF], 10));
        let image = CdtImage::parse(&data).expect("fichier valide");
        assert_eq!(image.blocks.len(), 1);

        let mut tape = Tape::new();
        tape.pending_blocks = image.blocks.into();
        tape.stopped = false;
        tape.motor_on = true;

        // Avant tout tick, aucun pulse n'a encore ete charge : le niveau
        // est celui d'initialisation.
        assert!(!tape.read_bit());

        let initial_level = tape.read_bit();
        let mut toggled = false;
        // Les 3223 pulses de pilote (donnee, flag >= 0x80) durent chacun
        // 2168 T-states : quelques milliers de pas suffisent a observer un
        // premier basculement de niveau.
        for _ in 0..10 {
            tape.tick(2168);
            if tape.read_bit() != initial_level {
                toggled = true;
                break;
            }
        }
        assert!(toggled, "le niveau doit basculer pendant le pilote");
    }

    #[test]
    fn the_motor_being_off_freezes_the_signal() {
        let mut data = cdt_header();
        data.extend_from_slice(&standard_block(&[0xFF], 10));
        let image = CdtImage::parse(&data).expect("fichier valide");

        let mut tape = Tape::new();
        tape.pending_blocks = image.blocks.into();
        tape.stopped = false;
        tape.motor_on = false;

        let level_before = tape.read_bit();
        tape.tick(1_000_000);
        assert_eq!(
            tape.read_bit(),
            level_before,
            "moteur arrete : le signal ne doit pas bouger"
        );
    }

    #[test]
    fn a_stop_the_tape_pause_halts_playback() {
        let mut data = cdt_header();
        data.push(0x20); // Pause/Stop the tape
        data.extend_from_slice(&0u16.to_le_bytes()); // 0 ms = arret
        let image = CdtImage::parse(&data).expect("fichier valide");
        assert_eq!(image.blocks.len(), 1);

        let mut tape = Tape::new();
        tape.pending_blocks = image.blocks.into();
        tape.stopped = false;
        tape.motor_on = true;

        let level_before = tape.read_bit();
        tape.tick(1_000_000);
        assert_eq!(
            tape.read_bit(),
            level_before,
            "un bloc Stop doit figer le signal, pas le faire osciller"
        );
    }

    #[test]
    fn turbo_block_with_nonstandard_timing_is_decoded() {
        // Bloc Turbo Speed Data (0x11) avec des durees de pulse
        // volontairement non standard : c'est le bloc qu'utilisent les
        // protections de copie, donc le seul a vraiment tester ce chemin.
        let mut data = cdt_header();
        let mut block = vec![0x11];
        block.extend_from_slice(&1000u16.to_le_bytes()); // pilot_pulse
        block.extend_from_slice(&500u16.to_le_bytes()); // sync1
        block.extend_from_slice(&600u16.to_le_bytes()); // sync2
        block.extend_from_slice(&400u16.to_le_bytes()); // zero_bit_pulse
        block.extend_from_slice(&900u16.to_le_bytes()); // one_bit_pulse
        block.extend_from_slice(&50u16.to_le_bytes()); // pilot_tone_length
        block.push(8); // used_bits_last_byte
        block.extend_from_slice(&0u16.to_le_bytes()); // pause_ms
        let payload = [0xAAu8];
        block.extend_from_slice(&(payload.len() as u32).to_le_bytes()[..3]); // longueur 24 bits
        block.extend_from_slice(&payload);
        data.extend_from_slice(&block);

        let image = CdtImage::parse(&data).expect("fichier valide");
        assert_eq!(image.blocks.len(), 1);

        let mut tape = Tape::new();
        tape.pending_blocks = image.blocks.into();
        tape.stopped = false;
        tape.motor_on = true;

        // 50 pulses de pilote a 1000 T-states, avancer pile au bord doit
        // laisser le niveau avoir bascule exactement 50 fois (pair -> revenu
        // au niveau initial), puis les deux pulses de sync avant les
        // donnees.
        let initial = tape.read_bit();
        tape.tick(1000 * 50);
        assert_eq!(
            tape.read_bit(),
            initial,
            "50 bascules (pair) doit revenir au niveau de depart"
        );
    }
}
