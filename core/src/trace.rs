use zilog_z80::bus::Bus;

/// Nombre d'instructions conservées. Le tampon est circulaire : on garde
/// toujours les dernières exécutées, ce qui est ce qu'on veut en pratique
/// puisqu'on découvre presque toujours le problème après coup.
const CAPACITY: usize = 65536;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TraceMode {
    Off,
    /// Toutes les instructions exécutées.
    All,
    /// Uniquement les ruptures de séquence (sauts, appels, retours, RST).
    /// Bien plus lisible pour suivre le cheminement d'un programme, et le
    /// tampon couvre une durée d'exécution bien plus longue.
    Branches,
}

/// Une instruction exécutée. Les octets sont relevés au moment de l'exécution
/// plutôt que désassemblés à la volée : c'est peu coûteux à l'enregistrement, et
/// surtout fidèle au code réellement exécuté même si le programme se modifie
/// lui-même ou si la banque mémoire a changé depuis.
#[derive(Clone, Copy)]
pub struct TraceEntry {
    pub pc: u16,
    pub sp: u16,
    pub opcode: [u8; 4],
}

/// Bus minimal exposant les seuls octets relevés, pour les redonner au
/// désassembleur au moment de l'affichage.
struct SnippetBus {
    base: u16,
    bytes: [u8; 4],
}

impl Bus for SnippetBus {
    fn read_byte(&self, address: u16) -> u8 {
        let offset = address.wrapping_sub(self.base) as usize;
        self.bytes.get(offset).copied().unwrap_or(0)
    }
    fn write_byte(&mut self, _address: u16, _data: u8) {}
}

/// Vrai si l'instruction rompt la séquence d'exécution.
fn is_branch(opcode: &[u8; 4]) -> bool {
    match opcode[0] {
        // JP nn, JP (HL), CALL nn, RET, JR e, DJNZ e
        0xC3 | 0xE9 | 0xCD | 0xC9 | 0x18 | 0x10 => true,
        // JP cc,nn
        0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => true,
        // CALL cc,nn
        0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => true,
        // RET cc
        0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => true,
        // JR cc,e
        0x20 | 0x28 | 0x30 | 0x38 => true,
        // RETN / RETI
        0xED => matches!(
            opcode[1],
            0x45 | 0x4D | 0x55 | 0x5D | 0x65 | 0x6D | 0x75 | 0x7D
        ),
        // JP (IX) / JP (IY)
        0xDD | 0xFD => opcode[1] == 0xE9,
        // Les huit RST : bits 7-6 et 2-0 à 1.
        other => other & 0xC7 == 0xC7,
    }
}

