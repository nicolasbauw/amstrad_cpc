//! Fenêtrage SDL2 : ouverture des fenêtres (écran principal et statut
//! machine), boucle d'événements (clavier, manette, raccourcis d'affichage),
//! rendu de la trame et régulation de la cadence. `main.rs` ne fait
//! qu'assembler une `Machine` prête à tourner et l'y confier via `run`.

use bytebox_core::app_log;
use bytebox_core::autotype::AutoTyper;
use crate::console_log::ConsoleLog;
use crate::console_panel::QuickCommandBar;
use crate::console_window::ConsoleWindow;
use bytebox_core::machine::{self, Machine};
use crate::renderer::Renderer;
use bytebox_core::video;
use sdl2::event::Event;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;

/// Pose `assets/croco.png` comme icône de la fenêtre donnée. Décodage en
/// pur Rust via la crate `image` (plutôt que la feature `"image"` de
/// `sdl2`, qui dépend de la bibliothèque système `libSDL2_image`) : pas de
/// dépendance supplémentaire à installer sur la machine qui compile ou qui
/// exécute l'émulateur. SDL copie les pixels de la surface dans la fenêtre
/// dès `set_icon` : la surface elle-même n'a pas besoin de survivre à cet
/// appel.
fn set_window_icon(window: &mut sdl2::video::Window) -> Result<(), String> {
    let img = image::open("assets/bytebox.png")
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (width, height) = img.dimensions();
    let mut pixels = img.into_raw();
    let pitch = width * 4;
    let surface = Surface::from_data(&mut pixels, width, height, pitch, PixelFormatEnum::RGBA32)
        .map_err(|e| e.to_string())?;
    window.set_icon(&surface);
    Ok(())
}

/// Niveau de zoom de la fenêtre d'affichage (touches F1-F4, ou
/// `default_zoom` dans config.toml).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DisplayMode {
    Normal,
    X2,
    X3,
    Fullscreen,
}

impl DisplayMode {
    /// Lit la valeur de `default_zoom` (config.toml, section [display]).
    /// Une valeur absente ou non reconnue retombe silencieusement sur la
    /// taille normale (mais journalise un avertissement si elle est
    /// présente et non reconnue : mieux vaut le signaler qu'échouer
    /// silencieusement sur une faute de frappe dans le fichier).
    fn from_config(value: Option<&str>) -> Self {
        match value {
            None => DisplayMode::Normal,
            Some("x1") => DisplayMode::Normal,
            Some("x2") => DisplayMode::X2,
            Some("x3") => DisplayMode::X3,
            Some("fullscreen") => DisplayMode::Fullscreen,
            Some(other) => {
                app_log!(
                    "Config: display.default_zoom='{other}' not recognized (expected x1, x2, x3 or fullscreen), using x1."
                );
                DisplayMode::Normal
            }
        }
    }
}

/// Applique un niveau de zoom à la fenêtre d'affichage principale.
///
/// Le letterboxing/pillarboxing qui conserve le ratio d'aspect (4:3) n'est
/// plus automatique comme au temps de `Canvas::set_logical_size` : c'est
/// désormais `Renderer::present` qui recalcule un viewport à chaque trame
/// (voir `renderer.rs`). Cette fonction ne fait donc plus que choisir la
/// taille de fenêtre ou le plein écran.
fn apply_display_mode(window: &mut sdl2::video::Window, mode: DisplayMode) {
    match mode {
        DisplayMode::Fullscreen => {
            let _ = window.set_fullscreen(sdl2::video::FullscreenType::Desktop);
            return;
        }
        DisplayMode::Normal | DisplayMode::X2 | DisplayMode::X3 => {
            let _ = window.set_fullscreen(sdl2::video::FullscreenType::Off);
        }
    }
    let factor = match mode {
        DisplayMode::Normal => 1,
        DisplayMode::X2 => 2,
        DisplayMode::X3 => 3,
        DisplayMode::Fullscreen => unreachable!("traite plus haut, avec un retour anticipe"),
    };
    let _ = window.set_size(
        video::SCREEN_WIDTH as u32 * factor,
        video::SCREEN_HEIGHT as u32 * factor,
    );
    window.set_position(
        sdl2::video::WindowPos::Centered,
        sdl2::video::WindowPos::Centered,
    );
}

