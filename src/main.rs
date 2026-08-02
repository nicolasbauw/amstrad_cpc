mod bus;
mod config;
mod console;
mod crtc;
mod fdc;
mod gate_array;
mod hexconversion;
mod machine;
mod memory;
mod monitor;
mod ppi;
mod psg;
mod trace;
mod video;

use machine::Machine;
use sdl2::event::Event;
use sdl2::pixels::PixelFormatEnum;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Amstrad CPC 6128 ===");

    // 1. Analyse des arguments de la ligne de commande pour le choix du mode
    let args: Vec<String> = env::args().collect();
    let mut diag_mode = false; // Par défaut, on démarre en mode normal

    if args.contains(&"--diag".to_string()) || args.contains(&"-d".to_string()) {
        diag_mode = true;
    }

    // 2. Initialisation de la Machine
    let mut machine = Machine::new();
    machine.diagnostic_mode = diag_mode;
    machine.load_roms()?;

    // 3. Initialisation de SDL2
    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;
    let controller_subsystem = sdl_context.game_controller()?;

    // Tentative d'ouverture de la première manette disponible
    let num_joysticks = controller_subsystem.num_joysticks().unwrap_or(0);
    let mut _active_controller = None;
    for i in 0..num_joysticks {
        if controller_subsystem.is_game_controller(i) {
            match controller_subsystem.open(i) {
                Ok(c) => {
                    println!("Controller opened: {}", c.name());
                    _active_controller = Some(c);
                    break;
                }
                Err(e) => println!("Failed to open controller {}: {}", i, e),
            }
        }
    }

    // 4. Initialisation de SDL_ttf pour le debugger
    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string())?;
    let home_dir = std::env::var("HOME")?;
    let font_path = if cfg!(target_os = "macos") {
        home_dir + "/Library/Fonts/NotoSansMono-Regular.ttf"
    } else {
        "/usr/share/fonts/noto/NotoSansMono-Regular.ttf".to_string()
    };

    let font = ttf_context
        .load_font(font_path, 13)
        .map_err(|e| e.to_string())?;

    let mut debug_visible = false;
    let debug_window = video_subsystem
        .window("Amstrad CPC 6128 - Machine Status", 800, 750)
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
        .window(
            window_title,
            video::SCREEN_WIDTH as u32,
            video::SCREEN_HEIGHT as u32,
        )
        .position_centered()
        .build()?;
    let mut canvas = window.into_canvas().build()?;
    let main_window_id = canvas.window().id();
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(
        PixelFormatEnum::RGB24,
        video::SCREEN_WIDTH as u32,
        video::SCREEN_HEIGHT as u32,
    )?;
    let mut event_pump = sdl_context.event_pump()?;

    let mut frame_buffer = vec![0u8; video::SCREEN_WIDTH * video::SCREEN_HEIGHT * 3];
    // Deux fois la durée d'une trame standard (312 lignes de 256 ticks) : simple
    // garde-fou pour ne pas bloquer la boucle SDL si le VSYNC n'arrive jamais.
    let max_ticks_per_frame: u32 = 2 * 79_872;
    let mut running = true;

    // Cadence d'exécution. Une scanline dure 64 µs sur le CPC, donc la durée d'une
    // trame se déduit du nombre de scanlines programmé dans le CRTC : un logiciel
    // qui reprogramme R4/R9 change réellement la fréquence trame, comme sur le
    // matériel. On vise une échéance absolue plutôt que de dormir une durée fixe :
    // avec une attente fixe, la période réelle vaut "temps de calcul + attente" et
    // la machine émulée tourne durablement trop lentement.
    const SCANLINE_MICROS: u64 = 64;
    let mut next_frame = std::time::Instant::now();

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
                    keycode: Some(kc),
                    scancode: Some(sc),
                    ..
                } => {
                    if !machine.bus.psg.set_key_state_scancode(sc, true) {
                        machine.bus.psg.set_key_state(kc, true);
                    }
                }
                Event::KeyUp {
                    keycode: Some(kc),
                    scancode: Some(sc),
                    ..
                } => {
                    if !machine.bus.psg.set_key_state_scancode(sc, false) {
                        machine.bus.psg.set_key_state(kc, false);
                    }
                }
                Event::ControllerButtonDown { button, .. } => {
                    let btn_idx = match button {
                        sdl2::controller::Button::DPadUp => 0,
                        sdl2::controller::Button::DPadDown => 1,
                        sdl2::controller::Button::DPadLeft => 2,
                        sdl2::controller::Button::DPadRight => 3,
                        sdl2::controller::Button::A => 4, // Fire 1
                        sdl2::controller::Button::B => 5, // Fire 2
                        sdl2::controller::Button::X | sdl2::controller::Button::Y => 6, // Fire 3
                        _ => 99,
                    };
                    if btn_idx != 99 {
                        machine.bus.psg.set_controller_button(btn_idx, true);
                    }
                }
                Event::ControllerButtonUp { button, .. } => {
                    let btn_idx = match button {
                        sdl2::controller::Button::DPadUp => 0,
                        sdl2::controller::Button::DPadDown => 1,
                        sdl2::controller::Button::DPadLeft => 2,
                        sdl2::controller::Button::DPadRight => 3,
                        sdl2::controller::Button::A => 4, // Fire 1
                        sdl2::controller::Button::B => 5, // Fire 2
                        sdl2::controller::Button::X | sdl2::controller::Button::Y => 6, // Fire 3
                        _ => 99,
                    };
                    if btn_idx != 99 {
                        machine.bus.psg.set_controller_button(btn_idx, false);
                    }
                }
                Event::ControllerAxisMotion { axis, value, .. } => {
                    let threshold = 10000;
                    match axis {
                        sdl2::controller::Axis::LeftX => {
                            if value > threshold {
                                machine.bus.psg.set_controller_button(3, true); // Right
                                machine.bus.psg.set_controller_button(2, false);
                            } else if value < -threshold {
                                machine.bus.psg.set_controller_button(2, true); // Left
                                machine.bus.psg.set_controller_button(3, false);
                            } else {
                                machine.bus.psg.set_controller_button(2, false);
                                machine.bus.psg.set_controller_button(3, false);
                            }
                        }
                        sdl2::controller::Axis::LeftY => {
                            if value > threshold {
                                machine.bus.psg.set_controller_button(1, true); // Down
                                machine.bus.psg.set_controller_button(0, false);
                            } else if value < -threshold {
                                machine.bus.psg.set_controller_button(0, true); // Up
                                machine.bus.psg.set_controller_button(1, false);
                            } else {
                                machine.bus.psg.set_controller_button(0, false);
                                machine.bus.psg.set_controller_button(1, false);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Une trame s'achève sur le VSYNC généré par le CRTC, pas sur un nombre
        // de ticks figé : un logiciel qui reprogramme R4/R9 change la durée de
        // trame, et le rendu doit suivre. La borne en ticks reste comme garde-fou
        // si le CRTC est programmé sans jamais produire de VSYNC.
        machine.frame_ready = false;
        let mut frame_ticks: u32 = 0;

        while machine.is_running() && !machine.frame_ready && frame_ticks < max_ticks_per_frame {
            frame_ticks += machine.step();
        }

        // Appel au module vidéo déporté pour le rendu VRAM
        video::render(&machine, &mut frame_buffer);

        let _ = texture.update(None, &frame_buffer, video::SCREEN_WIDTH * 3);
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
            debug_text.push_str(&machine.get_hardware_string(machine.show_keyboard_matrix()));

            let texture_creator_debug = debug_canvas.texture_creator();
            let mut y = 10;
            let line_height = 16;

            for line in debug_text.lines() {
                let formatted_line = line.replace('\t', "    ");
                if !formatted_line.trim().is_empty() {
                    // La ligne "Disk access" se termine par un marqueur '●' (rouge,
                    // accès disque en cours) rendu séparément du reste du texte.
                    let (text_part, dot) = match formatted_line.strip_suffix('\u{25CF}') {
                        Some(prefix) => (prefix, true),
                        None => (formatted_line.as_str(), false),
                    };

                    let mut x = 15;
                    if !text_part.is_empty() {
                        let surface = font
                            .render(text_part)
                            .blended(sdl2::pixels::Color::RGB(220, 220, 225))
                            .map_err(|e| e.to_string())?;
                        let texture = texture_creator_debug
                            .create_texture_from_surface(&surface)
                            .map_err(|e| e.to_string())?;

                        let query = texture.query();
                        let target_rect = sdl2::rect::Rect::new(x, y, query.width, query.height);
                        let _ = debug_canvas.copy(&texture, None, Some(target_rect));
                        x += query.width as i32;
                    }

                    if dot {
                        let surface = font
                            .render("\u{25CF}")
                            .blended(sdl2::pixels::Color::RGB(220, 40, 40))
                            .map_err(|e| e.to_string())?;
                        let texture = texture_creator_debug
                            .create_texture_from_surface(&surface)
                            .map_err(|e| e.to_string())?;

                        let query = texture.query();
                        let target_rect = sdl2::rect::Rect::new(x, y, query.width, query.height);
                        let _ = debug_canvas.copy(&texture, None, Some(target_rect));
                    }
                }
                y += line_height;
            }
            debug_canvas.present();
        }

        machine.console_handle().unwrap_or_default();

        let frame_duration = std::time::Duration::from_micros(
            machine.bus.crtc.frame_scanlines() as u64 * SCANLINE_MICROS,
        );
        let now = std::time::Instant::now();
        if now < next_frame {
            std::thread::sleep(next_frame - now);
            next_frame += frame_duration;
        } else {
            // Trame en retard : on repart de l'instant courant plutôt que de
            // tenter de rattraper, ce qui emballerait la machine émulée.
            next_frame = now + frame_duration;
        }
    }

    Ok(())
}
