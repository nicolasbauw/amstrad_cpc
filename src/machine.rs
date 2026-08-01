use crate::bus::CpcBus;
use crate::config;
use crate::hexconversion::HexStringToUnsigned;
use crate::memory::Memory;
use crate::monitor::MonitorCmd;
use std::{
    collections::HashSet, error, error::Error, fmt, fs::File, io::Read, sync::mpsc,
    sync::mpsc::SendError,
};
use zilog_z80::{bus::Bus, cpu::CPU};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = "
Emulator commands:
    disk d.dsk      Loads the d.dsk disk image on drive A
    disk d.dsk b    Loads the d.dsk disk image on drive B (if enabled in config.toml)
    disk eject      Ejects the disk image from drive A
    disk eject b    Ejects the disk image from drive B

Monitor commands:
    d 0x0000        disassembles code at 0x0000 and the 20 next
                    instructions
    m 0xeeee        displays memory content at address 0xeeee
    m 0xeeee 0xaa   sets memory address 0xeeee to the 0xaa value
    s 0xaa          searches for a byte in memory
    n               steps to next Z80 instruction
    l               steps to next video line
    j 0x0000        jumps to 0x0000 address
    b               displays set breakpoints
    b 0x0002        sets a breakpoint at address 0x0002
    f 0x0002        \"frees\" (deletes) breakpoint at address 0x0002
    w               displays set watchpoints
    w 0xeeee        adds a write watchpoint at address 0xeeee
    fw 0xeeee       removes watchpoint at address 0xeeee
    p               pause execution
    g               resume execution after the \"p\" command, or a breakpoint,
                    has been used to halt execution
    hw              displays Gate Array and CRTC status
    hw kb           keyboard test
    r               displays the contents of flags, registers and interrupts";

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
    pub diagnostic_mode: bool, // true = ROM de Diagnostic, false = ROMs d'origine du CPC 6128
    cmd_channel: (
        mpsc::Sender<(MonitorCmd, String, String)>,
        mpsc::Receiver<(MonitorCmd, String, String)>,
    ),
    breakpoints: HashSet<u16>,
    running: bool,
    stopped_at_breakpoint: bool,
    pub waiting_for_key: bool,
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
                debugger: config::Debugger { keyboard: false },
            }
        });

        let m = Self {
            cpu,
            bus,
            total_ticks: 0,
            hsync_accumulator: 0,
            current_line: 0,
            diagnostic_mode: false, // Basculé à false pour tester le boot officiel du CPC 6128 !
            cmd_channel: mpsc::channel(),
            breakpoints: HashSet::new(),
            running: true,
            stopped_at_breakpoint: false,
            waiting_for_key: false,
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

    pub fn start(&mut self) {
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn is_running(&mut self) -> bool {
        self.running
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
        if current_pc == 0x0038 {
            self.bus.gate_array.interrupt_requested = false;
        }

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
        let ticks = self.cpu.execute(&mut self.bus);

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

        let elapsed_ticks = if ticks == 0 { 4 } else { ticks };
        self.total_ticks += elapsed_ticks as u64;

        // Gestion HSYNC / Interruptions (période 256 ticks = 64µs)
        self.hsync_accumulator += elapsed_ticks;
        while self.hsync_accumulator >= 256 {
            self.hsync_accumulator -= 256;
            self.current_line = (self.current_line + 1) % 312;

            // VSYNC actif entre les lignes 280 et 284
            let vsync = self.current_line >= 280 && self.current_line < 284;
            // On force le bit 1 à 1 pour lire Joystick A par défaut
            self.bus.ppi.set_system_port_b(vsync, true);

            if self.bus.gate_array.step_hsync() {
                self.cpu.int_request(0xFF);
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
            && self.bus.memory.rom_high_present[self.bus.memory.selected_high_rom as usize]
        {
            format!("ROM High {}", self.bus.memory.selected_high_rom)
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

        // --- FDC / LECTEURS DE DISQUETTES ---
        {
            let fdc = self.bus.fdc.borrow();
            let _ = writeln!(s, "\n[FDC]");
            let _ = writeln!(s, "  Motor On           : {}", fdc.motor_on);
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
                println!("{:04X}    {:02X} ({})", a, val, source_str);
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
                        self.bus.fdc.borrow_mut().load_disk_b(&arg)
                    } else {
                        self.bus.fdc.borrow_mut().load_disk(&arg)
                    };
                    match result {
                        Ok(_) => println!(
                            "Disk '{}' loaded on drive {}",
                            arg,
                            if target_drive_b { "B" } else { "A" }
                        ),
                        Err(e) => println!("Error loading disk: {}", e),
                    }
                }
            }
        }
        Ok(())
    }
}
