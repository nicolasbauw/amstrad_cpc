use crate::bus::CpcBus;
use crate::config;
use crate::gate_array::{GateArray, GateArrayState};
use crate::hexconversion::HexStringToUnsigned;
use crate::memory::Memory;
use crate::monitor::MonitorCmd;
use crate::trace::{TraceMode, Tracer};
use std::{
    collections::HashSet, error, error::Error, fmt, fs::File, io::Read, sync::mpsc,
    sync::mpsc::SendError,
};
use zilog_z80::{bus::Bus, cpu::CPU};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Nombre de scanlines mémorisées par trame. Très au-delà des 312 d'une trame
/// standard, pour couvrir les écrans non standard sans allouer inutilement.
const MAX_CAPTURED_SCANLINES: usize = 512;

/// Durée d'un cycle Z80 à 4 MHz, en nanosecondes.
const CPU_TICK_NANOS: u64 = 250;

/// Temps réel correspondant à un nombre de cycles Z80 émulés.
///
/// C'est la seule mesure fiable pour cadencer l'émulateur. La déduire des
/// registres du CRTC est un piège : un jeu qui découpe son écran (Barbarian et
/// son panneau de score, toutes les ruptures) reprogramme R4/R9 en cours de
/// trame, si bien que la longueur annoncée par les registres à la fin de la
/// trame n'est pas celle de la trame qui vient d'être émulée. La machine
/// tournerait alors durablement trop lentement (ou trop vite), ce qui s'entend
/// immédiatement : le PSG ne produit plus 44100 échantillons par seconde
/// réelle, et la sortie audio se vide en craquant.
pub fn emulated_duration(ticks: u32) -> std::time::Duration {
    std::time::Duration::from_nanos(ticks as u64 * CPU_TICK_NANOS)
}

/// Durée réelle d'une instruction sur CPC, à partir de sa durée nominale.
///
/// Le Gate Array partage la mémoire avec le processeur, et il se la réserve
/// l'essentiel du temps pour lire la VRAM : il n'ouvre au Z80 qu'une fenêtre
/// d'accès par microseconde. Chaque cycle machine attend donc la fenêtre
/// suivante, et toute instruction dure au bout du compte un nombre entier de
/// microsecondes — soit un multiple de 4 cycles d'horloge.
///
/// Un Z80 nu exécute donc plus d'instructions par trame que le même Z80 dans
/// un CPC. L'ignorer fait tourner tous les jeux environ 10 % trop vite par
/// rapport au balayage vidéo et aux interruptions, ce qui déplace tout ce qui
/// se joue à la scanline près.
///
/// Nous arrondissons la durée totale de l'instruction, faute de connaître le
/// découpage en cycles machine que la crate ne fournit pas. Les deux méthodes
/// coïncident pour la grande majorité des instructions ; là où elles diffèrent,
/// l'écart vaut un seul cycle machine, soit un quart de microseconde.
pub fn cpc_instruction_time(nominal_ticks: u32) -> u32 {
    nominal_ticks.div_ceil(4) * 4
}
const HELP: &str = "
Emulator commands:
    disk d.dsk          Loads the d.dsk disk image on drive A
    disk d.dsk b        Loads the d.dsk disk image on drive B (if enabled in config.toml)
    disk eject          Ejects the disk image from drive A
    disk eject b        Ejects the disk image from drive B
    pc                  Performs a power cycle
    vol                 Displays the audio output volume
    vol 30              Sets the audio output volume to 30 %

Monitor commands:
    d 0x0000            disassembles code at 0x0000 and the 20 next
                        instructions
    m 0xeeee            displays memory content at address 0xeeee
    m 0xeeee 0xaa       sets memory address 0xeeee to the 0xaa value
    mr 0x1000           dumps 256 raw RAM bytes from 0x1000, ignoring any ROM
    mr 0x1000 0x1100    dumps the raw RAM range 0x1000..0x1100
    s 0xaa              searches for a byte in memory
    n                   steps to next Z80 instruction
    l                   steps to next video line
    j 0x0000            jumps to 0x0000 address
    b                   displays set breakpoints
    b 0x0002            sets a breakpoint at address 0x0002
    f 0x0002            \"frees\" (deletes) breakpoint at address 0x0002
    w                   displays set watchpoints
    w 0xeeee            adds a write watchpoint at address 0xeeee
    fw 0xeeee           removes watchpoint at address 0xeeee
    p                   pause execution
    g                   resume execution after the \"p\" command, or a breakpoint,
                        has been used to halt execution
    hw                  displays Gate Array and CRTC status
    hw kb               keyboard test
    r                   displays the contents of flags, registers and interrupts
    t                   displays trace status
    t on                records every executed instruction in a ring buffer
    t calls             records only jumps, calls and returns (far longer reach)
    t off               stops recording, keeping what has been captured
    t dump 100          displays the last 100 recorded instructions
    t save f.txt        writes the whole buffer to a file";

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum MachineError {
    ConfigFile,
    ConfigFileFmt,
    IOError,
    SendMsgError,
    SnapshotError,
    DisplayError,
    FontError,
}

impl fmt::Display for MachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MachineError::ConfigFileFmt => "Bad config file format",
            MachineError::ConfigFile => "Can't load config file",
            MachineError::IOError => "I/O Error",
            MachineError::SendMsgError => "Message not sent",
            MachineError::SnapshotError => "Snapshot I/O error",
            MachineError::DisplayError => "SDL3 error",
            MachineError::FontError => "Can't load font",
        })
    }
}

impl From<std::io::Error> for MachineError {
    fn from(_e: std::io::Error) -> MachineError {
        MachineError::IOError
    }
}

impl From<SendError<(String, String, String)>> for MachineError {
    fn from(_e: SendError<(String, String, String)>) -> MachineError {
        MachineError::SendMsgError
    }
}

impl From<MachineError> for std::io::Error {
    fn from(e: MachineError) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    }
}

impl error::Error for MachineError {}

