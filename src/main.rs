mod bus;
mod crtc;
mod gate_array;
mod memory;

use bus::CpcBus;
use memory::Memory;
use std::fs::File;
use std::io::Read;
use zilog_z80::cpu::CPU;

fn main() {
    println!("=== Émulateur Amstrad CPC - Étape 3 : Routage I/O (Gate Array & CRTC) ===");

    let mut memory = Memory::new();

    // 1. Chargement de la ROM basse (Diagnostic Lower)
    let low_rom_path = "bin/AmstradDiagLower.rom";
    println!("Chargement de la ROM basse : {}...", low_rom_path);
    let mut low_rom_file = match File::open(low_rom_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Erreur : Impossible d'ouvrir la ROM basse : {}", e);
            return;
        }
    };
    let mut low_rom_buffer = Vec::new();
    if let Err(e) = low_rom_file.read_to_end(&mut low_rom_buffer) {
        eprintln!("Erreur lors de la lecture de la ROM basse : {}", e);
        return;
    }
    memory.load_low_rom(&low_rom_buffer);
    println!("ROM basse chargée ({} octets).", low_rom_buffer.len());

    // 2. Chargement de la ROM haute (Diagnostic Upper)
    let high_rom_path = "bin/AmstradDiagUpper.rom";
    println!("Chargement de la ROM haute : {}...", high_rom_path);
    let mut high_rom_file = match File::open(high_rom_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Erreur : Impossible d'ouvrir la ROM haute : {}", e);
            return;
        }
    };
    let mut high_rom_buffer = Vec::new();
    if let Err(e) = high_rom_file.read_to_end(&mut high_rom_buffer) {
        eprintln!("Erreur lors de la lecture de la ROM haute : {}", e);
        return;
    }
    memory.load_high_rom(0, &high_rom_buffer);
    println!(
        "ROM haute chargée à l'index 0 ({} octets).",
        high_rom_buffer.len()
    );

    // 3. Initialisation du Bus et du CPU
    let mut bus = CpcBus::new(memory);
    let mut cpu = CPU::new();

    println!("CPU initialisé. Démarrage de l'émulation...");

    let mut total_ticks: u64 = 0;
    let mut instruction_count: u64 = 0;

    // Pour suivre l'état de la configuration du système
    let mut last_low_enabled = bus.memory.rom_low_enabled;
    let mut last_high_enabled = bus.memory.rom_high_enabled;
    let mut last_selected_rom = bus.memory.selected_high_rom;

    // Faisons tourner l'émulateur sur 200 instructions
    while instruction_count < 200 {
        let current_pc = cpu.reg.pc;

        // Exécution d'une instruction
        let ticks = cpu.execute(&mut bus);
        total_ticks += ticks as u64;
        instruction_count += 1;

        // Détection de changements d'état du banking
        if bus.memory.rom_low_enabled != last_low_enabled
            || bus.memory.rom_high_enabled != last_high_enabled
            || bus.memory.selected_high_rom != last_selected_rom
        {
            println!(
                " >>> CHANGEMENT DE BANKING à l'instruction [{}] PC: 0x{:04X} <<<",
                instruction_count, current_pc
            );
            println!(
                "     ROM Basse : {} | ROM Haute : {} | ROM Haute Sélectionnée : #{}",
                if bus.memory.rom_low_enabled {
                    "ACTIVÉE"
                } else {
                    "DÉSACTIVÉE"
                },
                if bus.memory.rom_high_enabled {
                    "ACTIVÉE"
                } else {
                    "DÉSACTIVÉE"
                },
                bus.memory.selected_high_rom
            );
            last_low_enabled = bus.memory.rom_low_enabled;
            last_high_enabled = bus.memory.rom_high_enabled;
            last_selected_rom = bus.memory.selected_high_rom;
        }
    }

    println!("====================================================");
    println!("Exécution terminée.");
    println!("Total ticks machine : {}", total_ticks);
}