pub struct Tracer {
    mode: TraceMode,
    entries: std::collections::VecDeque<TraceEntry>,
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracer {
    pub fn new() -> Self {
        Self {
            mode: TraceMode::Off,
            entries: std::collections::VecDeque::new(),
        }
    }

    /// À tester avant de relever les octets, pour que le tracé désactivé ne
    /// coûte rien d'autre qu'une comparaison.
    pub fn is_recording(&self) -> bool {
        self.mode != TraceMode::Off
    }

    pub fn start(&mut self, mode: TraceMode) {
        self.mode = mode;
        self.entries.clear();
    }

    pub fn stop(&mut self) {
        self.mode = TraceMode::Off;
    }

    pub fn record(&mut self, pc: u16, sp: u16, opcode: [u8; 4]) {
        if self.mode == TraceMode::Branches && !is_branch(&opcode) {
            return;
        }
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(TraceEntry { pc, sp, opcode });
    }

    pub fn status(&self) -> String {
        let mode = match self.mode {
            TraceMode::Off => "off",
            TraceMode::All => "on (toutes les instructions)",
            TraceMode::Branches => "calls (sauts, appels et retours)",
        };
        format!(
            "Trace: {mode} - {} instruction(s) en tampon (capacite {CAPACITY})",
            self.entries.len()
        )
    }

    /// Met en forme les `count` dernières instructions, la plus ancienne d'abord.
    pub fn format_last(&self, count: usize) -> String {
        use std::fmt::Write;
        let skip = self.entries.len().saturating_sub(count);
        let mut s = String::new();
        for entry in self.entries.iter().skip(skip) {
            let bus = SnippetBus {
                base: entry.pc,
                bytes: entry.opcode,
            };
            let (dasm, len) = zilog_z80::dasm::dasm(&bus, entry.pc);
            // dasm() préfixe déjà sa chaîne des octets, dans un formatage qui
            // varie selon le préfixe d'opcode ; on ne garde que le mnémonique et
            // on affiche les octets nous-mêmes, de façon uniforme.
            let text = dasm
                .split_once("  ")
                .map_or(dasm.as_str(), |(_, rest)| rest.trim_start())
                .to_string();
            let bytes: Vec<String> = entry.opcode[..len.min(4) as usize]
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect();
            let _ = writeln!(
                s,
                "{:04X}  {:<11} {:<20} SP:{:04X}",
                entry.pc,
                bytes.join(" "),
                text,
                entry.sp
            );
        }
        s
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_detection_covers_every_sequence_break() {
        for opcode in [
            0xC3, // JP nn
            0xE9, // JP (HL)
            0xCD, // CALL nn
            0xC9, // RET
            0x18, // JR e
            0x10, // DJNZ e
            0xCA, // JP Z,nn
            0xC4, // CALL NZ,nn
            0xD8, // RET C
            0x28, // JR Z,e
            0xC7, 0xCF, 0xD7, 0xDF, 0xE7, 0xEF, 0xF7, 0xFF, // les huit RST
        ] {
            assert!(is_branch(&[opcode, 0, 0, 0]), "opcode {opcode:#04X}");
        }

        assert!(is_branch(&[0xED, 0x4D, 0, 0])); // RETI
        assert!(is_branch(&[0xED, 0x45, 0, 0])); // RETN
        assert!(is_branch(&[0xDD, 0xE9, 0, 0])); // JP (IX)

        for opcode in [0x00, 0x3E, 0x76, 0x77, 0xAF, 0xEB, 0x06, 0x21] {
            assert!(!is_branch(&[opcode, 0, 0, 0]), "opcode {opcode:#04X}");
        }
        assert!(!is_branch(&[0xED, 0xB0, 0, 0])); // LDIR
        assert!(!is_branch(&[0xDD, 0x21, 0, 0])); // LD IX,nn
    }

    #[test]
    fn branches_mode_keeps_only_the_sequence_breaks() {
        let mut tracer = Tracer::new();
        tracer.start(TraceMode::Branches);
        tracer.record(0x1000, 0xC000, [0x00, 0, 0, 0]); // NOP
        tracer.record(0x1001, 0xC000, [0xCD, 0x34, 0x12, 0]); // CALL &1234
        tracer.record(0x1234, 0xC000, [0xAF, 0, 0, 0]); // XOR A
        tracer.record(0x1235, 0xC000, [0xC9, 0, 0, 0]); // RET
        assert_eq!(tracer.len(), 2);

        let dump = tracer.format_last(10);
        assert!(dump.contains("CALL"), "{dump}");
        assert!(dump.contains("RET"), "{dump}");
        assert!(!dump.contains("XOR"), "{dump}");
    }

    #[test]
    fn all_mode_keeps_everything_and_start_clears_the_buffer() {
        let mut tracer = Tracer::new();
        tracer.start(TraceMode::All);
        tracer.record(0x1000, 0xC000, [0x00, 0, 0, 0]);
        tracer.record(0x1001, 0xC000, [0x00, 0, 0, 0]);
        assert_eq!(tracer.len(), 2);

        tracer.start(TraceMode::All);
        assert_eq!(tracer.len(), 0);
    }

    /// Les octets sont ceux relevés à l'exécution, pas relus en mémoire : une
    /// instruction réécrite depuis reste affichée telle qu'elle a tourné.
    #[test]
    fn disassembly_uses_the_bytes_captured_at_execution_time() {
        let mut tracer = Tracer::new();
        tracer.start(TraceMode::All);
        tracer.record(0x8000, 0xBFF0, [0x21, 0x00, 0xC0, 0x00]); // LD HL,&C000
        let dump = tracer.format_last(1);
        assert!(dump.contains("8000"), "{dump}");
        assert!(dump.contains("21 00 C0"), "{dump}");
        assert!(dump.contains("SP:BFF0"), "{dump}");
    }

    #[test]
    fn recording_is_off_by_default() {
        let tracer = Tracer::new();
        assert!(!tracer.is_recording());
        assert!(tracer.status().contains("off"));
    }
}