/// Ouvre les fenêtres SDL2 et fait tourner la machine jusqu'à la fermeture.
/// `machine` doit déjà avoir ses ROMs (et, le cas échéant, sa disquette)
/// chargées ; `autotyper`, s'il y en a un, tape sa commande au clavier
/// émulé au fil des trames.
pub fn run(
    mut machine: Machine,
    mut autotyper: Option<AutoTyper>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 3. Initialisation de SDL2
    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;
    let controller_subsystem = sdl_context.game_controller()?;

    // Tentative d'ouverture de la première manette disponible. Réutilisée
    // aussi bien ici, au démarrage, que dans la boucle d'événements sur
    // branchement à chaud (voir `Event::ControllerDeviceAdded` plus bas) :
    // une seule manette active à la fois, comme au démarrage.
    let open_first_controller =
        |subsystem: &sdl2::GameControllerSubsystem| -> Option<sdl2::controller::GameController> {
            let num_joysticks = subsystem.num_joysticks().unwrap_or(0);
            for i in 0..num_joysticks {
                if subsystem.is_game_controller(i) {
                    match subsystem.open(i) {
                        Ok(c) => {
                            app_log!("Controller opened: {}", c.name());
                            return Some(c);
                        }
                        Err(e) => app_log!("Failed to open controller {}: {}", i, e),
                    }
                }
            }
            None
        };
    let mut active_controller = open_first_controller(&controller_subsystem);

    // 4. Ouverture de la sortie audio. Une machine sans carte son utilisable
    // ne doit pas empêcher l'émulateur de démarrer : on continue en silence.
    let mut audio = match crate::audio::Audio::new(&sdl_context) {
        Ok(a) => Some(a),
        Err(e) => {
            app_log!("Audio disabled: {e}");
            None
        }
    };

    // 5. Fenêtre "machine status" (F12), cachée par défaut. Son contenu est
    // un panneau egui (voir status_panel.rs, Plan V2.md jalon M1) : plus de
    // police à charger depuis le disque, egui embarque la sienne.
    let mut debug_visible = false;
    let mut debug_window = video_subsystem
        .window("Amstrad CPC 6128 - Machine Status", 900, 1250)
        .position_centered()
        .hidden()
        .resizable()
        .metal_view()
        // Reste visible par-dessus la fenêtre principale même en plein
        // écran (F4) : sans ça, recliquer sur l'émulateur repasse cette
        // fenêtre derrière, alors qu'elle sert justement à être consultée
        // en même temps que lui.
        .always_on_top()
        .build()?;
    // Même icône que la fenêtre principale : sans cet appel, le gestionnaire
    // de fenêtres retombe sur son icône par défaut (le logo Wayland sous
    // KDE/Wayland) pour cette fenêtre-ci uniquement.
    if let Err(e) = set_window_icon(&mut debug_window) {
        app_log!("Can't set debug window icon: {e}");
    }
    let debug_window_id = debug_window.id();
    let mut status_panel = crate::status_panel::StatusPanel::new(debug_window)?;

    // Console complète (F11), cachée par défaut, sur le même modèle que la
    // fenêtre de statut ci-dessus : elle remplace entièrement la console
    // pilotée depuis le terminal qui a lancé l'émulateur (voir
    // console_window.rs, Plan V2.md jalon M2) — il n'y en a plus d'autre.
    let mut console_window_visible = false;
    let mut console_win = video_subsystem
        .window("Amstrad CPC 6128 - Console", 900, 700)
        .position_centered()
        .hidden()
        .resizable()
        .metal_view()
        // Voir le commentaire équivalent sur la fenêtre de statut ci-dessus.
        .always_on_top()
        .build()?;
    if let Err(e) = set_window_icon(&mut console_win) {
        app_log!("Can't set console window icon: {e}");
    }
    let console_window_id = console_win.id();
    let mut console_window = ConsoleWindow::new(console_win)?;

    // Titre dynamique de la fenêtre en fonction du mode configuré
    let window_title = if machine.diagnostic_mode {
        "ByteBox - Amstrad CPC 6128 - BASIC 1.1 AZERTY + Diag ROM"
    } else {
        "ByteBox - Amstrad CPC 6128 - BASIC 1.1 AZERTY"
    };

    let mut window = video_subsystem
        .window(
            window_title,
            video::SCREEN_WIDTH as u32,
            video::SCREEN_HEIGHT as u32,
        )
        .position_centered()
        // Sans effet hors macOS ; là-bas, condition requise pour que wgpu
        // puisse créer une surface à partir du handle de cette fenêtre.
        .metal_view()
        .build()?;
    // Non bloquant : une icône manquante ou illisible ne doit pas empêcher
    // l'émulateur de démarrer, la fenêtre garde alors l'icône par défaut du
    // système de fenêtrage.
    if let Err(e) = set_window_icon(&mut window) {
        app_log!("Can't set window icon: {e}");
    }
    let mut renderer = Renderer::new(window)?;
    apply_display_mode(
        renderer.window_mut(),
        DisplayMode::from_config(machine.default_zoom()),
    );
    let main_window_id = renderer.window().id();
    let mut event_pump = sdl_context.event_pump()?;

    // Barre de commande rapide (F10, console_panel.rs) et console complète
    // (F11, console_window.rs) : deux vues du même historique
    // (Plan V2.md, jalon M2), alimentées via le même canal MonitorCmd.
    let mut quick_bar_visible = false;
    let mut quick_bar = QuickCommandBar::new();
    let mut console_log = ConsoleLog::new();
    let cmd_sender = machine.command_sender();
    // Tout ce qui a été journalisé avant l'ouverture des fenêtres (bannière
    // de démarrage, config invalide, --disk/--tape en ligne de commande...)
    // attendait dans la file globale (voir applog.rs) : on le récupère ici,
    // rien n'est perdu.
    for line in bytebox_core::applog::drain() {
        console_log.push_output(&line);
    }

    let mut frame_buffer = vec![0u8; video::SCREEN_WIDTH * video::SCREEN_HEIGHT * 3];
    // Deux fois la durée d'une trame standard (312 lignes de 256 ticks) : simple
    // garde-fou pour ne pas bloquer la boucle SDL si le VSYNC n'arrive jamais.
    let max_ticks_per_frame: u32 = 2 * 79_872;
    let mut running = true;

    // Cadence d'exécution. On vise une échéance absolue plutôt que de dormir une
    // durée fixe : avec une attente fixe, la période réelle vaut "temps de calcul
    // + attente" et la machine émulée tourne durablement trop lentement.
    // La durée d'une trame est celle du temps émulé qu'elle a réellement
    // consommé (voir machine::emulated_duration) : un logiciel qui reprogramme
    // le CRTC en cours de trame change bien la durée de trame, mais ses
    // registres ne la décrivent plus une fois la trame finie.
    let mut next_frame = std::time::Instant::now();
    // Trame arrêtée (émulation en pause) : cadence de repli pour continuer à
    // rafraîchir l'écran et lire la console sans tourner à vide.
    const PAUSED_FRAME: std::time::Duration = std::time::Duration::from_millis(20);
    let mut measure_start = std::time::Instant::now();
    let mut measured_ticks: u64 = 0;
    let mut late_frames: u32 = 0;

    while running {
        for event in event_pump.poll_iter() {
            // Alimente egui même quand le panneau est caché : sinon la
            // première trame après un F12 le retrouverait avec un état
            // d'entrée périmé (position de souris, modificateurs...).
            // `EguiSDL2State::sdl2_input_to_egui` filtre lui-même sur
            // l'identifiant de la fenêtre de statut, sans effet sur les
            // événements de la fenêtre principale.
            status_panel.handle_event(&event);
            console_window.handle_event(&event);
            // Même chose pour la barre rapide F10, superposée à la fenêtre
            // principale : `Renderer::handle_event` filtre lui aussi sur
            // l'identifiant de fenêtre.
            renderer.handle_event(&event);
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
                        status_panel.window_mut().hide();
                    } else if window_id == console_window_id {
                        console_window_visible = false;
                        console_window.window_mut().hide();
                    } else if window_id == main_window_id {
                        running = false;
                    }
                }
                // wgpu ne suit pas tout seul le redimensionnement de la
                // fenêtre (contrairement à `Canvas::set_logical_size`,
                // disparu avec lui) : il faut reconfigurer la surface à sa
                // nouvelle taille, sous peine de la dessiner étirée ou
                // tronquée jusqu'à la trame suivante.
                Event::Window {
                    win_event:
                        sdl2::event::WindowEvent::SizeChanged(..)
                        | sdl2::event::WindowEvent::Resized(..),
                    window_id,
                    ..
                } if window_id == main_window_id => {
                    renderer.resize();
                }
                Event::Window {
                    win_event:
                        sdl2::event::WindowEvent::SizeChanged(..)
                        | sdl2::event::WindowEvent::Resized(..),
                    window_id,
                    ..
                } if window_id == debug_window_id => {
                    status_panel.resize();
                }
                Event::Window {
                    win_event:
                        sdl2::event::WindowEvent::SizeChanged(..)
                        | sdl2::event::WindowEvent::Resized(..),
                    window_id,
                    ..
                } if window_id == console_window_id => {
                    console_window.resize();
                }
                // Le pointeur système n'a aucun rôle dans l'émulation (le
                // clavier et la manette suffisent) : il ne fait que masquer
                // l'image quand il traîne dessus. `show_cursor` est global à
                // SDL2 (pas par fenêtre), d'où la bascule sur Enter/Leave de
                // la fenêtre principale plutôt qu'un simple réglage figé au
                // démarrage — pour qu'il redevienne visible sur la fenêtre de
                // statut (F12) ou en dehors de l'émulateur.
                Event::Window {
                    win_event: sdl2::event::WindowEvent::Enter,
                    window_id,
                    ..
                } if window_id == main_window_id => {
                    sdl_context.mouse().show_cursor(false);
                }
                Event::Window {
                    win_event: sdl2::event::WindowEvent::Leave,
                    window_id,
                    ..
                } if window_id == main_window_id => {
                    sdl_context.mouse().show_cursor(true);
                }
                // Taille d'affichage : F1 normale, F2 x2, F3 x3, F4 plein
                // écran. Repasser par F1/F2/F3 quitte aussi le plein écran,
                // pour ne jamais y rester coincé sans savoir comment en
                // sortir.
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F1),
                    ..
                } => apply_display_mode(renderer.window_mut(), DisplayMode::Normal),
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F2),
                    ..
                } => apply_display_mode(renderer.window_mut(), DisplayMode::X2),
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F3),
                    ..
                } => apply_display_mode(renderer.window_mut(), DisplayMode::X3),
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F4),
                    ..
                } => {
                    // Bascule : F4 quitte le plein écran s'il est deja actif
                    // (par F4 ou par default_zoom = "fullscreen").
                    let currently_fullscreen = renderer.window().fullscreen_state()
                        != sdl2::video::FullscreenType::Off;
                    apply_display_mode(
                        renderer.window_mut(),
                        if currently_fullscreen {
                            DisplayMode::Normal
                        } else {
                            DisplayMode::Fullscreen
                        },
                    );
                }
                // Événements d'enfoncement de touches du clavier moderne PC
                //
                // Pas-à-pas CPU (F8) et pas-à-pas ligne (F9) : anciennement
                // F10/Shift+F10, déplacées pour lui laisser la barre de
                // commande rapide (voir plus bas).
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F8),
                    ..
                } => {
                    app_log!(
                        "{}",
                        (zilog_z80::dasm::dasm(&machine.bus, machine.cpu.reg.pc)).0
                    );
                    machine.step();
                    machine.print_registers();
                }
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F9),
                    ..
                } => {
                    let start_line = machine.current_line;
                    while machine.current_line == start_line {
                        let ticks = machine.step();
                        if ticks == 0 {
                            break;
                        }
                    }
                    app_log!(
                        "Stepped to next video line (Line {}).",
                        machine.current_line
                    );
                    machine.print_registers();
                }
                // Barre de commande rapide (F10, Plan V2.md jalon M2) :
                // superposée à la fenêtre principale plutôt que dans une
                // fenêtre séparée (contrairement à F11/F12), donc pas de
                // show()/hide() ici — son affichage tient entièrement à
                // `quick_bar_visible`, lu par la fermeture passée à
                // `renderer.present` plus bas. Reprend le focus à chaque
                // ouverture pour taper sans clic préalable.
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F10),
                    ..
                } => {
                    quick_bar_visible = !quick_bar_visible;
                    if quick_bar_visible {
                        quick_bar.request_focus();
                    }
                }
                // Console complète (F11) : fenêtre séparée, sur le même
                // modèle que le statut machine (F12).
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F11),
                    ..
                } => {
                    console_window_visible = !console_window_visible;
                    if console_window_visible {
                        console_window.window_mut().show();
                        console_window.request_focus();
                    } else {
                        console_window.window_mut().hide();
                    }
                }
                Event::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F12),
                    ..
                } => {
                    debug_visible = !debug_visible;
                    if debug_visible {
                        status_panel.window_mut().show();
                    } else {
                        status_panel.window_mut().hide();
                    }
                }
                // La barre rapide absorbe le clavier tant qu'elle est
                // ouverte, et cette entrée globale ne doit de toute façon
                // s'appliquer qu'à la fenêtre principale : sans le filtre
                // sur `window_id`, taper dans la console complète (F11,
                // fenêtre séparée) ou dans la fenêtre de statut (F12)
                // enverrait aussi chaque touche à la matrice clavier émulée.
                Event::KeyDown {
                    keycode: Some(kc),
                    scancode: Some(sc),
                    keymod,
                    window_id,
                    ..
                } if window_id == main_window_id && !quick_bar_visible => {
                    let shift_held = keymod.intersects(
                        sdl2::keyboard::Mod::LSHIFTMOD | sdl2::keyboard::Mod::RSHIFTMOD,
                    );
                    if !machine.bus.psg.set_key_state_scancode(sc, true, shift_held) {
                        machine.bus.psg.set_key_state(kc, true);
                    }
                }
                Event::KeyUp {
                    keycode: Some(kc),
                    scancode: Some(sc),
                    keymod,
                    ..
                } => {
                    let shift_held = keymod.intersects(
                        sdl2::keyboard::Mod::LSHIFTMOD | sdl2::keyboard::Mod::RSHIFTMOD,
                    );
                    if !machine.bus.psg.set_key_state_scancode(sc, false, shift_held) {
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
                // Branchement/débranchement à chaud : ces événements sont
                // générés nativement par SDL (relayés depuis udev/le pilote
                // système), pas par une scrutation active de notre part —
                // aucun coût CPU supplémentaire au repos, juste deux cas de
                // plus dans la boucle d'événements qui tourne déjà.
                Event::ControllerDeviceAdded { which, .. } => {
                    // Ne remplace pas une manette déjà active : la première
                    // branchée garde la main, comme au démarrage.
                    if active_controller.is_none() {
                        match controller_subsystem.open(which) {
                            Ok(c) => {
                                app_log!("Controller opened: {}", c.name());
                                active_controller = Some(c);
                            }
                            Err(e) => app_log!("Failed to open controller {which}: {e}"),
                        }
                    }
                }
                Event::ControllerDeviceRemoved { which, .. } => {
                    let was_active = active_controller
                        .as_ref()
                        .is_some_and(|c| c.instance_id() == which);
                    if was_active {
                        app_log!("Controller disconnected");
                        active_controller = None;
                        // Une direction ou un tir resté "enfoncé" au moment
                        // du débranchement resterait sinon bloqué indéfiniment
                        // dans la matrice clavier émulée.
                        for button in 0..7 {
                            machine.bus.psg.set_controller_button(button, false);
                        }
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
            let ticks = machine.step();
            if let Some(typer) = autotyper.as_mut() {
                typer.advance(&mut machine.bus.psg, ticks);
                if typer.is_done() {
                    autotyper = None;
                }
            }
            frame_ticks += ticks;
        }

        // Les échantillons produits pendant la trame partent vers la carte
        // son. En pause, le PSG n'avance plus : rien n'est produit et SDL
        // enchaîne sur du silence.
        let samples = machine.bus.psg.sound.take_samples();
        if let Some(audio) = audio.as_mut() {
            audio.set_volume(machine.volume());
            audio.push(&samples);
        }

        // Traité avant le rendu, pour que la sortie d'une commande saisie
        // dans l'une des deux façades console apparaisse dès cette trame,
        // plutôt qu'avec un tour de boucle de retard. Il n'y a plus de
        // terminal à alimenter : la sortie va uniquement dans le journal
        // partagé, lu aussi bien par la barre rapide (F10, une ligne) que
        // par la console complète (F11, tout l'historique).
        if let Ok(output) = machine.console_handle()
            && !output.is_empty()
        {
            console_log.push_output(&output);
        }
        // Idem pour tout ce que le reste de l'application a journalisé cette
        // trame (voir applog.rs) : connexion de manette, disquette éjectée,
        // avertissement de régulation audio... Rien de tout cela ne doit
        // atteindre le terminal qui a lancé l'émulateur.
        for line in bytebox_core::applog::drain() {
            console_log.push_output(&line);
        }

        // Appel au module vidéo déporté pour le rendu VRAM
        video::render(&machine, &mut frame_buffer);

        let mut quick_bar_ui = |ctx: &egui::Context| quick_bar.ui(ctx, &cmd_sender, &mut console_log);
        let overlay: Option<&mut dyn FnMut(&egui::Context)> = if quick_bar_visible {
            Some(&mut quick_bar_ui)
        } else {
            None
        };
        renderer.present(&frame_buffer, overlay);

        if debug_visible {
            let registers = machine.get_registers_string();
            let hardware = machine.get_hardware_string(machine.show_keyboard_matrix());
            status_panel.render(&registers, &hardware);
        }

        if console_window_visible {
            console_window.render(&mut console_log, &cmd_sender);
        }

        // Mesure de la vitesse réelle, moyennée sur une seconde : c'est elle
        // qui dit si la machine tient la cadence, et donc si le son a de quoi
        // alimenter la carte son en continu.
        measured_ticks += frame_ticks as u64;
        if measure_start.elapsed() >= std::time::Duration::from_secs(1) {
            let emulated = machine::emulated_duration(measured_ticks as u32).as_secs_f32();
            machine.set_measured_timing(
                100.0 * emulated / measure_start.elapsed().as_secs_f32(),
                late_frames,
            );
            // La régulation audio ne doit rien avoir à faire en régime normal.
            // Le moindre remplissage insère du silence dans le flux et étire la
            // musique : on le signale, il ne se voit nulle part ailleurs.
            // Débrayable, car sur une machine trop juste le message se
            // répéterait à chaque seconde (config.toml, [debugger] audio).
            if let Some(audio) = audio.as_mut().filter(|_| machine.report_audio_regulation()) {
                let (refills, padded_ms, dropped) = audio.take_stats();
                if refills > 0 || dropped > 0 {
                    app_log!(
                        "Audio: {refills} refill(s) ({padded_ms:.0} ms of silence inserted), \
                         {dropped} frame(s) dropped, queue {} samples",
                        audio.queued_samples()
                    );
                }
            }
            measured_ticks = 0;
            late_frames = 0;
            measure_start = std::time::Instant::now();
        }

        let frame_duration = if frame_ticks > 0 {
            machine::emulated_duration(frame_ticks)
        } else {
            PAUSED_FRAME
        };
        let now = std::time::Instant::now();
        if now < next_frame {
            std::thread::sleep(next_frame - now);
            next_frame += frame_duration;
        } else {
            // Trame en retard : on repart de l'instant courant plutôt que de
            // tenter de rattraper, ce qui emballerait la machine émulée. Le
            // temps perdu ne se rattrape donc jamais : la carte son se
            // retrouve à sec et le silence qu'elle joue à la place étire la
            // musique d'autant. Un décrochage bref suffit à l'entendre.
            next_frame = now + frame_duration;
            late_frames += 1;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_mode_recognizes_the_four_config_values() {
        assert_eq!(DisplayMode::from_config(Some("x1")), DisplayMode::Normal);
        assert_eq!(DisplayMode::from_config(Some("x2")), DisplayMode::X2);
        assert_eq!(DisplayMode::from_config(Some("x3")), DisplayMode::X3);
        assert_eq!(
            DisplayMode::from_config(Some("fullscreen")),
            DisplayMode::Fullscreen
        );
    }

    /// Absent ou mal orthographié, on ne bloque pas le démarrage : la
    /// fenêtre s'ouvre en taille normale plutôt que de faire planter
    /// l'émulateur sur une faute de frappe dans config.toml.
    #[test]
    fn display_mode_falls_back_to_normal_when_absent_or_unrecognized() {
        assert_eq!(DisplayMode::from_config(None), DisplayMode::Normal);
        assert_eq!(DisplayMode::from_config(Some("XL")), DisplayMode::Normal);
    }
}