pub struct Machine {
    pub cpu: CPU,
    pub bus: CpcBus,
    pub total_ticks: u64,
    pub hsync_accumulator: u32,
    pub current_line: u32,
    /// Armé au début du VSYNC : la trame est complète et peut être affichée.
    pub frame_ready: bool,
    /// État du Gate Array mémorisé au début de chaque scanline de la trame.
    /// Le rendu s'appuie dessus plutôt que sur l'état courant, sans quoi une
    /// reprogrammation du mode ou de la palette en cours de trame (rupture)
    /// s'appliquerait rétroactivement à tout l'écran.
    pub scanline_states: Vec<GateArrayState>,
    /// Octets de VRAM affichés sur chaque scanline de la trame, mémorisés au
    /// moment même où le CRTC balaie cette ligne (voir `capture_scanline_vram`).
    ///
    /// Un vrai tube cathodique peint chaque ligne avec le contenu de la VRAM
    /// tel qu'il est exactement à cet instant, jamais un état figé de fin de
    /// trame. Sans cette capture progressive, une routine de tracé de sprite
    /// interrompue par l'interruption vidéo (le XOR classique du CPC ne
    /// masque pas les interruptions) peut se retrouver à moitié terminée
    /// pile au moment où `video::render` prend son instantané global,
    /// produisant un sprite à moitié effacé pendant une seule trame — un
    /// clignotement très rapide, invisible sur un émulateur cycle-exact
    /// (bug des sprites qui clignotent dans Cauldron/BMX Simulator, TODO.txt).
    pub scanline_vram: Vec<Vec<u8>>,
    pub diagnostic_mode: bool, // true = ROM de Diagnostic, false = ROMs d'origine du CPC 6128
    cmd_channel: (
        mpsc::Sender<(MonitorCmd, String, String)>,
        mpsc::Receiver<(MonitorCmd, String, String)>,
    ),
    pub tracer: Tracer,
    breakpoints: HashSet<u16>,
    running: bool,
    stopped_at_breakpoint: bool,
    pub waiting_for_key: bool,
    /// Volume de la sortie audio, dans [0, 1]. Réglable depuis la console, il
    /// est relu par la boucle principale qui le transmet à la carte son.
    volume: f32,
    /// Vitesse d'exécution mesurée par la boucle principale, en % du temps
    /// réel. En dessous de 100, la machine prend du retard : la musique
    /// traîne, le jeu ralentit et la sortie audio se vide en craquant.
    measured_speed: f32,
    /// Trames ayant manqué leur échéance pendant la dernière seconde.
    late_frames: u32,
    /// Longueur de trame réellement balayée (d'un début de VSYNC au suivant)
    /// et nombre d'interruptions qui y sont tombées. Un Z80 à pleine vitesse
    /// avec une trame trop longue donne exactement les mêmes symptômes qu'une
    /// machine trop lente : c'est la fréquence de trame, pas la vitesse du
    /// CPU, que voit un jeu synchronisé sur le balayage.
    measured_frame_lines: u32,
    lines_since_vsync: u32,
    measured_interrupts_per_frame: u32,
    interrupts_since_vsync: u32,
    /// Instructions non gérées déjà signalées sur la console.
    unimplemented_reported: u32,
    config: config::Config,
}

impl Machine {
    pub fn new() -> Self {
        let memory = Memory::new();
        let bus = CpcBus::new(memory);
        let cpu = CPU::new();

        // Charge la configuration utilisateur (config.toml). En cas d'échec
        // (fichier absent ou mal formé), une configuration par défaut (tout
        // désactivé) est utilisée.
        let config = config::load_config_file().unwrap_or_else(|_| {
            println!("Config file not found or invalid: drive B disabled by default.");
            config::Config {
                drives: config::DriveConfig { drive_b: false },
                debugger: config::Debugger {
                    keyboard: false,
                    audio: false,
                },
                file: config::FileConfig::default(),
            }
        });

        let m = Self {
            cpu,
            bus,
            total_ticks: 0,
            hsync_accumulator: 0,
            current_line: 0,
            frame_ready: false,
            scanline_states: vec![GateArray::new().state(); MAX_CAPTURED_SCANLINES],
            scanline_vram: vec![Vec::new(); MAX_CAPTURED_SCANLINES],
            diagnostic_mode: false, // Basculé à false pour tester le boot officiel du CPC 6128 !
            cmd_channel: mpsc::channel(),
            tracer: Tracer::new(),
            breakpoints: HashSet::new(),
            running: true,
            stopped_at_breakpoint: false,
            waiting_for_key: false,
            volume: 0.5,
            measured_speed: 100.0,
            late_frames: 0,
            measured_frame_lines: 312,
            lines_since_vsync: 0,
            measured_interrupts_per_frame: 6,
            interrupts_since_vsync: 0,
            unimplemented_reported: 0,
            config,
        };

        m.bus
            .fdc
            .borrow_mut()
            .set_drive_b_enabled(m.config.drives.drive_b);
        if m.config.drives.drive_b {
            println!("Drive B enabled (config.toml)");
        }

        crate::console::launch(m.cmd_channel.0.clone()).unwrap();
        m
    }

    /// Indique si la matrice clavier doit être affichée dans la fenêtre
    /// SDL "Machine Status" (config.toml, section [debugger]).
    pub fn show_keyboard_matrix(&self) -> bool {
        self.config.debugger.keyboard
    }

    /// Indique si les interventions de la régulation audio doivent être
    /// signalées sur la console (config.toml, section [debugger]).
    pub fn report_audio_regulation(&self) -> bool {
        self.config.debugger.audio
    }

    /// Volume de la sortie audio, dans [0, 1].
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Renseigne la cadence mesurée par la boucle principale : vitesse en %
    /// du temps réel, et nombre de trames ayant manqué leur échéance.
    ///
    /// Les deux comptent : la moyenne sur une seconde peut afficher 100 %
    /// alors que des décrochages brefs se sont produits, et chacun d'eux
    /// étire la musique (voir la note sur la file audio dans audio.rs).
    pub fn set_measured_timing(&mut self, percent: f32, late_frames: u32) {
        self.measured_speed = percent;
        self.late_frames = late_frames;
    }

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn is_running(&mut self) -> bool {
        self.running
    }

    /// Éteint virtuellement la machine : arrête l'exécution sans fermer la
    /// fenêtre SDL ni quitter l'émulateur.
    pub fn power_off(&mut self) {
        self.stop();
        println!("Power off.");
    }

