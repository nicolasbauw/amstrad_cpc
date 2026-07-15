mod bus;
mod crtc;
mod gate_array;
mod memory;
mod ppi;
mod psg;

use bus::CpcBus;
use memory::Memory;
use std::fs::File;
use std::io::Read;
use zilog_z80::cpu::CPU;

// Importations SDL2
use sdl2::event::Event;
use sdl2::pixels::PixelFormatEnum;

/// Décode la VRAM en Mode 1 (320x200, 4 couleurs) et écrit le résultat dans le buffer RGB.
fn render_vram_mode1(bus: &CpcBus, frame_buffer: &mut [u8; 320 * 200 * 3]) {
    for char_y in 0..25 {
        for pixel_y in 0..8 {
            let line_y = char_y * 8 + pixel_y;
            let base_addr = 0xC000 + (char_y * 80) + (pixel_y * 2048);

            for x_bytes in 0..80 {
                let addr = (base_addr + x_bytes) as u16;
                let byte = bus.memory.read_byte(addr);

                let p0 = ((byte & 0x80) >> 7) | ((byte & 0x08) >> 2);
                let p1 = ((byte & 0x40) >> 6) | ((byte & 0x04) >> 1);
                let p2 = ((byte & 0x20) >> 5) | ((byte & 0x02) << 0);
                let p3 = ((byte & 0x10) >> 4) | ((byte & 0x01) << 1);

                let pixels = [p0, p1, p2, p3];

                for i in 0..4 {
                    let pixel_x = x_bytes * 4 + i;
                    let color_index = pixels[i] as usize;

                    let (r, g, b) = bus.gate_array.get_rgb_color(color_index);

                    let offset = (line_y * 320 + pixel_x) * 3;
                    if offset + 2 < frame_buffer.len() {
                        frame_buffer[offset] = r;
                        frame_buffer[offset + 1] = g;
                        frame_buffer[offset + 2] = b;
                    }
                }
            }
        }
    }
}

fn main() {
    println!("=== Émulateur Amstrad CPC 6128 - Noël Llopis Diagnostic ===");

    let mut memory = Memory::new();

    // 1. Chargement de la ROM basse (Diagnostic Lower)
    let low_rom_path = "bin/AmstradDiagLower.rom";
    let mut low_rom_file = match File::open(low_rom_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Erreur : Impossible d'ouvrir la ROM basse : {}", e);
            return;
        }
    };
    let mut low_rom_buffer = Vec::new();
    let _ = low_rom_file.read_to_end(&mut low_rom_buffer);
    memory.load_low_rom(&low_rom_buffer);

    // 2. Chargement de la ROM haute (Diagnostic Upper)
    let high_rom_path = "bin/AmstradDiagUpper.rom";
    let mut high_rom_file = match File::open(high_rom_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Erreur : Impossible d'ouvrir la ROM haute : {}", e);
            return;
        }
    };
    let mut high_rom_buffer = Vec::new();
    let _ = high_rom_file.read_to_end(&mut high_rom_buffer);
    memory.load_high_rom(0, &high_rom_buffer);

    // 3. Initialisation de SDL2
    let sdl_context = match sdl2::init() {
        Ok(context) => context,
        Err(e) => {
            eprintln!("Erreur lors de l'initialisation de SDL2 : {}", e);
            return;
        }
    };

    let video_subsystem = match sdl_context.video() {
        Ok(sub) => sub,
        Err(e) => {
            eprintln!("Erreur lors de l'obtention du sous-système vidéo : {}", e);
            return;
        }
    };

    let window = match video_subsystem
        .window("Amstrad CPC 6128", 640, 400)
        .position_centered()
        .build()
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Erreur lors de la création de la fenêtre : {}", e);
            return;
        }
    };

    let mut canvas = match window.into_canvas().build() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Erreur lors de la création du Canvas : {}", e);
            return;
        }
    };

    let texture_creator = canvas.texture_creator();
    let mut texture =
        match texture_creator.create_texture_streaming(PixelFormatEnum::RGB24, 320, 200) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Erreur lors de la création de la texture : {}", e);
                return;
            }
        };

    let mut event_pump = match sdl_context.event_pump() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Erreur lors de l'initialisation de l'event pump : {}", e);
            return;
        }
    };

    // 4. Initialisation du Bus et du CPU
    let mut bus = CpcBus::new(memory);
    let mut cpu = CPU::new();

    println!("Initialisation terminée. Lancement de l'affichage vidéo !");

    // Notre tampon pour l'image RGB
    let mut frame_buffer = [0u8; 320 * 200 * 3];

    let mut total_ticks: u64 = 0;
    let mut hsync_accumulator: u32 = 0;
    let hsync_period_ticks: u32 = 256;

    // Compteur de ligne d'affichage (0 à 311) pour gérer le timing du VSYNC
    let mut current_line: u32 = 0;

    // Nombre de ticks de CPU par frame vidéo (environ 50 frames par seconde, 1 frame = 312 lignes * 256 ticks = 79 872 ticks)
    let ticks_per_frame: u32 = 79_872;
    let mut running = true;
    let mut frame_count: u64 = 0;

    while running {
        // Gérer les événements utilisateur
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    running = false;
                }
                _ => {}
            }
        }

        // Faire tourner le CPU pour la durée exacte d'une frame d'affichage (~20 ms)
        let mut frame_ticks: u32 = 0;
        while frame_ticks < ticks_per_frame {
            let current_pc = cpu.reg.pc;

            // Petit espion d'exécution : affichons le PC actuel de temps en temps ou s'il y a un changement
            if total_ticks % 100_000 == 0 {
                println!(
                    "Trace d'exécution -> PC: 0x{:04X} | SP: 0x{:04X} | Int Requetées: {} | RomBasse: {}",
                    current_pc,
                    cpu.reg.sp,
                    bus.gate_array.interrupt_requested,
                    bus.memory.rom_low_enabled
                );
            }

            if current_pc == 0x0038 {
                bus.gate_array.interrupt_requested = false;
            }

            let ticks = cpu.execute(&mut bus);
            let elapsed_ticks = if ticks == 0 { 4 } else { ticks };

            frame_ticks += elapsed_ticks;
            total_ticks += elapsed_ticks as u64;

            // Avancer le signal HSYNC
            hsync_accumulator += elapsed_ticks;
            while hsync_accumulator >= hsync_period_ticks {
                hsync_accumulator -= hsync_period_ticks;

                // On passe à la ligne suivante
                current_line = (current_line + 1) % 312;

                // Le signal VSYNC est levé de la ligne 280 à 284
                let vsync_active = current_line >= 280 && current_line < 284;
                bus.ppi.set_vsync(vsync_active);

                // On avance d'une ligne dans le Gate Array pour les interruptions
                if bus.gate_array.step_hsync() {
                    cpu.int_request(0xFF);
                }
            }
        }

        // Décoder la VRAM et remplir notre frame buffer
        render_vram_mode1(&bus, &mut frame_buffer);

        // Mettre à jour la texture SDL2
        let _ = texture.update(None, &frame_buffer, 320 * 3);

        // Dessiner l'image mise à l'échelle sur le Canvas
        let _ = canvas.clear();
        let _ = canvas.copy(&texture, None, None);
        canvas.present();

        frame_count += 1;
        if frame_count % 150 == 0 {
            println!(
                "Statistiques : {} frames affichées (Total Ticks: {}, Ligne courante: {})",
                frame_count, total_ticks, current_line
            );
        }

        // Limiter à ~50 images par seconde de façon fluide
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    println!("Émulateur Amstrad CPC arrêté proprement. Merci d'avoir joué !");
}
