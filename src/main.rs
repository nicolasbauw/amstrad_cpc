mod bus;
mod memory;

use bus::CpcBus;
use memory::Memory;
use std::fs::File;
use std::io::Read;
use zilog_z80::cpu::CPU;

fn main() {
    println!("=== Émulateur Amstrad CPC - Initialisation ===");

    // 1. Initialisation de la mémoire et chargement de la ROM de diagnostic
    let mut memory = Memory::new();

    let rom_path = "bin/AmstradDiagLower.rom";
    println!("Chargement de la ROM de diagnostic : {}...", rom_path);

    let mut file = match File::open(rom_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Erreur : Impossible d'ouvrir le fichier de la ROM '{}' : {}",
                rom_path, e
            );
            return;
        }
    };

    let mut rom_buffer = Vec::new();
    if let Err(e) = file.read_to_end(&mut rom_buffer) {
        eprintln!("Erreur lors de la lecture de la ROM : {}", e);
        return;
    }

    println!("Taille de la ROM lue : {} octets", rom_buffer.len());
    memory.load_rom(&rom_buffer);

    // 2. Initialisation du Bus et du CPU
    let mut bus = CpcBus::new(memory);
    let mut cpu = CPU::new();

    // Au boot du Z80, le compteur de programme (PC) démarre à $0000.
    // Notre ROM de diagnostic est mappée à l'adresse $0000 car rom_low_enabled est true par défaut.
    println!("CPU initialisé. Démarrage de la boucle d'exécution principale...");

    let mut total_ticks: u64 = 0;
    let mut instruction_count: u64 = 0;

    // 3. Boucle d'exécution (exécute les premières instructions pour valider le boot)
    // Pour ne pas saturer la console, on va simuler les 100 premières instructions.
    while instruction_count < 100 {
        // Enregistre l'adresse actuelle pour l'affichage avant exécution
        let current_pc = cpu.reg.pc;

        // Exécution de l'instruction et récupération des cycles d'horloge (ticks)
        let ticks = cpu.execute(&mut bus);
        total_ticks += ticks as u64;
        instruction_count += 1;

        println!(
            "[{:05}] PC: 0x{:04X} | Instruction exécutée en {} ticks (Total: {} ticks)",
            instruction_count, current_pc, ticks, total_ticks
        );
    }

    println!("====================================================");
    println!("Exécution des 100 premières instructions terminée avec succès !");
    println!("Total ticks machine : {}", total_ticks);
}
