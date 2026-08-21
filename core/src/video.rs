use crate::gate_array::GateArrayState;
use crate::machine::Machine;

/// Dimensions du tampon de rendu (une fenêtre de moniteur, bordure comprise).
pub const SCREEN_WIDTH: usize = 800;
pub const SCREEN_HEIGHT: usize = 600;

/// Le moniteur ne connaît pas de coordonnées absolues : il se cale sur les
/// signaux de synchronisation. La position d'un caractère à l'écran est donc
/// donnée par sa distance au HSYNC (R2) et au VSYNC (R7), et non par un offset
/// fixe. Ces deux constantes représentent les temps de retour du faisceau
/// (back porch) du moniteur, exprimés dans les unités du CRTC ; elles sont
/// calibrées pour centrer un écran standard 40x25 dans la fenêtre.
const H_BACK_PORCH_CHARS: i32 = 13;
const V_BACK_PORCH_LINES: i32 = 22;

/// Un caractère CRTC fait 2 octets, rendus sur 16 pixels de large.
const PIXELS_PER_CHAR: i32 = 16;
/// Chaque scanline du CPC est doublée verticalement dans le tampon.
/// Public parce que le shader CRT (`renderer_crt.wgsl`) en a besoin : c'est
/// la vraie période d'une ligne de balayage, et donc celle de ses scanlines
/// — une par ligne du tampon en dessinerait deux fois trop.
pub const PIXELS_PER_SCANLINE: i32 = 2;

/// Abscisse du premier pixel d'une colonne de caractères, mesurée à partir du HSYNC.
fn char_x(x_char: u32, r2: i32, line_chars: i32) -> i32 {
    let chars_since_hsync = (x_char as i32 - r2).rem_euclid(line_chars);
    (chars_since_hsync - H_BACK_PORCH_CHARS) * PIXELS_PER_CHAR
}

/// Ordonnée de la première ligne du tampon pour une scanline, mesurée à partir du VSYNC.
fn scanline_y(scanline: u32, vsync_scanline: i32, frame_scanlines: i32) -> i32 {
    let lines_since_vsync = (scanline as i32 - vsync_scanline).rem_euclid(frame_scanlines);
    (lines_since_vsync - V_BACK_PORCH_LINES) * PIXELS_PER_SCANLINE
}

