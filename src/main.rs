mod bus;
mod console;
mod crtc;
mod gate_array;
mod hexconversion;
mod machine;
mod memory;
mod monitor;
mod ppi;
mod psg;
mod video;

use machine::Machine;
use sdl2::event::Event;
use sdl2::pixels::PixelFormatEnum;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Émulateur Amstrad CPC 6128 ===");

    // 1. Analyse des arguments de la ligne de commande pour le choix du mode
    let args: Vec<String> = env::args().collect();
    let mut diag_mode = true; // Par défaut, on démarre en mode Diagnostic

    if args.contains(&"--cpc".to_string()) || args.contains(&"--basic".to_string()) {
        diag_mode = false;
    }

    // 2. Initialisation de la Machine
    let mut machine = Machine::new();
    machine.diagnostic_mode = diag_mode;
    machine.load_roms()?;

    // 3. Initialisation de SDL2
    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;

    // Titre dynamique de la fenêtre en fonction du mode configuré
    let window_title = if machine.diagnostic_mode {
        "Amstrad CPC 6128 - Noël Llopis Diagnostic"
    } else {
        "Amstrad CPC 6128 - BASIC 1.1 AZERTY"
    };

    let window = video_subsystem
        .window(window_title, 640, 400)
        .position_centered()
        .build()?;
    let mut canvas = window.into_canvas().build()?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(PixelFormatEnum::RGB24, 320, 200)?;
    let mut event_pump = sdl_context.event_pump()?;

    let mut frame_buffer = [0u8; 320 * 200 * 3];
    let ticks_per_frame: u32 = 79_872;
    let mut running = true;
    let mut frame_count: u64 = 0;

    while running {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    running = false;
                }
                // Événements d'enfoncement de touches du clavier moderne PC
                Event::KeyDown {
                    keycode: Some(key), ..
                } => {
                    machine.bus.psg.set_key_state(key, true);
                }
                // Événements de relâchement de touches
                Event::KeyUp {
                    keycode: Some(key), ..
                } => {
                    machine.bus.psg.set_key_state(key, false);
                }
                _ => {}
            }
        }

        let mut frame_ticks: u32 = 0;
        if machine.is_running() {
            while frame_ticks < ticks_per_frame {
                frame_ticks += machine.step();
            }
        }
        // Appel au module vidéo déporté pour le rendu VRAM
        video::render(&machine, &mut frame_buffer);

        let _ = texture.update(None, &frame_buffer, 320 * 3);
        let _ = canvas.clear();
        let _ = canvas.copy(&texture, None, None);
        canvas.present();

        frame_count += 1;
        /*if frame_count % 150 == 0 {
            println!(
                "Statistiques : {} frames affichées (Total Ticks: {}, Ligne: {})",
                frame_count, machine.total_ticks, machine.current_line
            );
        }*/
        machine.console_handle().unwrap_or_default();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    Ok(())
}