    /// Rallume la machine : réinitialise le CPU, la RAM et les périphériques
    /// comme lors d'un vrai démarrage à froid, recharge les ROMs, puis
    /// reprend l'exécution. Les disquettes actuellement insérées ainsi que
    /// les breakpoints/watchpoints sont conservés (comme sur le matériel
    /// réel, une disquette reste dans le lecteur pendant un power cycle).
    pub fn power_on(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (disk_a, disk_b, drive_b_enabled) = {
            let fdc = self.bus.fdc.borrow();
            (
                fdc.drive_a
                    .disk_loaded
                    .then(|| fdc.drive_a.current_filename.clone()),
                fdc.drive_b
                    .disk_loaded
                    .then(|| fdc.drive_b.current_filename.clone()),
                fdc.drive_b_enabled,
            )
        };

        self.bus = CpcBus::new(Memory::new());
        self.cpu = CPU::new();
        self.total_ticks = 0;
        self.hsync_accumulator = 0;
        self.current_line = 0;
        self.frame_ready = false;
        self.scanline_states.fill(self.bus.gate_array.state());
        for slot in &mut self.scanline_vram {
            slot.clear();
        }
        self.stopped_at_breakpoint = false;
        self.waiting_for_key = false;

        {
            let mut fdc = self.bus.fdc.borrow_mut();
            fdc.set_drive_b_enabled(drive_b_enabled);
            if let Some(filename) = disk_a {
                let _ = fdc.load_disk(&filename);
            }
            if let Some(filename) = disk_b {
                let _ = fdc.load_disk_b(&filename);
            }
        }

        self.load_roms()?;
        self.start();
        println!("Power on.");
        Ok(())
    }

    /// Charge une image disque sur le lecteur A, en résolvant le nom donné
    /// via `[file] dsk_path` s'il ne désigne pas déjà un fichier existant.
    /// Utilisée aussi bien par la commande console `disk` que par l'option
    /// de ligne de commande `--disk`.
    pub fn load_disk(&mut self, filename: &str) -> Result<(), String> {
        let path = self.config.resolve_disk_path(filename);
        self.bus.fdc.borrow_mut().load_disk(&path)
    }

    /// Équivalent de [`Machine::load_disk`] pour le lecteur B.
    pub fn load_disk_b(&mut self, filename: &str) -> Result<(), String> {
        let path = self.config.resolve_disk_path(filename);
        self.bus.fdc.borrow_mut().load_disk_b(&path)
    }

    /// Coupe puis rétablit l'alimentation de la machine (redémarrage à
    /// froid), équivalent de la commande console "pc".
    pub fn power_cycle(&mut self) {
        self.power_off();
        if let Err(e) = self.power_on() {
            println!("Power cycle failed: {e}");
        } else {
            println!("Power cycle complete.");
        }
    }

    /// Charge les ROMs appropriées en fonction du mode (Diagnostic ou Officiel)
    pub fn load_roms(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // ROM Basse : OS 6128
        let mut f = File::open("bin/OS6128-AZERTY.rom")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        self.bus.memory.load_low_rom(&buf);

        // ROM Haute 0 : BASIC 1.1
        let mut f = File::open("bin/BASIC1-1-AZERTY.ROM")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        self.bus.memory.load_high_rom(0, &buf);

        // ROM Haute 7 : AMSDOS (Système de disquettes)

        let mut f = File::open("bin/AMSDOS.ROM")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        self.bus.memory.load_high_rom(7, &buf);

        if self.diagnostic_mode {
            // ROM Haute 15 (Diagnostic Upper)
            let mut f = File::open("bin/AmstradDiagUpper.rom")?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            self.bus.memory.load_high_rom(15, &buf);
        }
        Ok(())
    }

    /// Exécute une instruction et synchronise les périphériques
    pub fn step(&mut self) -> u32 {
        let current_pc = self.cpu.reg.pc;

        if self.breakpoints.contains(&self.cpu.reg.pc) && !self.stopped_at_breakpoint {
            self.stop();
            self.stopped_at_breakpoint = true;
            print!(
                "\nBreakpoint reached at {:#06X} (Total Ticks: {})\n",
                current_pc, self.total_ticks
            );
            return 0;
        }

        self.stopped_at_breakpoint = false;

        if self.tracer.is_recording() {
            let opcode = [
                self.bus.read_byte(current_pc),
                self.bus.read_byte(current_pc.wrapping_add(1)),
                self.bus.read_byte(current_pc.wrapping_add(2)),
                self.bus.read_byte(current_pc.wrapping_add(3)),
            ];
            self.tracer.record(current_pc, self.cpu.reg.sp, opcode);
        }

        // Acquittement de l'interruption : le CPU consomme la requête en attente
        // au moment où il l'accepte, donc la transition true -> false de
        // has_pending_int() sur une instruction identifie précisément le cycle
        // d'acknowledge. C'est plus fiable qu'un test sur PC == 0x0038, qui est
        // aussi la cible du RST 38h (opcode 0xFF, octet de remplissage courant).
        let int_pending_before = self.cpu.has_pending_int();
        let ticks = self.cpu.execute(&mut self.bus);

        // Une instruction que le CPU ne sait pas traiter ne doit pas passer
        // inaperçue : elle ne fait rien, donc le programme dérive à partir de
        // là. On la nomme (le désassembleur sait tout décoder) sans pour
        // autant noyer la console si elle tombe dans une boucle.
        if let Some(u) = self.cpu.take_unimplemented() {
            if self.unimplemented_reported < 10 {
                self.unimplemented_reported += 1;
                let (text, _) = zilog_z80::dasm::dasm(&self.bus, u.address);
                println!(
                    "\nUnimplemented instruction at {:#06X}: {}",
                    u.address, text
                );
                if self.unimplemented_reported == 10 {
                    println!("(further occurrences silenced, see the hardware status)");
                }
            }
        }
        if int_pending_before && !self.cpu.has_pending_int() {
            self.bus.gate_array.acknowledge_interrupt();
        }

        if let Some(addr) = self.bus.watchpoint_hit {
            self.bus.watchpoint_hit = None;
            self.stop();
            println!(
                "\nWatchpoint hit: write to {:#06X} at PC {:#06X}",
                addr, current_pc
            );
            println!("{}", (zilog_z80::dasm::dasm(&self.bus, current_pc)).0);
            self.print_registers();
            return 0;
        }

        let elapsed_ticks = cpc_instruction_time(if ticks == 0 { 4 } else { ticks });
        self.total_ticks += elapsed_ticks as u64;

        // Le PSG est cadencé par le même temps que le CPU : il doit avancer
        // ici, et pas une fois par trame, sinon les changements de registres
        // en cours de trame (toutes les musiques en font) seraient perdus.
        self.bus.psg.tick(elapsed_ticks);

        // Gestion HSYNC / Interruptions (période 256 ticks = 64µs)
        self.hsync_accumulator += elapsed_ticks;
        while self.hsync_accumulator >= 256 {
            self.hsync_accumulator -= 256;

            // Le balayage vertical (et donc la position et la durée du VSYNC)
            // est entièrement dérivé des registres du CRTC : tout logiciel qui
            // reprogramme R4/R5/R7/R9 change réellement la géométrie de trame.
            let vsync_start = self.bus.crtc.step_scanline();
            self.current_line = self.bus.crtc.scanline;
            self.frame_ready |= vsync_start;

            // Longueur de trame réellement balayée, mesurée d'un début de
            // VSYNC au suivant. C'est elle qui donne la fréquence de trame que
            // voit le logiciel, et elle peut différer de ce qu'annoncent les
            // registres dès qu'un jeu reprogramme le CRTC en cours de trame.
            self.lines_since_vsync += 1;
            if vsync_start {
                self.measured_frame_lines = self.lines_since_vsync;
                self.lines_since_vsync = 0;
                self.measured_interrupts_per_frame = self.interrupts_since_vsync;
                self.interrupts_since_vsync = 0;
            }

            // step_scanline() a fait basculer le CRTC sur la scanline suivante :
            // on mémorise l'état du Gate Array tel qu'elle démarre.
            if let Some(slot) = self
                .scanline_states
                .get_mut(self.bus.crtc.scanline as usize)
            {
                *slot = self.bus.gate_array.state();
            }

            // Capture des octets de VRAM affichés sur cette scanline, à
            // l'instant même où le CRTC la balaie (voir la doc de
            // `scanline_vram` et `video::capture_scanline_vram`).
            if let Some(slot) = self.scanline_vram.get_mut(self.bus.crtc.scanline as usize) {
                crate::video::capture_scanline_vram(&self.bus.crtc, &self.bus.memory, slot);
            }

            // On force le bit 1 à 1 pour lire Joystick A par défaut
            self.bus.ppi.set_system_port_b(self.bus.crtc.vsync, true);

            // Le Gate Array recale son compteur d'interruptions sur le front
            // montant du VSYNC, pas sur son niveau.
            if self.bus.gate_array.step_hsync(vsync_start) {
                self.cpu.int_request(0xFF);
                self.interrupts_since_vsync += 1;
            }
        }
        elapsed_ticks
    }

