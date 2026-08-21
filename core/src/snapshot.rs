//! Lecture et écriture d'instantanés au format `.SNA` (le format d'échange
//! standard des émulateurs CPC).
//!
//! Utile en soi (sauvegarder l'état d'une partie), mais c'est surtout un
//! outil de diagnostic : un instantané pris chez nous se recharge dans un
//! autre émulateur, ce qui permet de transplanter un état exact et de
//! comparer les comportements à partir du même point — sans dépendre d'une
//! frappe clavier reproductible.
//!
//! La lecture ([`load`]) est venue plus tard, pour un autre usage : RASM
//! (l'assembleur Z80 de Roudoudou) sait produire directement un `.SNA`
//! prêt à tourner à partir du code assemblé, ce qui donne un cycle
//! "assemble, charge, teste" sans repasser par une image disque.
//!
//! La disposition de l'en-tête suit `t_SNA_header` de Caprice32 (256 octets,
//! structure compactée), puisque c'est lui qui relit nos fichiers — et,
//! symétriquement, la référence sur laquelle notre lecteur s'aligne.

use crate::app_log;
use crate::machine::Machine;
use std::fs::File;
use std::io::Write;
use zilog_z80::bus::Bus;

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
    /// Premier champ de la version 2 : modèle de CPC pour lequel
    /// l'instantané a été pris (0 = 464, 1 = 664, 2 = 6128...). Jamais
    /// écrit par `save` (qui déclare la version 1), seulement lu par
    /// `load` pour avertir si le fichier vise une autre machine.
    pub const CPC_MODEL: usize = 0x6D;
}

/// Valeur du champ `CPC_MODEL` correspondant au 6128, la seule machine que
/// cet émulateur reproduit.
const MODEL_6128: u8 = 2;

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
            "RAM too small for a snapshot ({} bytes)",
            ram.len()
        ));
    }
    let header = build_header(machine, 128);

    let mut f = File::create(filename).map_err(|e| e.to_string())?;
    f.write_all(&header).map_err(|e| e.to_string())?;
    f.write_all(&ram[..STANDARD_RAM])
        .map_err(|e| e.to_string())?;
    app_log!("Snapshot saved: {filename}");
    Ok(())
}

