//! Fenêtre "machine status" (F12), en egui au-dessus de wgpu (Plan V2.md,
//! jalon M1) : remplace le texte dessiné à la main avec `SDL_ttf` par un
//! panneau egui, à périmètre fonctionnel identique.
//!
//! Contexte GPU volontairement indépendant de celui de `renderer.rs` (fenêtre
//! principale) plutôt que partagé : cette fenêtre de diagnostic, cachée par
//! défaut et peu sollicitée, ne justifie pas le risque de coupler son cycle
//! de vie à celui du rendu de la trame émulée. Le coût (un second contexte
//! wgpu) est négligeable au regard de ce que ça évite de complexité.

use egui_sdl2_event::EguiSDL2State;
use sdl2::video::Window;

pub struct StatusPanel {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_state: EguiSDL2State,
    egui_renderer: egui_wgpu::Renderer,
    start: std::time::Instant,
    /// # Sécurité
    ///
    /// Voir `renderer::Renderer` : `surface` est créée sans lien de durée de
    /// vie formel, `window` doit donc rester en vie au moins aussi longtemps
    /// — garanti ici en la possédant, et en la déclarant après `surface`
    /// (Rust détruit les champs dans leur ordre de déclaration).
    window: Window,
}

impl StatusPanel {
    pub fn new(window: Window) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // SAFETY : voir le commentaire de sécurité sur `StatusPanel`.
        let surface = unsafe {
            instance
                .create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&window).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .map_err(|e| e.to_string())?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bytebox status panel device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| e.to_string())?;

        let (draw_w, draw_h) = window.drawable_size();
        let surface_caps = surface.get_capabilities(&adapter);
        // Non-sRGB, comme le recommande la doc d'`egui_wgpu::Renderer::new` :
        // le shader d'egui écrit déjà des valeurs en espace gamma, un format
        // de surface *_Srgb lui appliquerait un second gamma au moment du
        // blending.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: draw_w.max(1),
            height: draw_h.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let egui_ctx = egui::Context::default();
        let egui_renderer =
            egui_wgpu::Renderer::new(&device, surface_format, egui_wgpu::RendererOptions::default());
        // dpi_scaling à 1.0 : `drawable_size` donne déjà des pixels physiques,
        // et cette fenêtre n'a pas besoin d'être nette sur un écran HiDPI à
        // tout prix pour un simple panneau de diagnostic.
        let egui_state = EguiSDL2State::new(draw_w, draw_h, 1.0);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            egui_ctx,
            egui_state,
            egui_renderer,
            start: std::time::Instant::now(),
            window,
        })
    }

    pub fn window_mut(&mut self) -> &mut Window {
        &mut self.window
    }

    /// À appeler pour chaque événement SDL2 reçu, qu'il concerne cette
    /// fenêtre ou non : `EguiSDL2State` filtre lui-même sur l'identifiant de
    /// fenêtre.
    pub fn handle_event(&mut self, event: &sdl2::event::Event) {
        self.egui_state.sdl2_input_to_egui(&self.window, event);
    }

    /// À appeler sur tout `WindowEvent::SizeChanged`/`Resized` de cette
    /// fenêtre.
    pub fn resize(&mut self) {
        let (w, h) = self.window.drawable_size();
        self.config.width = w.max(1);
        self.config.height = h.max(1);
        self.surface.configure(&self.device, &self.config);
        self.egui_state.update_screen_rect(w, h);
    }

    /// Dessine le panneau à partir des mêmes chaînes que produisait
    /// l'ancien rendu SDL_ttf (`Machine::get_registers_string`,
    /// `Machine::get_hardware_string`) : texte monospace clair sur fond bleu
    /// nuit, avec le marqueur "accès disque en cours" (point rouge en fin de
    /// ligne) rendu à part, comme avant.
    pub fn render(&mut self, registers: &str, hardware: &str) {
        self.egui_state
            .update_time(Some(self.start.elapsed().as_secs_f64()), 1.0 / 60.0);
        let raw_input = std::mem::take(&mut self.egui_state.raw_input);

        let mut text = String::with_capacity(registers.len() + 1 + hardware.len());
        text.push_str(registers);
        text.push('\n');
        text.push_str(hardware);

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::default()
                        .fill(egui::Color32::from_rgb(15, 15, 25))
                        .inner_margin(10.0),
                )
                .show(ctx, |ui| {
                    for line in text.lines() {
                        let line = line.replace('\t', "    ");
                        let (text_part, dot) = match line.strip_suffix('\u{25CF}') {
                            Some(prefix) => (prefix, true),
                            None => (line.as_str(), false),
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if !text_part.is_empty() {
                                ui.label(
                                    egui::RichText::new(text_part)
                                        .monospace()
                                        .color(egui::Color32::from_rgb(220, 220, 225)),
                                );
                            }
                            if dot {
                                ui.label(
                                    egui::RichText::new("\u{25CF}")
                                        .monospace()
                                        .color(egui::Color32::from_rgb(220, 40, 40)),
                                );
                            }
                        });
                    }
                });
        });
        self.egui_state
            .process_output(&self.window, &full_output.platform_output);

        let output = match self.surface.get_current_texture() {
            Ok(o) => o,
            Err(_) => {
                self.resize();
                return;
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("status panel encoder"),
            });
        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        {
            let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("status panel pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            // `egui_wgpu::Renderer::render` exige une passe 'static : elle
            // garde en vie elle-même toutes les ressources dont elle a
            // besoin, voir sa documentation.
            let mut rpass = rpass.forget_lifetime();
            self.egui_renderer
                .render(&mut rpass, &paint_jobs, &screen_descriptor);
        }
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.queue.submit([encoder.finish()]);
        output.present();
    }
}