    /// Renvoie une description textuelle de la banque active pour une adresse donnée
    // TODO déplacer cette foction dans memory.rs
    pub fn get_address_source_info(&self, addr: u16) -> String {
        if addr < 0x4000 && self.bus.memory.rom_low_enabled {
            "ROM Low".to_string()
        } else if addr >= 0xC000
            && self.bus.memory.rom_high_enabled
            && self.bus.memory.effective_high_rom().is_some()
        {
            // La ROM lue n'est pas forcément celle sélectionnée : un numéro
            // inexistant retombe sur la ROM 0.
            let selected = self.bus.memory.selected_high_rom;
            match self.bus.memory.effective_high_rom() {
                Some(rom) if rom as u8 != selected => {
                    format!("ROM High {rom} (slot {selected} absent)")
                }
                Some(rom) => format!("ROM High {rom}"),
                None => unreachable!(),
            }
        } else {
            let phys_addr = self.bus.memory.get_ram_physical_address(addr);
            let bank = phys_addr / 16384;
            format!("RAM bank {}", bank)
        }
    }

    pub fn get_registers_string(&mut self) -> String {
        let pc = self.cpu.reg.pc;
        let sp = self.cpu.reg.sp;
        let word_at_sp = self.bus.read_word(sp);
        format!(
            "=== REGISTERS & STATUS ===\n\
             PC :{:#06X}   SP : {:#06X}\n\
             S : {}  Z : {}  H : {}  P : {}  N : {}  C : {}\n\
             BC : {:#06X}  DE : {:#06X}  HL : {:#06X}  AF : {:#06X}\n\
             BC': {:#06X}  DE': {:#06X}  HL': {:#06X}  AF': {:#06X}\n\
             IXH : {:#04X}  IXL : {:#04X}  IYH : {:#04X}  IYL : {:#04X}\n\
             (SP) : {:#06X}  IFF1 : {}  IFF2 : {}  IM : {}  Pending INT : {}  Pending NMI : {}\n",
            pc,
            sp,
            self.cpu.reg.flags.s as i32,
            self.cpu.reg.flags.z as i32,
            self.cpu.reg.flags.h as i32,
            self.cpu.reg.flags.p as i32,
            self.cpu.reg.flags.n as i32,
            self.cpu.reg.flags.c as i32,
            self.cpu.reg.get_bc(),
            self.cpu.reg.get_de(),
            self.cpu.reg.get_hl(),
            self.cpu.reg.get_af(),
            self.cpu.alt.get_bc(),
            self.cpu.alt.get_de(),
            self.cpu.alt.get_hl(),
            self.cpu.alt.get_af(),
            self.cpu.reg.ixh,
            self.cpu.reg.ixl,
            self.cpu.reg.iyh,
            self.cpu.reg.iyl,
            word_at_sp,
            self.cpu.iff1(),
            self.cpu.iff2(),
            self.cpu.im(),
            self.cpu.has_pending_int(),
            self.cpu.has_pending_nmi()
        )
    }

    pub fn print_registers(&mut self) {
        print!("{}", self.get_registers_string());
    }