/// Décode la VRAM et remplit le buffer RGB en gérant les modes vidéo et la bordure.
/// Cette implémentation suit rigoureusement la logique MA/RA du CRTC 6845 et du Gate Array.
pub fn render(machine: &Machine, frame_buffer: &mut [u8]) {
    // 1. Géométrie programmée dans le CRTC
    let crtc = &machine.bus.crtc;
    let r1 = crtc.registers[1] as u32; // Caractères affichés par ligne
    let r2 = crtc.registers[2] as i32; // Position du HSYNC
    let r6 = crtc.registers[6] as u32; // Lignes de caractères affichées
    let r9 = (crtc.registers[9] & 0x1F) as u32; // Hauteur d'une ligne de caractères - 1

    let line_chars = crtc.line_chars() as i32;
    let frame_scanlines = crtc.frame_scanlines() as i32;
    let vsync_scanline = crtc.vsync_scanline() as i32;

    // État du Gate Array à une scanline donnée. Le rendu ne consulte jamais
    // l'état courant : celui-ci n'est que le dernier de la trame et écraserait
    // les changements de mode ou de palette faits en cours de balayage.
    let state_at = |scanline: i32| -> GateArrayState {
        let index = scanline.rem_euclid(frame_scanlines.max(1)) as usize;
        machine
            .scanline_states
            .get(index)
            .copied()
            .unwrap_or_else(|| machine.bus.gate_array.state())
    };

    // 2. Bordure. Elle occupe tout ce qui n'est pas la zone affichée, et sa
    // couleur se reprogramme aussi en cours de trame : on remplit donc chaque
    // ligne du tampon avec la couleur en vigueur sur la scanline correspondante.
    for y in 0..SCREEN_HEIGHT {
        let lines_since_vsync = y as i32 / PIXELS_PER_SCANLINE + V_BACK_PORCH_LINES;
        let (br, bg, bb) = state_at(lines_since_vsync + vsync_scanline).rgb(16);
        let row = &mut frame_buffer[y * SCREEN_WIDTH * 3..(y + 1) * SCREEN_WIDTH * 3];
        for pixel in row.chunks_exact_mut(3) {
            pixel[0] = br;
            pixel[1] = bg;
            pixel[2] = bb;
        }
    }

    if r1 == 0 || r6 == 0 {
        return; // Aucune zone affichée : l'écran est entièrement en bordure
    }

    // Adresse de départ de l'écran (R12/R13), base du compteur MA.
    let start_addr = ((crtc.registers[12] as u16) << 8) | (crtc.registers[13] as u16);

    // Abscisses des colonnes de caractères, calculées une fois pour toutes.
    let column_x: Vec<i32> = (0..r1).map(|x| char_x(x, r2, line_chars)).collect();

    for char_y in 0..r6 {
        // Le compteur MA repart de l'adresse de base à chaque ligne de
        // caractères et s'incrémente de R1 : c'est ce qui permet le scrolling
        // matériel par réécriture de R12/R13.
        let line_ma = start_addr.wrapping_add((char_y * r1) as u16) & 0x3FFF;

        for raster in 0..=r9 {
            let scanline = char_y * (r9 + 1) + raster;
            let line_y = scanline_y(scanline, vsync_scanline, frame_scanlines);

            if line_y + PIXELS_PER_SCANLINE <= 0 || line_y >= SCREEN_HEIGHT as i32 {
                continue;
            }

            let state = state_at(scanline as i32);

            // Octets tels que capturés au moment où le CRTC a réellement
            // balayé cette scanline pendant l'émulation (voir
            // `Machine::capture_beam_progress` et `capture_scanline_chars`),
            // plutôt que relus maintenant dans la VRAM courante. Un vrai
            // tube cathodique peint chaque ligne avec le contenu de la VRAM
            // tel qu'il était exactement à cet instant : une routine de
            // tracé de sprite par XOR (le CPC ne masque pas les
            // interruptions pendant ce genre de boucle) peut être
            // interrompue à mi-chemin par l'interruption vidéo, et sans
            // cette capture progressive, un instantané global pris en fin de
            // trame la surprendrait à moitié terminée — un sprite à moitié
            // effacé pendant une seule trame, perçu comme un clignotement
            // très rapide (voir doc/sprite-flicker.md).
            //
            // La capture est faite position de caractère par position de
            // caractère : le repli ci-dessous (lecture directe de la VRAM)
            // couvre donc aussi la fin d'une ligne que le faisceau n'avait
            // pas encore atteinte, pas seulement les lignes jamais
            // capturées.
            let captured = machine
                .scanline_vram
                .get(scanline as usize)
                .filter(|bytes| !bytes.is_empty());

            for x_char in 0..r1 {
                let ma = line_ma.wrapping_add(x_char as u16) & 0x3FFF;
                // RA ne fournit que 3 bits d'adresse : au delà de 8 scanlines
                // par ligne de caractères, le CRTC réaffiche les mêmes octets.
                let addr_base =
                    ((ma & 0x3000) << 2) | (((raster as u16) & 0x07) << 11) | ((ma & 0x03FF) << 1);
                let x_base = column_x[x_char as usize];

                for byte_off in 0..2u16 {
                    let byte = captured
                        .and_then(|bytes| bytes.get((x_char * 2 + byte_off as u32) as usize))
                        .copied()
                        .unwrap_or_else(|| {
                            machine
                                .bus
                                .memory
                                .read_video_ram_byte(addr_base.wrapping_add(byte_off))
                        });
                    let x_byte = x_base + byte_off as i32 * 8;

                    for dy in 0..PIXELS_PER_SCANLINE {
                        let y = line_y + dy;
                        if y < 0 || y >= SCREEN_HEIGHT as i32 {
                            continue;
                        }
                        match state.video_mode {
                            0 => render_byte_mode0(&state, frame_buffer, byte, x_byte, y as usize),
                            1 => render_byte_mode1(&state, frame_buffer, byte, x_byte, y as usize),
                            2 => render_byte_mode2(&state, frame_buffer, byte, x_byte, y as usize),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

/// Capture les octets de VRAM affichés sur la scanline courante du CRTC
/// (`crtc.char_row`/`crtc.raster`), pour que `render` puisse ensuite peindre
/// cette ligne avec le contenu exact qu'un tube cathodique y aurait vu
/// passer — pas l'état (potentiellement plus tardif) de la VRAM au moment où
/// toute la trame est dessinée d'un coup.
///
/// Capture PROGRESSIVE : `upto_char` dit jusqu'où le faisceau est arrivé
/// dans la ligne, et seules les positions de caractère nouvellement
/// franchies sont ajoutées à `out` (deux octets chacune, comme le CRTC les
/// lit). Appelée plusieurs fois par scanline depuis `Machine::step`, au fil
/// de l'avancée du faisceau, plutôt qu'une seule fois en début de ligne :
/// une écriture survenant en milieu de ligne se voit alors sur sa moitié
/// droite mais pas sur la gauche, déjà balayée — ce que la capture en un
/// bloc ne pouvait pas reproduire (`Plan V3.md`, point 4).
///
/// `out` reste vide (plutôt que de recevoir des données obsolètes) quand la
/// ligne courante n'appartient pas à la zone affichée : `render` retombe
/// alors sur une lecture directe de la VRAM pour cette ligne, ce qui n'a pas
/// d'incidence puisque seule la bordure y est dessinée. Même repli, position
/// par position, pour la fin d'une ligne que le faisceau n'a pas encore
/// atteinte.
pub fn capture_scanline_chars(
    crtc: &crate::crtc::Crtc,
    memory: &crate::memory::Memory,
    out: &mut Vec<u8>,
    upto_char: u32,
) {
    let r1 = crtc.registers[1] as u32;
    let r6 = crtc.registers[6] as u32;
    if r1 == 0 || (crtc.char_row as u32) >= r6 {
        return;
    }
    // Le CRTC ne lit la VRAM que pendant la fenêtre d'affichage, les R1
    // premiers caractères de la ligne : au-delà, le faisceau est dans la
    // bordure et il n'y a plus rien à capturer.
    let target = upto_char.min(r1);
    let already = (out.len() / 2) as u32;
    if already >= target {
        return;
    }

    let start_addr = ((crtc.registers[12] as u16) << 8) | (crtc.registers[13] as u16);
    let line_ma = start_addr.wrapping_add((crtc.char_row as u32 * r1) as u16) & 0x3FFF;
    let raster = crtc.raster as u16;

    out.reserve(2 * (target - already) as usize);
    for x_char in already..target {
        let ma = line_ma.wrapping_add(x_char as u16) & 0x3FFF;
        let addr_base = ((ma & 0x3000) << 2) | ((raster & 0x07) << 11) | ((ma & 0x03FF) << 1);
        out.push(memory.read_video_ram_byte(addr_base));
        out.push(memory.read_video_ram_byte(addr_base.wrapping_add(1)));
    }
}

/// Écrit un pixel du tampon en écartant tout ce qui sort de la fenêtre.
fn put_pixel(frame_buffer: &mut [u8], x: i32, y: usize, (r, g, b): (u8, u8, u8)) {
    if x < 0 || x >= SCREEN_WIDTH as i32 {
        return;
    }
    let idx = (y * SCREEN_WIDTH + x as usize) * 3;
    frame_buffer[idx] = r;
    frame_buffer[idx + 1] = g;
    frame_buffer[idx + 2] = b;
}

/// Peint `width` pixels consécutifs de la même couleur.
fn draw_pixel_run(
    state: &GateArrayState,
    frame_buffer: &mut [u8],
    color_idx: u8,
    x_start: i32,
    width: i32,
    y: usize,
) {
    let rgb = state.rgb(color_idx as usize);
    for dx in 0..width {
        put_pixel(frame_buffer, x_start + dx, y, rgb);
    }
}

// Mode 0 : 160x200, 16 couleurs (2 pixels par octet)
fn render_byte_mode0(
    state: &GateArrayState,
    frame_buffer: &mut [u8],
    byte: u8,
    x_byte: i32,
    y: usize,
) {
    let p0 =
        ((byte & 0x80) >> 7) | ((byte & 0x08) >> 2) | ((byte & 0x20) >> 3) | ((byte & 0x02) << 2);
    let p1 =
        ((byte & 0x40) >> 6) | ((byte & 0x04) >> 1) | ((byte & 0x10) >> 2) | ((byte & 0x01) << 3);

    for (i, &color_idx) in [p0, p1].iter().enumerate() {
        draw_pixel_run(state, frame_buffer, color_idx, x_byte + i as i32 * 4, 4, y);
    }
}

// Mode 1 : 320x200, 4 couleurs (4 pixels par octet)
fn render_byte_mode1(
    state: &GateArrayState,
    frame_buffer: &mut [u8],
    byte: u8,
    x_byte: i32,
    y: usize,
) {
    let p0 = ((byte & 0x80) >> 7) | ((byte & 0x08) >> 2);
    let p1 = ((byte & 0x40) >> 6) | ((byte & 0x04) >> 1);
    let p2 = ((byte & 0x20) >> 5) | (byte & 0x02);
    let p3 = ((byte & 0x10) >> 4) | ((byte & 0x01) << 1);

    for (i, &color_idx) in [p0, p1, p2, p3].iter().enumerate() {
        draw_pixel_run(state, frame_buffer, color_idx, x_byte + i as i32 * 2, 2, y);
    }
}

// Mode 2 : 640x200, 2 couleurs (8 pixels par octet)
fn render_byte_mode2(
    state: &GateArrayState,
    frame_buffer: &mut [u8],
    byte: u8,
    x_byte: i32,
    y: usize,
) {
    for i in 0..8 {
        let color_idx = (byte >> (7 - i)) & 1;
        draw_pixel_run(state, frame_buffer, color_idx, x_byte + i, 1, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crtc::Crtc;

    /// Sur un écran standard 40x25, le calage par la synchro doit reproduire
    /// exactement le centrage qui était auparavant codé en dur (80, 100), et
    /// l'image doit tenir dans la fenêtre.
    #[test]
    fn standard_screen_is_centered_in_the_window() {
        let crtc = Crtc::new();
        let r2 = crtc.registers[2] as i32;
        let line_chars = crtc.line_chars() as i32;
        let frame = crtc.frame_scanlines() as i32;
        let vsync = crtc.vsync_scanline() as i32;

        assert_eq!(char_x(0, r2, line_chars), 80);
        assert_eq!(scanline_y(0, vsync, frame), 100);

        // 40 caractères de 16 px = 640 px, 200 scanlines doublées = 400 px.
        assert_eq!(char_x(39, r2, line_chars) + PIXELS_PER_CHAR, 80 + 640);
        assert_eq!(
            scanline_y(199, vsync, frame) + PIXELS_PER_SCANLINE,
            100 + 400
        );
        assert!(80 + 640 <= SCREEN_WIDTH as i32);
        assert!(100 + 400 <= SCREEN_HEIGHT as i32);
    }

    /// Retarder une synchro rapproche la zone affichée de la synchro précédente :
    /// l'image se décale donc vers la gauche (R2) ou vers le haut (R7), ce qui est
    /// le sens observé sur un vrai CPC.
    #[test]
    fn delaying_a_sync_moves_the_picture_back() {
        let mut crtc = Crtc::new();
        let line_chars = crtc.line_chars() as i32;

        let x_before = char_x(0, crtc.registers[2] as i32, line_chars);
        crtc.registers[2] += 2; // HSYNC repoussé de 2 caractères
        let x_after = char_x(0, crtc.registers[2] as i32, line_chars);
        assert_eq!(x_after - x_before, -2 * PIXELS_PER_CHAR);

        let y_before = scanline_y(
            0,
            crtc.vsync_scanline() as i32,
            crtc.frame_scanlines() as i32,
        );
        crtc.registers[7] += 1; // VSYNC repoussé d'une ligne de caractères
        let y_after = scanline_y(
            0,
            crtc.vsync_scanline() as i32,
            crtc.frame_scanlines() as i32,
        );
        assert_eq!(y_after - y_before, -8 * PIXELS_PER_SCANLINE);
    }

    /// Le cœur de la capture per-caractère (`Plan V3.md`, point 4) : une
    /// écriture VRAM survenant alors que le faisceau est au milieu de la
    /// ligne ne doit se voir QUE sur les positions pas encore balayées. La
    /// capture d'une ligne d'un seul bloc, elle, donnait forcément la même
    /// valeur partout — soit l'ancienne, soit la nouvelle.
    #[test]
    fn a_write_mid_line_only_shows_on_the_part_not_yet_scanned() {
        let crtc = Crtc::new();
        let mut memory = crate::memory::Memory::new(0);
        let r1 = crtc.registers[1] as usize; // 40 caractères affichés
        assert!(r1 > 20, "le test suppose une ligne d'au moins 20 caracteres");

        // Toute la VRAM à 0xAA, puis le faisceau parcourt les 10 premiers
        // caractères de la ligne.
        memory.ram[..64 * 1024].fill(0xAA);
        let mut captured = Vec::new();
        capture_scanline_chars(&crtc, &memory, &mut captured, 10);
        assert_eq!(captured.len(), 20, "10 caracteres = 20 octets");

        // Le programme réécrit la VRAM pendant que le faisceau est là, puis
        // le balayage se poursuit jusqu'au bout de la ligne.
        memory.ram[..64 * 1024].fill(0x55);
        capture_scanline_chars(&crtc, &memory, &mut captured, r1 as u32);
        assert_eq!(captured.len(), r1 * 2, "toute la ligne doit etre capturee");

        assert!(
            captured[..20].iter().all(|&b| b == 0xAA),
            "la moitie gauche, deja balayee, doit garder l'ancien contenu"
        );
        assert!(
            captured[20..].iter().all(|&b| b == 0x55),
            "la partie balayee apres l'ecriture doit montrer le nouveau contenu"
        );
    }

    /// Le faisceau ne capture jamais au-delà de la fenêtre d'affichage (les
    /// R1 premiers caractères) : le reste de la ligne est de la bordure, où
    /// le CRTC ne lit pas la VRAM.
    #[test]
    fn capture_stops_at_the_end_of_the_display_window() {
        let crtc = Crtc::new();
        let memory = crate::memory::Memory::new(0);
        let r1 = crtc.registers[1] as usize;

        let mut captured = Vec::new();
        // Bien au-delà de R1 : la ligne complète fait R0+1 = 64 caractères.
        capture_scanline_chars(&crtc, &memory, &mut captured, 64);
        assert_eq!(captured.len(), r1 * 2);
    }
}
