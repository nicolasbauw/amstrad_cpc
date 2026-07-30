use crate::machine::Machine;

/// Décode la VRAM et remplit le buffer RGB en gérant les modes vidéo et la bordure.
/// Cette implémentation suit rigoureusement la logique MA/RA du CRTC 6845 et du Gate Array.
pub fn render(machine: &Machine, frame_buffer: &mut [u8]) {
    // 1. Remplissage initial avec la couleur de la bordure (Pen 16)
    let (br, bg, bb) = machine.bus.gate_array.get_rgb_color(16);
    for i in 0..(frame_buffer.len() / 3) {
        frame_buffer[i * 3] = br;
        frame_buffer[i * 3 + 1] = bg;
        frame_buffer[i * 3 + 2] = bb;
    }

    // 2. Récupération des paramètres CRTC avec valeurs de sécurité
    let r1 = machine.bus.crtc.registers[1] as u16; // Nb caractères horizontaux
    let r6 = machine.bus.crtc.registers[6] as u16; // Nb lignes de caractères
    let r9 = (machine.bus.crtc.registers[9] & 0x1F) as u16; // Hauteur d'un caractère (max scanline)

    // Le CPC fonctionne normalement avec 200 lignes visibles, soit 25 lignes de 8 scanlines.
    // Le CRTC gère RA (Raster Address) qui définit la scanline actuelle au sein du bloc.
    // L'affichage est "écrasé" si on ignore le saut de scanline ou si on lit trop vite.

    let start_addr =
        ((machine.bus.crtc.registers[12] as u16) << 8) | (machine.bus.crtc.registers[13] as u16);
    let mode = machine.bus.gate_array.video_mode;

    // Centrage (offset_x/y)
    let offset_x = 80;
    let offset_y = 100;

    for char_y in 0..r6 {
        for pixel_y in 0..=r9 {
            let line_y_base = offset_y + (char_y * (r9 + 1) + pixel_y) as usize * 2;
            if line_y_base >= 600 {
                break;
            }

            let line_ma = (start_addr + (char_y * r1)) & 0x3FFF;

            for x_char in 0..r1 {
                let ma = (line_ma + x_char) & 0x3FFF;
                let addr_base =
                    ((ma & 0x3000) << 2) | ((pixel_y & 0x07) << 11) | ((ma & 0x03FF) << 1);

                for byte_off in 0..2 {
                    let addr = addr_base + byte_off as u16;
                    let byte = machine.bus.memory.read_ram_byte(addr);

                    for dy in 0..2 {
                        let line_y = line_y_base + dy;
                        if line_y >= 600 {
                            continue;
                        }
                        match mode {
                            0 => render_byte_mode0(
                                machine,
                                frame_buffer,
                                byte,
                                x_char,
                                byte_off,
                                line_y,
                                offset_x,
                            ),
                            1 => render_byte_mode1(
                                machine,
                                frame_buffer,
                                byte,
                                x_char,
                                byte_off,
                                line_y,
                                offset_x,
                            ),
                            2 => render_byte_mode2(
                                machine,
                                frame_buffer,
                                byte,
                                x_char,
                                byte_off,
                                line_y,
                                offset_x,
                            ),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

// Mode 0 : 160x200, 16 couleurs (2 pixels par octet)
fn render_byte_mode0(
    machine: &Machine,
    frame_buffer: &mut [u8],
    byte: u8,
    x_char: u16,
    byte_off: u8,
    line_y: usize,
    offset_x: usize,
) {
    let p0 =
        ((byte & 0x80) >> 7) | ((byte & 0x08) >> 2) | ((byte & 0x20) >> 3) | ((byte & 0x02) << 2);
    let p1 =
        ((byte & 0x40) >> 6) | ((byte & 0x04) >> 1) | ((byte & 0x10) >> 2) | ((byte & 0x01) << 3);

    let pixels = [p0, p1];
    for (i, &color_idx) in pixels.iter().enumerate() {
        let (r, g, b) = machine.bus.gate_array.get_rgb_color(color_idx as usize);
        let start_x = offset_x + (x_char as usize * 16) + (byte_off as usize * 8) + (i * 4);
        for dx in 0..4 {
            let x = start_x + dx;
            if x < 800 {
                let fb_idx = (line_y * 800 + x) * 3;
                frame_buffer[fb_idx] = r;
                frame_buffer[fb_idx + 1] = g;
                frame_buffer[fb_idx + 2] = b;
            }
        }
    }
}

// Mode 1 : 320x200, 4 couleurs (4 pixels par octet)
fn render_byte_mode1(
    machine: &Machine,
    frame_buffer: &mut [u8],
    byte: u8,
    x_char: u16,
    byte_off: u8,
    line_y: usize,
    offset_x: usize,
) {
    let p0 = ((byte & 0x80) >> 7) | ((byte & 0x08) >> 2);
    let p1 = ((byte & 0x40) >> 6) | ((byte & 0x04) >> 1);
    let p2 = ((byte & 0x20) >> 5) | ((byte & 0x02) << 0);
    let p3 = ((byte & 0x10) >> 4) | ((byte & 0x01) << 1);

    let pixels = [p0, p1, p2, p3];
    for (i, &color_idx) in pixels.iter().enumerate() {
        let (r, g, b) = machine.bus.gate_array.get_rgb_color(color_idx as usize);
        let start_x = offset_x + (x_char as usize * 16) + (byte_off as usize * 8) + (i * 2);
        for dx in 0..2 {
            let x = start_x + dx;
            if x < 800 {
                let fb_idx = (line_y * 800 + x) * 3;
                frame_buffer[fb_idx] = r;
                frame_buffer[fb_idx + 1] = g;
                frame_buffer[fb_idx + 2] = b;
            }
        }
    }
}

// Mode 2 : 640x200, 2 couleurs (8 pixels par octet)
fn render_byte_mode2(
    machine: &Machine,
    frame_buffer: &mut [u8],
    byte: u8,
    x_char: u16,
    byte_off: u8,
    line_y: usize,
    offset_x: usize,
) {
    for i in 0..8 {
        let color_idx = (byte >> (7 - i)) & 1;
        let (r, g, b) = machine.bus.gate_array.get_rgb_color(color_idx as usize);
        let x = offset_x + (x_char as usize * 16) + (byte_off as usize * 8) + i;
        if x < 800 {
            let fb_idx = (line_y * 800 + x) * 3;
            frame_buffer[fb_idx] = r;
            frame_buffer[fb_idx + 1] = g;
            frame_buffer[fb_idx + 2] = b;
        }
    }
}