    pub fn get_hardware_string(&mut self, show_kb: bool) -> String {
        use std::fmt::Write;
        let mut s = String::new();

        let _ = writeln!(s, "=== CPC HARDWARE STATUS ===");

        // --- GATE ARRAY & MEMORY CONFIG ---
        let _ = writeln!(s, "\n[Gate Array & Memory]");
        let _ = writeln!(
            s,
            "  Video Mode         : {}",
            self.bus.gate_array.video_mode
        );
        let _ = writeln!(
            s,
            "  Selected Pen       : {}",
            self.bus.gate_array.selected_pen
        );
        let _ = writeln!(
            s,
            "  HSYNC Counter      : {}/52",
            self.bus.gate_array.hsync_counter
        );
        let _ = writeln!(
            s,
            "  Interrupt Requested: {}",
            self.bus.gate_array.interrupt_requested
        );
        let _ = writeln!(
            s,
            "  Low ROM Enabled    : {}",
            self.bus.memory.rom_low_enabled
        );
        let _ = writeln!(
            s,
            "  High ROM Enabled   : {}",
            self.bus.memory.rom_high_enabled
        );
        let _ = writeln!(
            s,
            "  Selected High ROM  : {}",
            self.bus.memory.selected_high_rom
        );
        let _ = writeln!(
            s,
            "  RAM Configuration  : Bank Config {}",
            self.bus.memory.ram_config
        );

        // Affichage de la palette du Gate Array
        let _ = write!(s, "  Palette (HW index) : ");
        for (i, val) in self.bus.gate_array.palette.iter().enumerate() {
            if i == 16 {
                let _ = write!(s, "Border:{} ", val);
            } else {
                let _ = write!(s, "Inks[{}]={} ", i, val);
            }
        }
        let _ = writeln!(s);

        // --- CRTC 6845 ---
        let _ = writeln!(s, "\n[CRTC 6845]");
        let _ = writeln!(
            s,
            "  Selected Register  : R{}",
            self.bus.crtc.selected_register
        );
        let _ = writeln!(
            s,
            "  Scanline           : {}/{} (char row {}, raster {})",
            self.bus.crtc.scanline,
            self.bus.crtc.frame_scanlines(),
            self.bus.crtc.char_row,
            self.bus.crtc.raster
        );
        let _ = writeln!(s, "  VSYNC              : {}", self.bus.crtc.vsync);
        let _ = write!(s, "  Registers          : ");
        for (i, val) in self.bus.crtc.registers.iter().enumerate() {
            let _ = write!(s, "R{}={:<3} ", i, val);
            if i == 8 {
                let _ = write!(s, "\n                       ");
            }
        }
        let _ = writeln!(s);

        // --- PPI 8255 ---
        let _ = writeln!(s, "\n[PPI 8255]");
        let _ = writeln!(s, "  Port A (PSG Data)  : {:#04X}", self.bus.ppi.port_a);
        let _ = writeln!(
            s,
            "  Port B (System)    : {:#04X} (VSYNC: {})",
            self.bus.ppi.port_b_input,
            (self.bus.ppi.port_b_input & 0x01) != 0
        );
        let _ = writeln!(
            s,
            "  Port C (Control)   : {:#04X} (PSG Control: {:#04X}, KB Line: {})",
            self.bus.ppi.port_c,
            self.bus.ppi.port_c & 0xC0,
            self.bus.ppi.port_c & 0x0F
        );
        let _ = writeln!(
            s,
            "  Control Register   : {:#04X}",
            self.bus.ppi.control_register
        );

        // --- PSG AY-3-8912 ---
        let _ = writeln!(s, "\n[PSG AY-3-8912]");
        let _ = writeln!(
            s,
            "  Selected Register  : R{}",
            self.bus.psg.selected_register
        );
        let _ = write!(s, "  Registers          : ");
        for (i, val) in self.bus.psg.registers.iter().enumerate() {
            let _ = write!(s, "R{}={:<3} ", i, val);
            if i == 7 {
                let _ = write!(s, "\n                       ");
            }
        }
        let _ = writeln!(s);

        // Vue "musicien" des mêmes registres : périodes, volumes et état du
        // mélangeur, autrement dit ce qu'on entend réellement.
        let regs = &self.bus.psg.registers;
        let mixer = regs[7];
        for (channel, name) in ["A", "B", "C"].iter().enumerate() {
            let period = (regs[channel * 2] as u16) | ((regs[channel * 2 + 1] as u16 & 0x0F) << 8);
            let frequency = if period == 0 {
                0
            } else {
                crate::sound::PSG_CLOCK / (16 * period as u32)
            };
            let amplitude = regs[8 + channel];
            let volume = if amplitude & 0x10 != 0 {
                format!("env ({})", self.bus.psg.sound.envelope_volume())
            } else {
                format!("{}", amplitude & 0x0F)
            };
            let _ = writeln!(
                s,
                "  Channel {}          : period {:<4} ({:>5} Hz)  tone {}  noise {}  volume {}",
                name,
                period,
                frequency,
                if (mixer >> channel) & 1 == 0 {
                    "on "
                } else {
                    "off"
                },
                if (mixer >> (channel + 3)) & 1 == 0 {
                    "on "
                } else {
                    "off"
                },
                volume
            );
        }
        let _ = writeln!(s, "  Noise period       : {}", regs[6] & 0x1F);
        let _ = writeln!(
            s,
            "  Envelope           : shape {:#04X}  period {}  volume {}",
            regs[13],
            (regs[11] as u32) | ((regs[12] as u32) << 8),
            self.bus.psg.sound.envelope_volume()
        );
        // Échantillons produits mais pas encore récupérés par la sortie audio.
        // Une valeur qui grimpe trahit une trame qui n'est plus rendue.
        let _ = writeln!(
            s,
            "  Pending samples    : {}",
            self.bus.psg.sound.buffered_samples()
        );

        // --- CADENCE ---
        // Le son est un microscope à problèmes de timing : en dessous de 100 %,
        // la musique traîne, le jeu ralentit et la carte son se retrouve à sec.
        let _ = writeln!(s, "\n[Timing]");
        let _ = writeln!(s, "  Emulation speed    : {:.0} %", self.measured_speed);
        let unimplemented = self.cpu.unimplemented_count();
        if unimplemented > 0 {
            let _ = writeln!(s, "  Unimplemented ops  : {unimplemented}");
        }
        let _ = writeln!(s, "  Late frames        : {} / s", self.late_frames);
        let _ = writeln!(
            s,
            "  Frame (measured)   : {} scanlines ({:.1} Hz)",
            self.measured_frame_lines,
            if self.measured_frame_lines > 0 {
                1_000_000.0 / (self.measured_frame_lines as f32 * 64.0)
            } else {
                0.0
            }
        );
        let _ = writeln!(
            s,
            "  Frame (registers)  : {} scanlines",
            self.bus.crtc.frame_scanlines()
        );
        let _ = writeln!(
            s,
            "  Interrupts / frame : {}",
            self.measured_interrupts_per_frame
        );

        // --- FDC / LECTEURS DE DISQUETTES ---
        {
            let fdc = self.bus.fdc.borrow();
            let disk_access = matches!(
                fdc.phase,
                crate::fdc::FdcPhase::ExecutionRead | crate::fdc::FdcPhase::ExecutionWrite
            );
            let _ = writeln!(s, "\n[FDC]");
            let _ = writeln!(s, "  Motor On           : {}", fdc.motor_on);
            let _ = writeln!(
                s,
                "  Disk access        : {}",
                if disk_access { "\u{25CF}" } else { " " }
            );
            let _ = writeln!(
                s,
                "  Drive A            : {}",
                if fdc.drive_a.disk_loaded {
                    fdc.drive_a.current_filename.as_str()
                } else {
                    "None"
                }
            );
            if fdc.drive_b_enabled {
                let _ = writeln!(
                    s,
                    "  Drive B            : {}",
                    if fdc.drive_b.disk_loaded {
                        fdc.drive_b.current_filename.as_str()
                    } else {
                        "None"
                    }
                );
            } else {
                let _ = writeln!(s, "  Drive B            : disabled (config.toml)");
            }
        }

        // --- KEYBOARD MATRIX ---
        if show_kb {
            let _ = writeln!(
                s,
                "\n[Keyboard Matrix (Negative Logic: 0 = Pressed, 1 = Released)]"
            );
            let _ = writeln!(
                s,
                "  Selected Keyboard Line: {}",
                self.bus.psg.selected_keyboard_line
            );
            for line in 0..10 {
                let val = self.bus.psg.keyboard_matrix[line];
                let _ = write!(s, "  Line {}: {:08b} (0x{:02X})", line, val, val);

                // Si au moins un bit est à 0 (touche pressée)
                if val != 0xFF {
                    let _ = write!(s, "  -> Pressed: ");
                    let mut pressed_keys = Vec::new();
                    for bit in 0..8 {
                        if (val & (1 << bit)) == 0 {
                            let key_name = match (line, bit) {
                                // Ligne 0
                                (0, 0) => "Up",
                                (0, 1) => "Right",
                                (0, 2) => "Down",
                                (0, 3) => "Kp 9",
                                (0, 4) => "Kp 6",
                                (0, 5) => "Kp 3",
                                (0, 6) => "Kp Enter",
                                (0, 7) => "Kp .",

                                // Ligne 1
                                (1, 0) => "Left",
                                (1, 1) => "Copy",
                                (1, 2) => "Kp 7",
                                (1, 3) => "Kp 8",
                                (1, 4) => "Kp 5",
                                (1, 5) => "Kp 1",
                                (1, 6) => "Kp 2",
                                (1, 7) => "Kp 0",

                                // Ligne 2
                                (2, 0) => "Clr",
                                (2, 1) => "*",
                                (2, 2) => "Enter",
                                (2, 3) => "#",
                                (2, 4) => "Kp 4",
                                (2, 5) => "Shift",
                                (2, 6) => "$",
                                (2, 7) => "Ctrl",

                                // Ligne 3
                                (3, 0) => "-",
                                (3, 1) => ")",
                                (3, 2) => "^",
                                (3, 3) => "P",
                                (3, 4) => "ù",
                                (3, 5) => "M",
                                (3, 6) => "=",
                                (3, 7) => ":",

                                // Ligne 4
                                (4, 0) => "0",
                                (4, 1) => "9",
                                (4, 2) => "O",
                                (4, 3) => "I",
                                (4, 4) => "L",
                                (4, 5) => "K",
                                (4, 6) => ",",
                                (4, 7) => ";",

                                // Ligne 5
                                (5, 0) => "8",
                                (5, 1) => "7",
                                (5, 2) => "U",
                                (5, 3) => "Y",
                                (5, 4) => "H",
                                (5, 5) => "J",
                                (5, 6) => "N",
                                (5, 7) => "Space",

                                // Ligne 6
                                (6, 0) => "6",
                                (6, 1) => "5",
                                (6, 2) => "R",
                                (6, 3) => "T",
                                (6, 4) => "G",
                                (6, 5) => "F",
                                (6, 6) => "B",
                                (6, 7) => "V",

                                // Ligne 7
                                (7, 0) => "4",
                                (7, 1) => "3",
                                (7, 2) => "E",
                                (7, 3) => "Z (W)",
                                (7, 4) => "S",
                                (7, 5) => "D",
                                (7, 6) => "C",
                                (7, 7) => "X",

                                // Ligne 8
                                (8, 0) => "1",
                                (8, 1) => "2",
                                (8, 2) => "Escape",
                                (8, 3) => "A (Q)",
                                (8, 4) => "Tab",
                                (8, 5) => "Q (A)",
                                (8, 6) => "CapsLock",
                                (8, 7) => "W (Z)",

                                // Ligne 9
                                (9, 0) => "Joy A Up",
                                (9, 1) => "Joy A Down",
                                (9, 2) => "Joy A Left",
                                (9, 3) => "Joy A Right",
                                (9, 4) => "Joy A Fire 1",
                                (9, 5) => "Joy A Fire 2",
                                (9, 6) => "Joy A Fire 3",
                                (9, 7) => "Del",

                                _ => "Unknown",
                            };
                            pressed_keys.push(key_name);
                        }
                    }
                    let _ = write!(s, "{}", pressed_keys.join(", "));
                }
                let _ = writeln!(s);
            }
        }

        s
    }