/// Restaure l'état contenu dans un fichier `.sna`.
///
/// La machine est d'abord éteinte puis rallumée (`power_cycle`), comme le
/// fait Caprice32 avant de charger : on repart ainsi d'un état connu — RAM
/// à zéro, périphériques aux valeurs par défaut, ROMs rechargées — plutôt
/// que de superposer l'instantané à ce qui tournait avant, où le moindre
/// champ absent du format resterait celui de la session précédente.
///
/// L'état matériel est ensuite restauré en REJOUANT les écritures d'I/O que
/// le programme d'origine avait faites (Gate Array, CRTC, ROM haute, PPI,
/// PSG), plutôt qu'en écrivant les champs de chaque composant. Là encore
/// c'est la méthode de Caprice32, et elle a la même vertu ici : tout l'état
/// dérivé de ces écritures (drapeaux ROM de la mémoire, ligne clavier
/// sélectionnée, moteur cassette, masques de registres PSG, redémarrage de
/// l'enveloppe...) se remet en place tout seul, sans avoir à le rejouer
/// champ par champ ni à se souvenir de ce qui découle de quoi.
pub fn load(machine: &mut Machine, filename: &str) -> Result<(), String> {
    let data = std::fs::read(filename).map_err(|e| e.to_string())?;
    if data.len() < HEADER_LEN || &data[..8] != b"MV - SNA" {
        return Err(format!("{filename} is not a .SNA snapshot"));
    }
    let version = data[off::VERSION];

    // Taille du vidage RAM, en Ko. Ramenée à un multiple de 64 comme le
    // fait Caprice32 : c'est la granularité réelle du format (64 ou 128 Ko
    // en pratique), et ça écarte du même coup les valeurs fantaisistes.
    let ram_kb = u16::from_le_bytes([data[off::RAM_SIZE], data[off::RAM_SIZE + 1]]) & !0x3F;
    if ram_kb == 0 {
        // Une taille nulle signifie que la RAM n'est pas là où on la
        // cherche : elle est découpée en blocs `MEM0`-`MEM8` après
        // l'en-tête, éventuellement compressés (variante de la version 3).
        // Caprice32 refuse ce cas, et nous aussi plutôt que de charger une
        // machine dont toute la mémoire serait restée à zéro — ce qui
        // ressemblerait à un plantage de l'émulateur, pas à un format non
        // géré.
        return Err(format!(
            "{filename}: memory stored in MEM0-MEM8 chunks (compressed v3 snapshot), which this emulator cannot read"
        ));
    }
    let ram_len = ram_kb as usize * 1024;
    if data.len() < HEADER_LEN + ram_len {
        return Err(format!(
            "{filename}: truncated, header announces {ram_kb} KB of RAM but only {} bytes follow it",
            data.len().saturating_sub(HEADER_LEN)
        ));
    }
    // Les blocs qui suivraient éventuellement le vidage RAM (version 3) sont
    // ignorés en silence : le format prévoit explicitement qu'un lecteur
    // saute ce qu'il ne connaît pas.

    // Le modèle n'existe qu'à partir de la version 2. Un avertissement, pas
    // un refus : le fichier reste chargeable, mais un programme écrit pour
    // un 464 (pas d'AMSDOS, ROM et firmware différents) n'a aucune raison
    // de tourner correctement ici, et mieux vaut le dire que laisser
    // conclure à un bug d'émulation.
    if version >= 2 {
        let model = data[off::CPC_MODEL];
        if model != MODEL_6128 {
            app_log!(
                "Snapshot was taken on CPC model {model} (0=464, 1=664, 2=6128); loading it on the emulated 6128 anyway."
            );
        }
    }

    machine.power_cycle();

    let ram = &mut machine.bus.memory.ram;
    if ram.len() < ram_len {
        return Err(format!(
            "{filename}: snapshot holds {ram_kb} KB of RAM, more than this machine has ({} KB)",
            ram.len() / 1024
        ));
    }
    ram[..ram_len].copy_from_slice(&data[HEADER_LEN..HEADER_LEN + ram_len]);

    // --- CPU ---
    let reg = &mut machine.cpu.reg;
    reg.flags.set_from_byte(data[off::AF]);
    reg.a = data[off::AF + 1];
    reg.c = data[off::BC];
    reg.b = data[off::BC + 1];
    reg.e = data[off::DE];
    reg.d = data[off::DE + 1];
    reg.l = data[off::HL];
    reg.h = data[off::HL + 1];
    reg.r = data[off::R];
    reg.i = data[off::I];
    reg.ixl = data[off::IX];
    reg.ixh = data[off::IX + 1];
    reg.iyl = data[off::IY];
    reg.iyh = data[off::IY + 1];
    reg.sp = u16::from_le_bytes([data[off::SP], data[off::SP + 1]]);
    reg.pc = u16::from_le_bytes([data[off::PC], data[off::PC + 1]]);

    let alt = &mut machine.cpu.alt;
    alt.flags.set_from_byte(data[off::AFX]);
    alt.a = data[off::AFX + 1];
    alt.c = data[off::BCX];
    alt.b = data[off::BCX + 1];
    alt.e = data[off::DEX];
    alt.d = data[off::DEX + 1];
    alt.l = data[off::HLX];
    alt.h = data[off::HLX + 1];

    // Le format nomme ses deux champs IFF0/IFF1 là où nous disons IFF1/IFF2
    // (voir doc/sna-format.md) : l'ordre ci-dessous est le bon malgré
    // l'apparence de décalage.
    machine
        .cpu
        .set_interrupt_state(data[off::IM], data[off::IFF0] != 0, data[off::IFF1] != 0);

    // --- Gate Array ---
    // Une encre se pose en deux temps, exactement comme le ferait le
    // programme d'origine : sélection du stylo, puis écriture de la couleur.
    for pen in 0..17u8 {
        // Le stylo 16 (la bordure) ne se sélectionne pas par son numéro mais
        // par le bit 4 — voir `GateArray::write_register`.
        let select = if pen == 16 { 0x10 } else { pen };
        machine.bus.write_io(0x7F00, select);
        machine.bus.write_io(0x7F00, 0x40 | (data[off::GA_INK + pen as usize] & 0x1F));
    }
    // Stylo réellement sélectionné au moment de la capture, une fois toute
    // la palette posée (sinon la boucle ci-dessus l'écraserait).
    machine.bus.write_io(0x7F00, data[off::GA_PEN] & 0x3F);
    machine
        .bus
        .write_io(0x7F00, 0x80 | (data[off::GA_ROM_CONFIG] & 0x3F));
    machine
        .bus
        .write_io(0x7F00, 0xC0 | (data[off::GA_RAM_CONFIG] & 0x3F));

    // --- CRTC ---
    for r in 0..18u8 {
        machine.bus.write_io(0xBC00, r);
        machine
            .bus
            .write_io(0xBD00, data[off::CRTC_REGISTERS + r as usize]);
    }
    machine.bus.write_io(0xBC00, data[off::CRTC_REG_SELECT]);

    // --- ROM haute ---
    machine.bus.write_io(0xDF00, data[off::UPPER_ROM]);

    // --- PPI ---
    // Le registre de contrôle EN PREMIER, contrairement à Caprice32 qui
    // l'écrit en dernier : chez nous (comme sur un vrai 8255) le configurer
    // remet les ports A et C à zéro — comportement délibéré, exigé par
    // Barbarian, voir `Ppi::write_register`. L'écrire après les ports
    // effacerait donc ce qu'on vient de restaurer.
    machine.bus.write_io(0xF700, data[off::PPI_CONTROL]);
    machine.bus.write_io(0xF400, data[off::PPI_A]);
    machine.bus.write_io(0xF600, data[off::PPI_C]);
    // Le port B est DÉLIBÉRÉMENT laissé tel quel. C'est une entrée : il ne
    // porte pas de l'état programme mais le câblage de CETTE machine —
    // straps constructeur (bits 1-3 : c'est ce qui fait afficher "Amstrad"
    // au démarrage plutôt que Triumph, Saisho ou Solavox), fréquence
    // secteur, plus des signaux vivants recalculés en permanence (VSYNC,
    // données cassette). Le restaurer depuis un fichier revenait à laisser
    // un instantané reconfigurer l'identité de la machine : constaté en
    // pratique, un instantané forgé avec un port B à zéro faisait démarrer
    // le CPC sous une autre marque. Caprice32 ne le restaure pas non plus
    // (il l'écrit via un OUT sur &F5xx, sans effet sur les lignes d'entrée).

    // --- PSG ---
    // Passe par `write_current_register` plutôt que par le tableau : c'est
    // lui qui applique les masques propres à chaque registre, et qui
    // redémarre le générateur d'enveloppe sur R13.
    for r in 0..16u8 {
        machine.bus.psg.selected_register = r;
        machine
            .bus
            .psg
            .write_current_register(data[off::PSG_REGISTERS + r as usize]);
    }
    machine.bus.psg.selected_register = data[off::PSG_REG_SELECT];

    // L'historique par scanline décrit encore la trame d'avant le
    // chargement : le remettre à l'état restauré évite que la première trame
    // affichée mélange l'ancienne palette et la nouvelle image.
    let state = machine.bus.gate_array.state();
    machine.scanline_states.fill(state);
    for slot in &mut machine.scanline_vram {
        slot.clear();
    }

    app_log!("Snapshot loaded: {filename} (version {version}, {ram_kb} KB)");
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

    /// L'aller-retour complet : ce qu'on écrit doit se relire à l'identique.
    /// C'est le test qui couvre le plus de terrain d'un coup — un offset
    /// faux, un couple d'octets inversé ou un composant oublié dans `load`
    /// se voit ici, sans avoir à écrire un test par champ.
    #[test]
    fn a_saved_snapshot_reloads_to_the_same_state() {
        let mut machine = Machine::new();
        machine.cpu.reg.pc = 0x1234;
        machine.cpu.reg.sp = 0xBEEF;
        machine.cpu.reg.a = 0x5A;
        machine.cpu.reg.set_ix(0xABCD);
        machine.cpu.alt.b = 0x77;
        machine.cpu.set_interrupt_state(2, true, false);
        // Un octet témoin dans une banque que le paging ne déplace pas.
        machine.bus.memory.ram[0x1000] = 0xA5;
        machine.bus.gate_array.palette[3] = 0x1A;
        machine.bus.gate_array.video_mode = 2;
        machine.bus.crtc.registers[6] = 24;
        machine.bus.psg.registers[7] = 0x38;

        let path = std::env::temp_dir().join("bytebox_test_roundtrip.sna");
        let path = path.to_str().unwrap();
        std::fs::remove_file(path).ok();
        save(&machine, path).expect("ecriture de l'instantane");

        // Une machine repartie de zéro, pour qu'aucune valeur ne puisse
        // "survivre" par hasard et faire passer le test sans rien restaurer.
        let mut reloaded = Machine::new();
        load(&mut reloaded, path).expect("lecture de l'instantane");

        assert_eq!(reloaded.cpu.reg.pc, 0x1234);
        assert_eq!(reloaded.cpu.reg.sp, 0xBEEF);
        assert_eq!(reloaded.cpu.reg.a, 0x5A);
        assert_eq!(reloaded.cpu.reg.get_ix(), 0xABCD);
        assert_eq!(reloaded.cpu.alt.b, 0x77);
        assert_eq!(reloaded.cpu.im(), 2);
        assert!(reloaded.cpu.iff1());
        assert!(!reloaded.cpu.iff2());
        assert_eq!(reloaded.bus.memory.ram[0x1000], 0xA5);
        assert_eq!(reloaded.bus.gate_array.palette[3], 0x1A);
        assert_eq!(reloaded.bus.gate_array.video_mode, 2);
        assert_eq!(reloaded.bus.crtc.registers[6], 24);
        assert_eq!(reloaded.bus.psg.registers[7], 0x38);

        std::fs::remove_file(path).ok();
    }

    /// L'épreuve du feu, avec de vraies ROMs : on laisse le CPC démarrer
    /// jusqu'au BASIC, on l'enregistre, on recharge dans une machine neuve,
    /// puis on fait tourner les deux en parallèle et on compare l'image
    /// produite.
    ///
    /// C'est ce qu'aucun test unitaire ne peut montrer : que l'état restauré
    /// est non seulement identique champ à champ, mais qu'il REPART
    /// correctement — un CRTC à moitié restauré ou une bascule
    /// d'interruption oubliée donne deux images qui divergent au bout de
    /// quelques trames, même si tous les champs se relisent bien.
    ///
    /// Ignoré par défaut : il émule plusieurs secondes de CPC et demande les
    /// ROMs. `cargo test --release snapshot -- --ignored`.
    #[test]
    #[ignore]
    fn a_reloaded_snapshot_keeps_producing_the_same_picture() {
        let mut original = Machine::new();
        if original.load_roms().is_err() {
            println!("ROMs absentes : test ignore");
            return;
        }
        // Assez long pour dépasser l'écran de démarrage et atteindre le
        // prompt BASIC, curseur clignotant compris.
        let mut t = 0u64;
        while t < 3 * 4_000_000 {
            t += original.step() as u64;
        }

        let path = std::env::temp_dir().join("bytebox_test_live_roundtrip.sna");
        let path = path.to_str().unwrap();
        std::fs::remove_file(path).ok();
        save(&original, path).expect("ecriture de l'instantane");

        let mut reloaded = Machine::new();
        reloaded.load_roms().expect("rechargement des ROMs");
        load(&mut reloaded, path).expect("lecture de l'instantane");

        // Les deux machines avancent du même nombre de trames, puis doivent
        // afficher exactement la même chose.
        let px = crate::video::SCREEN_WIDTH * crate::video::SCREEN_HEIGHT * 3;
        let mut frame_original = vec![0u8; px];
        let mut frame_reloaded = vec![0u8; px];
        for frame in 0..8 {
            for machine in [&mut original, &mut reloaded] {
                machine.frame_ready = false;
                let mut guard = 0u32;
                while !machine.frame_ready && guard < 200_000 {
                    machine.step();
                    guard += 1;
                }
            }
            crate::video::render(&original, &mut frame_original);
            crate::video::render(&reloaded, &mut frame_reloaded);
            assert_eq!(
                frame_original, frame_reloaded,
                "l'image diverge a la trame {frame} apres rechargement"
            );
        }

        // Garde-fou contre un test qui ne prouverait rien : deux écrans
        // vides seraient identiques eux aussi. On exige donc que l'image
        // comparée contienne vraiment quelque chose — l'écran BASIC a du
        // texte jaune sur fond bleu, donc plusieurs couleurs distinctes.
        let distinct: std::collections::HashSet<[u8; 3]> = frame_original
            .chunks_exact(3)
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        assert!(
            distinct.len() > 1,
            "l'ecran compare est uniforme : le test ne prouverait rien"
        );

        std::fs::remove_file(path).ok();
    }

    /// Le port B du PPI porte le câblage de la machine, pas de l'état
    /// programme : ses bits 1-3 sont les straps constructeur, ceux qui font
    /// afficher "Amstrad" au démarrage. Un instantané ne doit donc PAS
    /// pouvoir les changer — une première version de `load` les restaurait,
    /// et un fichier au port B nul faisait démarrer le CPC sous une autre
    /// marque.
    #[test]
    fn loading_never_lets_a_snapshot_rewire_the_machine_identity() {
        let mut header = [0u8; HEADER_LEN];
        header[..8].copy_from_slice(b"MV - SNA");
        header[off::VERSION] = 1;
        header[off::RAM_SIZE..off::RAM_SIZE + 2].copy_from_slice(&128u16.to_le_bytes());
        header[off::PPI_B] = 0x00; // straps constructeur à zéro

        let path = std::env::temp_dir().join("bytebox_test_portb.sna");
        let path = path.to_str().unwrap();
        let mut file = header.to_vec();
        file.extend(std::iter::repeat_n(0u8, 128 * 1024));
        std::fs::write(path, file).unwrap();

        let mut machine = Machine::new();
        let expected = machine.bus.ppi.port_b_input;
        load(&mut machine, path).expect("lecture de l'instantane");

        assert_eq!(
            (machine.bus.ppi.port_b_input >> 1) & 0x07,
            (expected >> 1) & 0x07,
            "les straps constructeur ont ete ecrases par l'instantane"
        );

        std::fs::remove_file(path).ok();
    }

    /// Un fichier qui n'est pas un instantané doit être refusé franchement,
    /// pas chargé à moitié : sans la signature, tout le reste ne serait
    /// qu'une réinterprétation d'octets quelconques.
    #[test]
    fn a_file_without_the_signature_is_refused() {
        let path = std::env::temp_dir().join("bytebox_test_notasnapshot.sna");
        let path = path.to_str().unwrap();
        std::fs::write(path, vec![0u8; HEADER_LEN + 1024]).unwrap();

        let mut machine = Machine::new();
        assert!(load(&mut machine, path).is_err());

        std::fs::remove_file(path).ok();
    }

    /// Taille de RAM nulle = mémoire rangée dans des blocs MEM0-MEM8
    /// (version 3 compressée), que nous ne savons pas lire. Doit être une
    /// erreur explicite, surtout pas un chargement silencieux qui donnerait
    /// une machine à la mémoire entièrement vide.
    #[test]
    fn a_chunked_v3_snapshot_is_refused_rather_than_loaded_empty() {
        let mut header = [0u8; HEADER_LEN];
        header[..8].copy_from_slice(b"MV - SNA");
        header[off::VERSION] = 3;
        // RAM_SIZE laissé à zéro : c'est ce qui signale les blocs MEM*.

        let path = std::env::temp_dir().join("bytebox_test_chunked.sna");
        let path = path.to_str().unwrap();
        std::fs::write(path, header).unwrap();

        let mut machine = Machine::new();
        let err = load(&mut machine, path).expect_err("un v3 chunke doit etre refuse");
        assert!(err.contains("MEM0-MEM8"), "message peu explicite : {err}");

        std::fs::remove_file(path).ok();
    }
}
