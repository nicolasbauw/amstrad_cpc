use crate::bus::CpcBus;
use crate::memory::Memory;
use std::fs::File;
use std::io::Read;
use zilog_z80::cpu::CPU;

pub struct Machine {
    pub cpu: CPU,
    pub bus: CpcBus,
    pub total_ticks: u64,
    pub hsync_accumulator: u32,
    pub current_line: u32,
}

impl Machine {
    pub fn new() -> Self {
        let memory = Memory::new();
        let bus = CpcBus::new(memory);
        let cpu = CPU::new();

        Self {
            cpu,
            bus,
            total_ticks: 0,
            hsync_accumulator: 0,
            current_line: 0,
        }
    }

    /// Charge les ROMs depuis le répertoire bin/
    pub fn load_roms(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // ROM Basse
        let mut f = File::open("bin/AmstradDiagLower.rom")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        self.bus.memory.load_low_rom(&buf);

        // ROM Haute 0 (Diagnostic)
        let mut f = File::open("bin/AmstradDiagUpper.rom")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        self.bus.memory.load_high_rom(0, &buf);

        Ok(())
    }

    /// Exécute une instruction et synchronise les périphériques
    pub fn step(&mut self) -> u32 {
        let current_pc = self.cpu.reg.pc;
        if current_pc == 0x0038 {
            self.bus.gate_array.interrupt_requested = false;
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
}