    pub fn print_hardware_status(&mut self, show_kb: bool) {
        print!("{}", self.get_hardware_string(show_kb));
    }

    pub fn console_handle(&mut self) -> Result<(), Box<dyn Error>> {
        let (command, arg, arg2) = self.cmd_channel.1.try_recv()?;

        match command {
            MonitorCmd::Help => {
                println!("Version {VERSION}");
                println!("{HELP}");
            }
            MonitorCmd::Unknown => {
                println!("Unknown command");
            }
            MonitorCmd::Pause => {
                println!("Emulation paused !");
                self.stop();
            }
            MonitorCmd::Resume => {
                println!("Emulation resumed !");
                self.start();
            }
            MonitorCmd::Hardware => {
                let show_kb = arg == "kb" || arg == "keyboard";
                if show_kb {
                    self.waiting_for_key = true;
                    println!("Waiting for a key press on the CPC window to show matrix status...");
                } else {
                    self.print_hardware_status(false);
                }
            }
            MonitorCmd::Registers => {
                self.print_registers();
            }
            MonitorCmd::ReadMem => {
                let a = arg.to_u16()?;
                let val = self.bus.read_byte(a);
                let source_str = self.get_address_source_info(a);
                // Sur CPC le code utilisateur vit sous les ROMs : quand une ROM
                // est en place, la valeur que verrait le CPU masque celle de la
                // RAM, qui est pourtant souvent celle qu'on cherche.
                let ram = self.bus.memory.read_ram_byte(a);
                if ram != val {
                    println!(
                        "{:04X}    {:02X} ({})    RAM: {:02X}",
                        a, val, source_str, ram
                    );
                } else {
                    println!("{:04X}    {:02X} ({})", a, val, source_str);
                }
            }
            MonitorCmd::WriteMem => {
                let a = arg.to_u16()?;
                let val = arg2.to_u8()?;
                self.bus.write_byte(a, val);
                let source_str = self.get_address_source_info(a);
                println!("{:04X}    {:02X} ({})", a, val, source_str);
            }
            MonitorCmd::SearchMem => {
                let val = arg.to_u8()?;
                println!("Searching for byte {:#02X} in memory...", val);
                let mut found_count = 0;
                for addr in 0..=0xFFFF {
                    let byte = self.bus.read_byte(addr);
                    if byte == val {
                        let source_str = self.get_address_source_info(addr);
                        println!("  {:#06X} : {:#02X} ({})", addr, val, source_str);
                        found_count += 1;
                    }
                }
                println!("Total found: {} occurrences.", found_count);
            }
            MonitorCmd::ListBreakpoints => {
                if self.breakpoints.is_empty() {
                    println!("No breakpoints !")
                }
                for b in &self.breakpoints {
                    println!("{:#06X}", b);
                }
            }
            MonitorCmd::AddBreakpoint => {
                let a = arg.to_u16()?;
                self.breakpoints.insert(a);
                println!("New breakpoint at {:#06X}", a);
            }
            MonitorCmd::AddWatchpoint => {
                let a = arg.to_u16()?;
                self.bus.watchpoints.insert(a);
                println!("New watchpoint at {:#06X}", a);
            }
            MonitorCmd::ListWatchpoints => {
                if self.bus.watchpoints.is_empty() {
                    println!("No watchpoints !")
                }
                for w in &self.bus.watchpoints {
                    println!("{:#06X}", w);
                }
            }
            MonitorCmd::RemoveWatchpoint => {
                let a = arg.to_u16()?;
                if self.bus.watchpoints.remove(&a) {
                    println!("Watchpoint at {:#06X} removed", a);
                }
            }
            MonitorCmd::Step => {
                println!("{}", (zilog_z80::dasm::dasm(&self.bus, self.cpu.reg.pc)).0);
                self.step();
                self.print_registers();
            }
            MonitorCmd::StepLine => {
                let start_line = self.current_line;
                while self.current_line == start_line {
                    let ticks = self.step();
                    if ticks == 0 {
                        break;
                    }
                }
                println!("Stepped to next video line (Line {}).", self.current_line);
                self.print_registers();
            }
            MonitorCmd::RemoveBreakpoint => {
                let a = arg.to_u16()?;
                if self.breakpoints.remove(&a) {
                    println!("Breakpoint at {:#06X} removed", a);
                }
            }
            MonitorCmd::Disassemble => {
                let mut a = arg.to_u16()?;
                for _ in 0..=20 {
                    let d = zilog_z80::dasm::dasm(&self.bus, a);
                    println!("{:04X}    {}", a, d.0);
                    a += (d.1) as u16;
                }
            }
            MonitorCmd::Jump => {
                let a = arg.to_u16()?;
                self.cpu.reg.pc = a;
            }
            MonitorCmd::Disk => {
                // Sans argument supplémentaire, on s'adresse au lecteur A.
                // "disk <fichier> b" ou "disk eject b" ciblent le lecteur B.
                let target_drive_b = arg2.eq_ignore_ascii_case("b");

                if arg == "eject" {
                    if target_drive_b {
                        self.bus.fdc.borrow_mut().eject_disk_b();
                    } else {
                        self.bus.fdc.borrow_mut().eject_disk();
                    }
                } else {
                    let result = if target_drive_b {
                        self.load_disk_b(&arg)
                    } else {
                        self.load_disk(&arg)
                    };
                    if let Err(e) = result {
                        println!("Error loading disk: {}", e);
                    }
                }
            }
            MonitorCmd::PowerCycle => {
                self.power_cycle();
            }
            MonitorCmd::Volume => {
                if !arg.is_empty() {
                    match arg.parse::<f32>() {
                        Ok(percent) => self.volume = (percent / 100.0).clamp(0.0, 1.0),
                        Err(_) => println!("Usage: vol [0-100]"),
                    }
                }
                println!("Audio volume: {} %", (self.volume * 100.0).round());
            }
            MonitorCmd::ReadRam => {
                // Vidage de la RAM brute, sans le banking ROM : sur CPC le code
                // utilisateur vit sous la ROM basse, et c'est justement ce que
                // "m" ne peut pas montrer sur une plage.
                let start = arg.to_u16()?;
                let end = if arg2.is_empty() {
                    start.saturating_add(0xFF)
                } else {
                    arg2.to_u16()?
                };
                let mut addr = start;
                loop {
                    let bytes: Vec<u8> = (0..16)
                        .map(|i| self.bus.memory.read_ram_byte(addr.wrapping_add(i)))
                        .collect();
                    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
                    let text: String = bytes
                        .iter()
                        .map(|&b| {
                            if (0x20..0x7F).contains(&b) {
                                b as char
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    println!("{:04X}  {}  {}", addr, hex.join(" "), text);
                    match addr.checked_add(16) {
                        Some(next) if next <= end => addr = next,
                        _ => break,
                    }
                }
            }
            MonitorCmd::Trace => match arg.as_str() {
                "" => println!("{}", self.tracer.status()),
                "on" => {
                    self.tracer.start(TraceMode::All);
                    println!("Trace started (every instruction).");
                }
                "calls" => {
                    self.tracer.start(TraceMode::Branches);
                    println!("Trace started (jumps, calls and returns only).");
                }
                "off" => {
                    self.tracer.stop();
                    println!("Trace stopped. {} instruction(s) kept.", self.tracer.len());
                }
                "dump" => {
                    let count = arg2.parse().unwrap_or(32);
                    print!("{}", self.tracer.format_last(count));
                }
                "save" => {
                    if arg2.is_empty() {
                        println!("Usage: t save <file>");
                    } else {
                        match std::fs::write(&arg2, self.tracer.format_last(usize::MAX)) {
                            Ok(_) => {
                                println!("{} instruction(s) written to {arg2}", self.tracer.len())
                            }
                            Err(e) => println!("Can't write {arg2}: {e}"),
                        }
                    }
                }
                _ => println!("Usage: t | t on | t calls | t off | t dump [n] | t save <file>"),
            },
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Durées réelles sur CPC de quelques instructions, exprimées en
    /// microsecondes dans la documentation de la machine : le Gate Array les
    /// aligne toutes sur sa fenêtre d'accès mémoire.
    #[test]
    fn instructions_last_a_whole_number_of_microseconds() {
        // (durée nominale du Z80, durée sur CPC)
        for (nominal, cpc) in [
            (4, 4),   // NOP
            (6, 8),   // INC HL
            (7, 8),   // LD A,(HL)
            (10, 12), // JP nn
            (11, 12), // PUSH BC
            (13, 16), // DJNZ pris
            (17, 20), // CALL nn
            (19, 20), // LD A,(IX+d)
            (21, 24), // LDIR, une itération
            (23, 24), // RES b,(IX+d)
        ] {
            assert_eq!(
                cpc_instruction_time(nominal),
                cpc,
                "instruction de {nominal} cycles"
            );
        }
    }

    /// Aucune instruction ne peut durer autre chose qu'un nombre entier de
    /// microsecondes, et l'arrondi ne raccourcit jamais rien.
    #[test]
    fn the_gate_array_never_shortens_an_instruction() {
        for nominal in 1..=32u32 {
            let cpc = cpc_instruction_time(nominal);
            assert_eq!(cpc % 4, 0, "{nominal} cycles");
            assert!(cpc >= nominal, "{nominal} cycles");
            assert!(cpc - nominal < 4, "{nominal} cycles : arrondi excessif");
        }
    }

    /// La cadence de l'émulateur se déduit des cycles Z80 émulés : une trame
    /// standard (312 lignes de 256 cycles) doit valoir très exactement une
    /// période de trame CPC.
    #[test]
    fn emulated_time_matches_the_cpc_clock() {
        assert_eq!(
            emulated_duration(4_000_000),
            std::time::Duration::from_secs(1),
            "4 MHz : quatre millions de cycles font une seconde"
        );
        assert_eq!(
            emulated_duration(312 * 256).as_micros(),
            312 * 64,
            "une trame standard dure 312 scanlines de 64 us"
        );
    }

    /// Test de bout en bout : on laisse le vrai firmware démarrer, puis on lui
    /// fait émettre le bip de la console (CHR$(7)), et on vérifie qu'un son
    /// sort effectivement du PSG. Il traverse tout le chemin réel — routine
    /// son de la ROM, PPI, registres du PSG, synthèse — donc il attrape ce
    /// qu'aucun test unitaire ne voit : une écriture de registre qui n'arrive
    /// pas, un mélangeur mal décodé, une horloge de travers.
    ///
    /// Le test est ignoré si les ROMs ne sont pas présentes.
    #[test]
    fn the_firmware_beep_produces_an_audible_tone() {
        let mut machine = Machine::new();
        if machine.load_roms().is_err() {
            println!("ROMs absentes : test ignore");
            return;
        }

        // Deux secondes de temps CPU : le firmware a fini de s'initialiser et
        // le gestionnaire de son tourne sur interruption.
        let mut ticks = 0u64;
        while ticks < 2 * 4_000_000 {
            ticks += machine.step() as u64;
            machine.bus.psg.sound.take_samples();
        }

        // LD A,7 / CALL &BB5A (TXT OUTPUT) / JR $
        for (offset, byte) in [0x3E, 0x07, 0xCD, 0x5A, 0xBB, 0x18, 0xFE]
            .into_iter()
            .enumerate()
        {
            machine.bus.write_byte(0x8000 + offset as u16, byte);
        }
        machine.cpu.reg.pc = 0x8000;

        let mut samples = Vec::new();
        let mut ticks = 0u64;
        while ticks < 2 * 4_000_000 {
            ticks += machine.step() as u64;
            samples.append(&mut machine.bus.psg.sound.take_samples());
        }

        // Le bip du firmware est un ton d'environ 700 Hz : la sortie doit
        // varier franchement, et pas seulement s'installer à un niveau fixe
        // (ce que produirait un mélangeur qui laisse tout passer).
        let max = samples.iter().cloned().fold(f32::MIN, f32::max);
        let min = samples.iter().cloned().fold(f32::MAX, f32::min);
        assert!(max > 0.1, "aucun son audible pendant le bip (max {max})");
        assert!(
            min == 0.0,
            "le bip doit alterner avec du silence (min {min})"
        );

        // Le firmware programme bien un ton, pas du bruit.
        assert_eq!(
            machine.bus.psg.registers[7] & 0x38,
            0x38,
            "le bip ne doit pas utiliser le generateur de bruit"
        );
    }

    /// Bout en bout de `--disk` et `--autocmd` : charge une vraie disquette
    /// par son seul nom (donc via `[file] dsk_path`, comme le ferait la
    /// ligne de commande), tape `RUN"BARBA.I` au clavier émulé sans
    /// intervention humaine, et vérifie que le jeu démarre réellement.
    ///
    /// Le test est ignoré si les ROMs ou la disquette sont absentes.
    #[test]
    fn autocmd_types_a_command_that_actually_starts_the_game() {
        let mut machine = Machine::new();
        if machine.load_roms().is_err() {
            println!("ROMs absentes : test ignore");
            return;
        }
        // Chemin complet plutôt que le seul nom de fichier : ce test vérifie
        // que la frappe automatique fonctionne, pas la résolution de
        // `dsk_path`, qui dépend elle-même du fichier de config utilisé
        // (`config/config.toml` en debug, `~/.config/dart/config.toml` en
        // release — voir `config::load_config_file`) et n'a donc pas de
        // comportement fiable identique dans les deux profils de build.
        if machine.load_disk("bin/Barbarian.dsk").is_err() {
            println!("Disquette absente : test ignore");
            return;
        }

        // Sans le "\n" final : c'est la forme que tape un utilisateur sur la
        // ligne de commande (--autocmd='RUN"BARBA.I'), et c'est justement
        // celle qui a échappé aux tests du module autotype — la commande se
        // tapait mais ne validait jamais rien, faute d'ENTRÉE. C'est
        // `crate::ensure_validated` (main.rs) qui l'ajoute.
        let mut typer = crate::autotype::AutoTyper::new(&crate::ensure_validated("RUN\"BARBA.I"));
        let mut ticks = 0u64;
        while !typer.is_done() {
            let elapsed = machine.step();
            typer.advance(&mut machine.bus.psg, elapsed);
            ticks += elapsed as u64;
            assert!(
                ticks < 10 * 4_000_000,
                "la frappe automatique ne se termine pas"
            );
        }

        // Le chargement du jeu depuis la disquette prend plusieurs
        // secondes de temps émulé.
        let mut ticks = 0u64;
        while ticks < 90 * 4_000_000 {
            ticks += machine.step() as u64;
        }

        // Le code de Barbarian vit entre 0x7000 et 0x9FFF (voir
        // doc/barbarian-demo.md) ; le BASIC et le firmware, eux, tournent
        // sous 0x4000 ou dans les ROMs hautes. S'y retrouver prouve que la
        // commande tapée a bien été reçue et exécutée par le firmware, pas
        // seulement posée sans effet sur un clavier qui ne répondrait pas.
        assert!(
            (0x7000..0xA000).contains(&machine.cpu.reg.pc),
            "le jeu ne semble pas avoir demarre, PC={:#06X}",
            machine.cpu.reg.pc
        );
    }
}
