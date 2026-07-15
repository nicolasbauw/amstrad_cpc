use crate::machine::Machine;

/// Décode la VRAM en fonction du mode vidéo actuel et remplit le buffer RGB.
pub fn render(machine: &Machine, frame_buffer: &mut [u8]) {
    // Debug : quel est le mode actuel ?
    println!("Video mode: {}", machine.bus.gate_array.video_mode);
    match machine.bus.gate_array.video_mode {
        1 => render_mode1(machine, frame_buffer),
        _ => {
            // Si mode non supporté, on affiche au moins un motif de test ou on log
        }
    }
}

fn render_mode1(machine: &Machine, frame_buffer: &mut [u8]) {
    // Mode 1 : 320x200, 4 couleurs
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
