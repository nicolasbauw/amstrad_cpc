//! Écriture d'instantanés au format `.SNA` (le format d'échange standard
//! des émulateurs CPC).
//!
//! Utile en soi (sauvegarder l'état d'une partie), mais c'est surtout un
//! outil de diagnostic : un instantané pris chez nous se recharge dans un
//! autre émulateur, ce qui permet de transplanter un état exact et de
//! comparer les comportements à partir du même point — sans dépendre d'une
//! frappe clavier reproductible.
//!
//! Seule l'écriture est implémentée (pas la relecture) : c'est ce dont on a
//! besoin pour la comparaison, et un format à moitié relu serait un piège.
//!
//! La disposition de l'en-tête suit `t_SNA_header` de Caprice32 (256 octets,
//! structure compactée), puisque c'est lui qui devra relire nos fichiers.

use crate::machine::Machine;
use std::fs::File;
use std::io::Write;

/// Taille de l'en-tête, avant le vidage de la RAM.
const HEADER_LEN: usize = 256;

/// Offsets dans l'en-tête, repris un à un de `t_SNA_header`. Les nommer
/// évite de compter les octets à la main à chaque champ.
mod off {
    pub const VERSION: usize = 0x10;
    pub const AF: usize = 0x11;
    pub const BC: usize = 0x13;
    pub const DE: usize = 0x15;
    pub const HL: usize = 0x17;
    pub const R: usize = 0x19;
    pub const I: usize = 0x1A;
    pub const IFF0: usize = 0x1B;
    pub const IFF1: usize = 0x1C;
    pub const IX: usize = 0x1D;
    pub const IY: usize = 0x1F;
    pub const SP: usize = 0x21;
    pub const PC: usize = 0x23;
    pub const IM: usize = 0x25;
    pub const AFX: usize = 0x26;
    pub const BCX: usize = 0x28;
    pub const DEX: usize = 0x2A;
    pub const HLX: usize = 0x2C;
    pub const GA_PEN: usize = 0x2E;
    pub const GA_INK: usize = 0x2F;
    pub const GA_ROM_CONFIG: usize = 0x40;
    pub const GA_RAM_CONFIG: usize = 0x41;
    pub const CRTC_REG_SELECT: usize = 0x42;
    pub const CRTC_REGISTERS: usize = 0x43;
    pub const UPPER_ROM: usize = 0x55;
    pub const PPI_A: usize = 0x56;
    pub const PPI_B: usize = 0x57;
    pub const PPI_C: usize = 0x58;
    pub const PPI_CONTROL: usize = 0x59;
    pub const PSG_REG_SELECT: usize = 0x5A;
    pub const PSG_REGISTERS: usize = 0x5B;
    pub const RAM_SIZE: usize = 0x6B;
}

/// Construit l'en-tête de 256 octets décrivant l'état courant de `machine`.
fn build_header(machine: &Machine, ram_kb: u16) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..8].copy_from_slice(b"MV - SNA");

    // Version 1 : registres, Gate Array, CRTC, PPI, PSG et RAM. Les
    // extensions 2 et 3 (modèle de machine, état du FDC, compteurs internes
    // du CRTC...) décrivent des détails que nous ne modélisons pas
    // identiquement ; les annoncer serait promettre plus que ce qu'on écrit.
    h[off::VERSION] = 1;

    let r = &machine.cpu.reg;
    let alt = &machine.cpu.alt;

    // Chaque paire est stockée en petit-boutien : [bas, haut].
    h[off::AF] = r.flags.to_byte();
    h[off::AF + 1] = r.a;
    h[off::BC] = r.c;
    h[off::BC + 1] = r.b;
    h[off::DE] = r.e;
    h[off::DE + 1] = r.d;
    h[off::HL] = r.l;
    h[off::HL + 1] = r.h;
    h[off::R] = r.r;
    h[off::I] = r.i;
    // Le format sépare les deux bascules d'interruption, dans cet ordre
    // (IFF0 porte IFF1, IFF1 porte IFF2 — nommage historique du format).
    h[off::IFF0] = u8::from(machine.cpu.iff1());
    h[off::IFF1] = u8::from(machine.cpu.iff2());
    h[off::IX] = r.ixl;
    h[off::IX + 1] = r.ixh;
    h[off::IY] = r.iyl;
    h[off::IY + 1] = r.iyh;
    h[off::SP..off::SP + 2].copy_from_slice(&r.sp.to_le_bytes());
    h[off::PC..off::PC + 2].copy_from_slice(&r.pc.to_le_bytes());
    h[off::IM] = machine.cpu.im();
    h[off::AFX] = alt.flags.to_byte();
    h[off::AFX + 1] = alt.a;
    h[off::BCX] = alt.c;
    h[off::BCX + 1] = alt.b;
    h[off::DEX] = alt.e;
    h[off::DEX + 1] = alt.d;
    h[off::HLX] = alt.l;
    h[off::HLX + 1] = alt.h;

    // Gate Array
    let ga = &machine.bus.gate_array;
    h[off::GA_PEN] = ga.selected_pen;
    h[off::GA_INK..off::GA_INK + 17].copy_from_slice(&ga.palette);
    // Registre de configuration du Gate Array : bits 0-1 le mode vidéo,
    // bit 2 la ROM basse et bit 3 la ROM haute — à 1 pour DÉSACTIVÉE, donc
    // l'inverse de nos drapeaux "enabled".
    let mem = &machine.bus.memory;
    let mut rom_config = ga.video_mode & 0x03;
    if !mem.rom_low_enabled {
        rom_config |= 0x04;
    }
    if !mem.rom_high_enabled {
        rom_config |= 0x08;
    }
    h[off::GA_ROM_CONFIG] = rom_config;
    h[off::GA_RAM_CONFIG] = mem.ram_config;

    // CRTC
    let crtc = &machine.bus.crtc;
    h[off::CRTC_REG_SELECT] = crtc.selected_register;
    h[off::CRTC_REGISTERS..off::CRTC_REGISTERS + 18].copy_from_slice(&crtc.registers);

    h[off::UPPER_ROM] = mem.selected_high_rom;

    // PPI
    let ppi = &machine.bus.ppi;
    h[off::PPI_A] = ppi.port_a;
    h[off::PPI_B] = ppi.port_b_input;
    h[off::PPI_C] = ppi.port_c;
    h[off::PPI_CONTROL] = ppi.control_register;

    // PSG
    let psg = &machine.bus.psg;
    h[off::PSG_REG_SELECT] = psg.selected_register;
    h[off::PSG_REGISTERS..off::PSG_REGISTERS + 16].copy_from_slice(&psg.registers);

    h[off::RAM_SIZE..off::RAM_SIZE + 2].copy_from_slice(&ram_kb.to_le_bytes());
    h
}

