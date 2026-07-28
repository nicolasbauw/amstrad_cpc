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
    println!("=== Amstrad CPC 6128 ===");

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

    // 4. Initialisation de SDL_ttf pour le debugger
    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string())?;
    let font_path = "/usr/share/fonts/noto/NotoSansMono-Regular.ttf";
    let font = ttf_context
        .load_font(font_path, 13)
        .map_err(|e| e.to_string())?;

    let mut debug_visible = false;
    let debug_window = video_subsystem
        .window("Amstrad CPC 6128 - Debugger", 800, 750)
        .position_centered()
        .hidden()
        .resizable()
        .build()?;
    let mut debug_canvas = debug_window.into_canvas().build()?;
    let debug_window_id = debug_canvas.window().id();

    // Titre dynamique de la fenêtre en fonction du mode configuré
    let window_title = if machine.diagnostic_mode {
        "Amstrad CPC 6128 - Diag ROM"
    } else {
        "Amstrad CPC 6128 - BASIC 1.1 AZERTY"
    };

    let window = video_subsystem
        .window(window_title, 640, 400)
        .position_centered()
        .build()?;
    let mut canvas = window.into_canvas().build()?;
    let main_window_id = canvas.window().id();
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(PixelFormatEnum::RGB24, 320, 200)?;
    let mut event_pump = sdl_context.event_pump()?;

    let mut frame_buffer = [0u8; 320 * 200 * 3];
    let ticks_per_frame: u32 = 79_872;
    let mut running = true;

    while running {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    running = false;
                }
                Event::Window {
                    win_event: sdl2::event::WindowEvent::Close,
                    window_id,
                    ..
                } => {
                    if window_id == debug_window_id {
                        debug_visible = false;
                        debug_canvas.window_mut().hide();
                    } else if window_id == main_window_id {
                        running = false;
                    }
                }
                // Événements d'enfoncement de touches du clavier moderne PC
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F10),
                    keymod,
                    ..
                } => {
                    if keymod.contains(sdl2::keyboard::Mod::LSHIFTMOD)
                        || keymod.contains(sdl2::keyboard::Mod::RSHIFTMOD)
                    {
                        // Shift + F10 : Step Line
                        let start_line = machine.current_line;
                        while machine.current_line == start_line {
                            let ticks = machine.step();
                            if ticks == 0 {
                                break;
                            }
                        }
                        println!(
                            "Stepped to next video line (Line {}).",
                            machine.current_line
                        );
                        machine.print_registers();
                    } else {
                        // F10 : Step CPU
                        println!(
                            "{}",
                            (zilog_z80::dasm::dasm(&machine.bus, machine.cpu.reg.pc)).0
                        );
                        machine.step();
                        machine.print_registers();
                    }
                }
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F12),
                    ..
                } => {
                    debug_visible = !debug_visible;
                    if debug_visible {
                        debug_canvas.window_mut().show();
                    } else {
                        debug_canvas.window_mut().hide();
                    }
                }
                Event::KeyDown {
                    keycode: Some(key),
                    //scancode,
                    ..
                } => {
                    //println!("SDL Key Pressed: {:?} (Scancode: {:?})", key, scancode); // DEBUG
                    machine.bus.psg.set_key_state(key, true);
                    if machine.waiting_for_key {
                        machine.print_hardware_status(true);
                        machine.waiting_for_key = false;
                    }
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

        while frame_ticks < ticks_per_frame {
            if machine.is_running() {
                frame_ticks += machine.step()
            } else {
                break;
            }
        }

        // Appel au module vidéo déporté pour le rendu VRAM
        video::render(&machine, &mut frame_buffer);

        let _ = texture.update(None, &frame_buffer, 320 * 3);
        let _ = canvas.clear();
        let _ = canvas.copy(&texture, None, None);
        canvas.present();

        if debug_visible {
            // Rendu en temps réel du debugger sur la deuxième fenêtre
            debug_canvas.set_draw_color(sdl2::pixels::Color::RGB(15, 15, 25)); // Bleu nuit
            debug_canvas.clear();

            let mut debug_text = String::new();
            debug_text.push_str(&machine.get_registers_string());
            debug_text.push_str("\n");
            debug_text.push_str(&machine.get_hardware_string(true)); // Afficher la matrice clavier en tps réel !

            let texture_creator_debug = debug_canvas.texture_creator();
            let mut y = 10;
            let line_height = 16;

            for line in debug_text.lines() {
                let formatted_line = line.replace('\t', "    ");
                if !formatted_line.trim().is_empty() {
                    let surface = font
                        .render(&formatted_line)
                        .blended(sdl2::pixels::Color::RGB(220, 220, 225))
                        .map_err(|e| e.to_string())?;
                    let texture = texture_creator_debug
                        .create_texture_from_surface(&surface)
                        .map_err(|e| e.to_string())?;

                    let query = texture.query();
                    let target_rect = sdl2::rect::Rect::new(15, y, query.width, query.height);
                    let _ = debug_canvas.copy(&texture, None, Some(target_rect));
                }
                y += line_height;
            }
            debug_canvas.present();
        }

        machine.console_handle().unwrap_or_default();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    Ok(())
}
