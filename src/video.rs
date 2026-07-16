use crate::machine::Machine;

/// Décode la VRAM et remplit le buffer RGB.
/// Pour l'instant, la ROM de diagnostic fonctionne exclusivement en Mode 1 (320x200, 4 couleurs).
/// Nous forçons le rendu en Mode 1 pour garantir un affichage fiable et performant.
pub fn render(machine: &Machine, frame_buffer: &mut [u8]) {
    render_mode1(machine, frame_buffer);
}

fn render_mode1(machine: &Machine, frame_buffer: &mut [u8]) {
    // Mode 1 : 320x200, 4 couleurs.
    // L'écran est organisé en 25 lignes de texte de 8 pixels de haut chacune.
    for char_y in 0..25 {
        for pixel_y in 0..8 {
            let line_y = char_y * 8 + pixel_y;
            // Adresse de départ de la ligne de pixel en VRAM (entrelacée de $0800 = 2048 octets)
            let base_addr = 0xC000 + (char_y * 80) + (pixel_y * 2048);

            for x_bytes in 0..80 {
                let addr = (base_addr + x_bytes) as u16;
                let byte = machine.bus.memory.read_byte(addr);

                // En Mode 1, un octet contient 4 pixels horizontaux :
                // Pixel 0 (gauche) : bits (7, 3)
                // Pixel 1 : bits (6, 2)
                // Pixel 2 : bits (5, 1)
                // Pixel 3 (droite) : bits (4, 0)
                let p0 = ((byte & 0x80) >> 7) | ((byte & 0x08) >> 2);
                let p1 = ((byte & 0x40) >> 6) | ((byte & 0x04) >> 1);
                let p2 = ((byte & 0x20) >> 5) | ((byte & 0x02) << 0);
                let p3 = ((byte & 0x10) >> 4) | ((byte & 0x01) << 1);

                let pixels = [p0, p1, p2, p3];

                for i in 0..4 {
                    let pixel_x = x_bytes * 4 + i;
                    let color_index = pixels[i] as usize;

                    // Récupération de la couleur RGB associée à travers la palette du Gate Array
                    let (r, g, b) = machine.bus.gate_array.get_rgb_color(color_index);

                    // Écriture dans le frame buffer RGB
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
