use crate::bus::CpcBus;
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
    p               pause execution
    g               resume execution after the \"p\" command, or a breakpoint,
                    has been used to halt execution
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
}

impl Machine {
    pub fn new() -> Self {
        let memory = Memory::new();
        let bus = CpcBus::new(memory);
        let cpu = CPU::new();

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
        };
        crate::console::launch(m.cmd_channel.0.clone()).unwrap();
        m
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
        if self.diagnostic_mode {
            println!("Configuration : Chargement des ROMs de Diagnostic...");

            // ROM Basse de Diagnostic
            let mut f = File::open("bin/AmstradDiagLower.rom")?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            self.bus.memory.load_low_rom(&buf);

            // ROM Haute 0 (Diagnostic Upper)
            let mut f = File::open("bin/AmstradDiagUpper.rom")?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            self.bus.memory.load_high_rom(0, &buf);
        } else {
            println!("Configuration : Chargement des ROMs d'origine Amstrad CPC 6128 (AZERTY)...");

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
        }

        Ok(())
    }

    /// Exécute une instruction et synchronise les périphériques
    pub fn step(&mut self) -> u32 {
        let current_pc = self.cpu.reg.pc;
        if current_pc == 0x0038 {
            self.bus.gate_array.interrupt_requested = false;
        }

        if self.breakpoints.contains(&self.cpu.reg.pc) {
            self.stop();
            //return 0;
        }

        let ticks = self.cpu.execute(&mut self.bus);
        let elapsed_ticks = if ticks == 0 { 4 } else { ticks };
        self.total_ticks += elapsed_ticks as u64;

        // Gestion HSYNC / Interruptions (période 256 ticks = 64µs)
        self.hsync_accumulator += elapsed_ticks;
        while self.hsync_accumulator >= 256 {
            self.hsync_accumulator -= 256;
            self.current_line = (self.current_line + 1) % 312;

            // VSYNC actif entre les lignes 280 et 284
            let vsync = self.current_line >= 280 && self.current_line < 284;
            self.bus.ppi.set_vsync(vsync);

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
            MonitorCmd::Registers => {
                print!(
                    "PC :{:#06X}\tSP : {:#06X}\nS : {}\tZ : {}\tH : {}\tP : {}\tN : {}\tC : {}\nB : {:#04X}\tC : {:#04X}\nD : {:#04X}\tE : {:#04X}\nH : {:#04X}\tL : {:#04X}\nA : {:#04X}\t(SP) : {:#06X}\nIFF1 : {}\tIFF2 : {}\tIM : {}\nPending INT : {}\tPending NMI : {}\n",
                    self.cpu.reg.pc,
                    self.cpu.reg.sp,
                    self.cpu.reg.flags.s as i32,
                    self.cpu.reg.flags.z as i32,
                    self.cpu.reg.flags.h as i32,
                    self.cpu.reg.flags.p as i32,
                    self.cpu.reg.flags.n as i32,
                    self.cpu.reg.flags.c as i32,
                    self.cpu.reg.b,
                    self.cpu.reg.c,
                    self.cpu.reg.d,
                    self.cpu.reg.e,
                    self.cpu.reg.h,
                    self.cpu.reg.l,
                    self.cpu.reg.a,
                    self.bus.read_word(self.cpu.reg.sp),
                    self.cpu.iff1(),
                    self.cpu.iff2(),
                    self.cpu.im(),
                    self.cpu.has_pending_int(),
                    self.cpu.has_pending_nmi()
                );
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
            MonitorCmd::RemoveBreakpoint => {
                let a = arg.to_u16()?;
                if self.breakpoints.remove(&a) {
                    println!("Breakpoint at {:#06X} removed", a);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