/// Écrit l'état courant de `machine` dans un fichier `.sna`.
///
/// Seuls les 128 Ko standard sont enregistrés : la RAM étendue éventuelle
/// (voir `[memory] extra_ram_banks`) n'a pas de représentation dans ce
/// format, et l'inclure produirait un fichier que personne ne saurait
/// relire correctement.
pub fn save(machine: &Machine, filename: &str) -> Result<(), String> {
    const STANDARD_RAM: usize = 128 * 1024;
    let ram = &machine.bus.memory.ram;
    if ram.len() < STANDARD_RAM {
        return Err(format!(
            "RAM trop petite pour un instantané ({} octets)",
            ram.len()
        ));
    }
    let header = build_header(machine, 128);

    let mut f = File::create(filename).map_err(|e| e.to_string())?;
    f.write_all(&header).map_err(|e| e.to_string())?;
    f.write_all(&ram[..STANDARD_RAM])
        .map_err(|e| e.to_string())?;
    println!("Snapshot saved: {filename}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'en-tête doit être reconnaissable et décrire fidèlement l'état du
    /// CPU : c'est ce qui permet à un autre émulateur de reprendre
    /// exactement là où nous en étions.
    #[test]
    fn the_header_carries_the_signature_and_the_cpu_state() {
        let mut machine = Machine::new();
        machine.cpu.reg.pc = 0x1234;
        machine.cpu.reg.sp = 0xBEEF;
        machine.cpu.reg.a = 0x5A;
        machine.cpu.reg.b = 0x11;
        machine.cpu.reg.c = 0x22;
        machine.cpu.reg.ixh = 0xAB;
        machine.cpu.reg.ixl = 0xCD;

        let h = build_header(&machine, 128);

        assert_eq!(&h[..8], b"MV - SNA");
        assert_eq!(h[off::VERSION], 1);
        // Paires en petit-boutien : l'octet de poids faible d'abord.
        assert_eq!([h[off::PC], h[off::PC + 1]], [0x34, 0x12]);
        assert_eq!([h[off::SP], h[off::SP + 1]], [0xEF, 0xBE]);
        assert_eq!(h[off::AF + 1], 0x5A, "A est l'octet haut de AF");
        assert_eq!(h[off::BC], 0x22, "C est l'octet bas de BC");
        assert_eq!(h[off::BC + 1], 0x11, "B est l'octet haut de BC");
        assert_eq!([h[off::IX], h[off::IX + 1]], [0xCD, 0xAB]);
        assert_eq!([h[off::RAM_SIZE], h[off::RAM_SIZE + 1]], [128, 0]);
    }

    /// Le bit de configuration du Gate Array vaut 1 pour une ROM
    /// *désactivée* : c'est l'inverse de nos drapeaux internes, et s'y
    /// tromper produirait un instantané qui redémarre sur la mauvaise
    /// configuration mémoire.
    #[test]
    fn the_rom_configuration_bits_are_inverted_relative_to_our_flags() {
        let mut machine = Machine::new();

        machine.bus.memory.rom_low_enabled = true;
        machine.bus.memory.rom_high_enabled = true;
        let h = build_header(&machine, 128);
        assert_eq!(
            h[off::GA_ROM_CONFIG] & 0x0C,
            0x00,
            "deux ROM actives : aucun bit de desactivation"
        );

        machine.bus.memory.rom_low_enabled = false;
        machine.bus.memory.rom_high_enabled = false;
        let h = build_header(&machine, 128);
        assert_eq!(
            h[off::GA_ROM_CONFIG] & 0x0C,
            0x0C,
            "deux ROM inactives : les deux bits poses"
        );
    }

    /// Un fichier complet fait 256 octets d'en-tête plus les 128 Ko de RAM :
    /// une taille fausse est le premier symptôme d'un en-tête mal aligné.
    #[test]
    fn a_saved_file_has_the_expected_size() {
        let machine = Machine::new();
        let path = std::env::temp_dir().join("bytebox_test_snapshot.sna");
        let path = path.to_str().unwrap();
        std::fs::remove_file(path).ok();

        save(&machine, path).expect("ecriture de l'instantane");
        let len = std::fs::metadata(path).unwrap().len();
        assert_eq!(len as usize, HEADER_LEN + 128 * 1024);

        std::fs::remove_file(path).ok();
    }
}
