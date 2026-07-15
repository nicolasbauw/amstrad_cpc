mod bus;
mod crtc;
mod gate_array;
mod machine;
mod memory;
mod ppi;
mod psg;

use machine::Machine;
use sdl2::event::Event;
use sdl2::pixels::PixelFormatEnum;

fn render_vram_mode1(machine: &Machine, frame_buffer: &mut [u8; 320 * 200 * 3]) {
    for char_y in 0..25 {
        for pixel_y in 0..8 {
            let line_y = char_y * 8 + pixel_y;
            let base_addr = 0xC000 + (char_y * 80) + (pixel_y * 2048);

            for x_bytes in 0..80 {
                let addr = (base_addr + x_bytes) as u16;
                let byte = machine.bus.memory.read_byte(addr);

                let p0 = ((byte & 0x80) >> 7) | ((byte & 0x08) >> 2);
                let p1 = ((byte & 0x40) >> 6) | ((byte & 0x04) >> 1);
                let p2 = ((byte & 0x20) >> 5) | ((byte & 0x02) << 0);
                let p3 = ((byte & 0x10) >> 4) | ((byte & 0x01) << 1);

                let pixels = [p0, p1, p2, p3];

                for i in 0..4 {
                    let pixel_x = x_bytes * 4 + i;
                    let color_index = pixels[i] as usize;
                    let (r, g, b) = machine.bus.gate_array.get_rgb_color(color_index);

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Émulateur Amstrad CPC 6128 ===");

    let mut machine = Machine::new();
    machine.load_roms()?;

    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;
    let window = video_subsystem
        .window("Amstrad CPC 6128", 640, 400)
        .position_centered()
        .build()?;
    let mut canvas = window.into_canvas().build()?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(PixelFormatEnum::RGB24, 320, 200)?;
    let mut event_pump = sdl_context.event_pump()?;

    let mut frame_buffer = [0u8; 320 * 200 * 3];
    let ticks_per_frame: u32 = 79_872;
    let mut running = true;

    while running {
        for event in event_pump.poll_iter() {
            if let Event::Quit { .. } = event {
                running = false;
            }
        }

        let mut frame_ticks: u32 = 0;
        while frame_ticks < ticks_per_frame {
            frame_ticks += machine.step();
        }

        render_vram_mode1(&machine, &mut frame_buffer);
        let _ = texture.update(None, &frame_buffer, 320 * 3);
        let _ = canvas.clear();
        let _ = canvas.copy(&texture, None, None);
        canvas.present();

        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    Ok(())
}
