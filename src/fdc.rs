use std::fs::File;
use std::io::{Read, Write};

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FdcPhase {
    Command,
    ExecutionRead,
    ExecutionWrite,
    Result,
}

#[derive(Clone)]
pub struct Sector {
    pub id: u8,
    pub size: usize,
    pub data: Vec<u8>,
    /// Vrai si ce secteur a été enregistré avec la marque d'adresse "Deleted
    /// Data" (bit 6 de ST2 dans l'en-tête de piste du .dsk), plutôt que la
    /// marque normale. Un vrai contrôleur µPD765A distingue les deux via les
    /// commandes Read Data (0x06, données normales) et Read Deleted Data
    /// (0x0C, données effacées) : plusieurs protections CPC marquent
    /// volontairement un ou deux secteurs "deleted" sur une piste donnée,
    /// lisibles uniquement par la seconde commande, pour détecter une copie
    /// qui ne préserverait pas cette marque.
    pub deleted: bool,
}

pub struct Track {
    pub number: u8,
    pub side: u8,
    pub sector_size: u8,
    pub sectors: Vec<Sector>,
}

pub struct DskImage {
    pub tracks: Vec<Track>,
}

impl DskImage {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 0x100 {
            return Err("Fichier DSK trop court".to_string());
        }

        let signature = std::str::from_utf8(&data[0..8]).unwrap_or("");

        if signature.starts_with("MV - CPC") {
            Self::parse_standard(data)
        } else if signature.starts_with("EXTENDED") {
            Self::parse_extended(data)
        } else {
            Err("Format DSK non reconnu (signature invalide)".to_string())
        }
    }

    /// Lecture sécurisée d'un octet : ne panique jamais sur un fichier corrompu.
    fn get_u8(data: &[u8], offset: usize) -> Result<u8, String> {
        data.get(offset).copied().ok_or_else(|| {
            format!(
                "DSK corrompu : lecture hors limites à l'offset {:#X}",
                offset
            )
        })
    }

    fn parse_standard(data: &[u8]) -> Result<Self, String> {
        let num_tracks = Self::get_u8(data, 0x30)?;
        let num_sides = Self::get_u8(data, 0x31)?.max(1);
        let track_size =
            u16::from_le_bytes([Self::get_u8(data, 0x32)?, Self::get_u8(data, 0x33)?]) as usize;

        if track_size == 0 {
            return Err("Taille de piste nulle dans l'en-tête DSK".to_string());
        }

        let mut tracks = Vec::new();
        let mut offset = 0x100usize;

        for _t in 0..num_tracks {
            for _s in 0..num_sides {
                if offset + 0x100 > data.len() {
                    break;
                }
                let track_header = &data[offset..offset + 0x100];
                let track = Self::parse_track_header(track_header, offset + 0x100, data, false)?;
                tracks.push(track);
                offset += track_size;
            }
        }

        Ok(DskImage { tracks })
    }

    fn parse_extended(data: &[u8]) -> Result<Self, String> {
        let num_tracks = Self::get_u8(data, 0x30)?;
        let num_sides = Self::get_u8(data, 0x31)?.max(1);

        let mut tracks = Vec::new();
        let mut offset = 0x100usize;

        for t in 0..num_tracks {
            for s in 0..num_sides {
                let size_table_offset = 0x34 + (t as usize * num_sides as usize) + s as usize;
                let track_size_code = Self::get_u8(data, size_table_offset)?;
                let track_size = (track_size_code as usize) * 256;
                if track_size == 0 {
                    // Piste non formatée : le format Extended DSK ne stocke aucune
                    // donnée pour elle dans le fichier (offset non avancé).
                    continue;
                }

                if offset + 0x100 > data.len() {
                    break;
                }
                let track_header = &data[offset..offset + 0x100];
                let track = Self::parse_track_header(track_header, offset + 0x100, data, true)?;
                tracks.push(track);
                offset += track_size;
            }
        }

        Ok(DskImage { tracks })
    }

    /// Analyse un en-tête de piste (256 octets), commun aux deux formats DSK.
    /// `extended` détermine si les octets "taille réelle" (utilisés uniquement en
    /// Extended DSK, pour les secteurs de taille non standard) doivent être pris
    /// en compte.
    fn parse_track_header(
        track_header: &[u8],
        sector_data_start: usize,
        data: &[u8],
        extended: bool,
    ) -> Result<Track, String> {
        let track_num = track_header[0x10];
        let side_num = track_header[0x11];
        let sec_size_code = track_header[0x14];
        // Borne de sécurité : un CPC n'a jamais plus d'une trentaine de secteurs
        // par piste. Ça évite qu'un fichier corrompu/malveillant fasse déborder
        // la table de secteurs (256 octets) de l'en-tête.
        let num_sectors = track_header[0x15].min(29);

        let mut sectors = Vec::new();
        let mut sector_data_offset = sector_data_start;

        for sec_idx in 0..num_sectors {
            let info_offset = 0x18 + (sec_idx as usize * 8);
            if info_offset + 8 > track_header.len() {
                break;
            }

            let sec_id = track_header[info_offset + 2];
            let sec_size_code_inf = track_header[info_offset + 3].min(6); // 128<<6 = 8Ko, plafond réaliste
            let declared_size = 128usize << sec_size_code_inf;

            let actual_size = if extended {
                let lo = track_header[info_offset + 6];
                let hi = track_header[info_offset + 7];
                let sz = u16::from_le_bytes([lo, hi]) as usize;
                if sz > 0 { sz } else { declared_size }
            } else {
                declared_size
            };

            if sector_data_offset + actual_size > data.len() {
                break;
            }
            let sec_data = data[sector_data_offset..sector_data_offset + actual_size].to_vec();
            let st2 = track_header[info_offset + 5];

            sectors.push(Sector {
                id: sec_id,
                size: actual_size,
                data: sec_data,
                deleted: (st2 & 0x40) != 0,
            });
            sector_data_offset += actual_size;
        }

        Ok(Track {
            number: track_num,
            side: side_num,
            sector_size: sec_size_code,
            sectors,
        })
    }
}

/// Durée d'un tour de disquette, en cycles Z80 : les lecteurs 3" du CPC
/// tournent à 300 tr/min, soit 200 ms, soit 800 000 cycles à 4 MHz. C'est
/// l'unité de temps de tout ce qui dépend de la rotation (attente d'un
/// identifiant de secteur sous la tête).
pub const REVOLUTION_TICKS: u32 = 800_000;

/// Durée d'un octet sur la piste, en cycles Z80 : 250 kbit/s en MFM, soit
/// 32 µs par octet, soit 128 cycles à 4 MHz. Un tour de piste vaut donc
/// environ 6250 octets bruts.
const BYTE_TICKS: u64 = 128;

/// Octets "de service" écrits autour des données de chaque secteur :
/// synchronisation, en-tête d'identification et son CRC, marque de données,
/// CRC des données et intervalle jusqu'au secteur suivant. Une soixantaine
/// d'octets avec les valeurs d'intervalle habituelles du CPC.
const SECTOR_OVERHEAD_BYTES: u64 = 62;

/// État propre à un lecteur de disquette physique : position de la tête,
/// disque chargé, image `.dsk` en mémoire. Le CPC 6128 peut piloter jusqu'à
/// deux lecteurs (A et B), partageant le même contrôleur FDC (voir `Fdc`
/// ci-dessous), exactement comme sur le matériel réel où un seul chip
/// µPD765A gère plusieurs lecteurs via le champ "Drive Select".
pub struct Drive {
    pub current_track: u8,
    pub current_sector: u8,
    pub current_side: u8,
    pub disk_loaded: bool,
    pub current_filename: String,
    pub dsk: Option<DskImage>,
}

impl Drive {
    pub fn new() -> Self {
        Self {
            current_track: 0,
            current_sector: 0xC1, // Premier secteur standard d'une disquette CPC (système AMSDOS)
            current_side: 0,
            disk_loaded: false,
            current_filename: "None".to_string(),
            dsk: None,
        }
    }

    fn reset_position(&mut self) {
        self.current_track = 0;
        self.current_sector = 0xC1;
        self.current_side = 0;
    }
}

impl Default for Drive {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Fdc {
    pub phase: FdcPhase,
    pub command_buffer: Vec<u8>,
    pub command_len: usize,
    pub result_buffer: Vec<u8>,
    pub result_index: usize,

    // --- Lecteurs physiques ---
    pub drive_a: Drive,
    pub drive_b: Drive,
    /// Le lecteur B n'est disponible que si activé dans la configuration
    /// utilisateur (config.toml, section [drives], clé drive_b).
    pub drive_b_enabled: bool,
    /// Lecteur ciblé par la dernière commande décodée (0 = A, 1 = B), déduit
    /// du bit US0 (bit 0) du champ "Drive/HD" présent dans la plupart des
    /// commandes du FDC.
    pub selected_drive: u8,

    pub motor_on: bool,

    // Execution phase (transfert de données secteur)
    pub execution_buffer: Vec<u8>,
    pub execution_index: usize,

    // Status registers (renvoyés dans les phases de résultat)
    pub st0: u8,

    /// Vrai après un Seek/Recalibrate tant que "Sense Interrupt Status" n'a pas
    /// encore été exécuté par le logiciel (comportement du vrai µPD765A : appeler
    /// cette commande sans interruption en attente renvoie "invalid command").
    pub seek_interrupt_pending: bool,

    /// Cycles Z80 restants avant que le contrôleur ne rende la main. Tant
    /// qu'ils ne sont pas écoulés, le MSR annonce "occupé, rien à
    /// transférer" (RQM=0, CB=1), comme un vrai FDC qui attend que le
    /// secteur voulu se présente sous la tête.
    pub busy_ticks: u32,

    /// Temps écoulé depuis la mise sous tension, en cycles Z80. Sert
    /// d'horloge de rotation : la position angulaire de la disquette est
    /// `time % REVOLUTION_TICKS`, ce qui suffit à savoir quel identifiant
    /// de secteur se présentera ensuite sous la tête.
    pub time: u64,

    // --- État de la commande Format Track (0x0D) ---
    pub formatting: bool,
    pub format_n: u8,
    pub format_sc: u8,
    pub format_fill: u8,
}

impl Fdc {
    pub fn new() -> Self {
        Self {
            phase: FdcPhase::Command,
            command_buffer: Vec::new(),
            command_len: 0,
            result_buffer: Vec::new(),
            result_index: 0,
            drive_a: Drive::new(),
            drive_b: Drive::new(),
            drive_b_enabled: false,
            selected_drive: 0,
            motor_on: false,
            execution_buffer: Vec::new(),
            execution_index: 0,
            st0: 0,
            seek_interrupt_pending: false,
            busy_ticks: 0,
            time: 0,
            formatting: false,
            format_n: 0,
            format_sc: 0,
            format_fill: 0xE5,
        }
    }

    /// Accès immuable au lecteur actuellement sélectionné.
    pub fn drive(&self) -> &Drive {
        if self.selected_drive == 1 {
            &self.drive_b
        } else {
            &self.drive_a
        }
    }

    /// Accès mutable au lecteur actuellement sélectionné.
    pub fn drive_mut(&mut self) -> &mut Drive {
        if self.selected_drive == 1 {
            &mut self.drive_b
        } else {
            &mut self.drive_a
        }
    }

    /// Indique si le lecteur actuellement sélectionné est physiquement
    /// disponible : le lecteur A est toujours présent, le lecteur B
    /// uniquement s'il a été activé via config.toml.
    /// Bits d'identification renvoyés dans ST0 à la fin d'un Seek ou d'un
    /// Recalibrate : numéro de lecteur (US1/US0) et tête courante (HD, bit 2).
    /// Un pilote qui gère plusieurs lecteurs ou les deux faces s'en sert pour
    /// savoir quel déplacement vient de s'achever.
    fn seek_st0_unit_bits(&self) -> u8 {
        (self.selected_drive & 0x03) | ((self.drive().current_side & 0x01) << 2)
    }

    fn selected_drive_available(&self) -> bool {
        self.selected_drive == 0 || self.drive_b_enabled
    }

    /// Active ou désactive la disponibilité du lecteur B (piloté par
    /// config.toml, section [drives], clé drive_b).
    pub fn set_drive_b_enabled(&mut self, enabled: bool) {
        self.drive_b_enabled = enabled;
    }

    /// Réinitialise l'état transitoire (phase, tampons de commande/résultat/exécution).
    /// Appelé lors du chargement/éjection d'une disquette pour éviter qu'une commande
    /// FDC en cours ne se retrouve dans un état incohérent après un changement de média.
    fn reset_transient_state(&mut self) {
        self.phase = FdcPhase::Command;
        self.command_buffer.clear();
        self.command_len = 0;
        self.result_buffer.clear();
        self.result_index = 0;
        self.execution_buffer.clear();
        self.execution_index = 0;
        self.seek_interrupt_pending = false;
        self.formatting = false;
        self.st0 = 0;
        self.busy_ticks = 0;
    }

    fn load_disk_into(drive: &mut Drive, filename: &str) -> Result<(), String> {
        let mut f = File::open(filename).map_err(|e| e.to_string())?;
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        let dsk = DskImage::parse(&buffer)?;
        drive.dsk = Some(dsk);
        drive.disk_loaded = true;
        drive.current_filename = filename.to_string();
        drive.reset_position();
        Ok(())
    }

    /// Charge un fichier disquette .dsk sur le lecteur A.
    ///
    /// N'imprime que le message : réafficher le prompt "> " qui suit est la
    /// responsabilité de l'appelant, qui seul sait s'il s'agit d'une
    /// commande console (déjà couverte par le réaffichage de `sdl::run`
    /// après `Machine::console_handle`) ou d'un chargement hors-bande (voir
    /// `console::notice`, utilisée par exemple au démarrage pour `--disk`).
    pub fn load_disk(&mut self, filename: &str) -> Result<(), String> {
        Self::load_disk_into(&mut self.drive_a, filename)?;
        self.reset_transient_state();
        println!("Floppy DSK Loaded on drive A: {}", filename);
        Ok(())
    }

    /// Charge un fichier disquette .dsk sur le lecteur B.
    /// Échoue si le lecteur B n'a pas été activé dans config.toml.
    pub fn load_disk_b(&mut self, filename: &str) -> Result<(), String> {
        if !self.drive_b_enabled {
            return Err(
                "Le lecteur B n'est pas activé dans la configuration (config.toml : [drives] drive_b = true)"
                    .to_string(),
            );
        }
        Self::load_disk_into(&mut self.drive_b, filename)?;
        self.reset_transient_state();
        println!("Floppy DSK Loaded on drive B: {}", filename);
        Ok(())
    }

    /// Éjecte la disquette du lecteur A.
    pub fn eject_disk(&mut self) {
        self.drive_a.dsk = None;
        self.drive_a.disk_loaded = false;
        self.drive_a.current_filename = "None".to_string();
        self.reset_transient_state();
        println!("Floppy DSK Ejected from drive A");
    }

    /// Éjecte la disquette du lecteur B.
    pub fn eject_disk_b(&mut self) {
        if !self.drive_b_enabled {
            println!("Le lecteur B n'est pas activé dans la configuration (config.toml)");
            return;
        }
        self.drive_b.dsk = None;
        self.drive_b.disk_loaded = false;
        self.drive_b.current_filename = "None".to_string();
        self.reset_transient_state();
        println!("Floppy DSK Ejected from drive B");
    }

    /// Crée une disquette vierge, formatée AMSDOS standard (40 pistes, une
    /// face, 9 secteurs de 512 octets numérotés 0xC1-0xC9, remplis de l'octet
    /// de bourrage habituel de Format Track). Rien n'est écrit dans un
    /// fichier ici, seulement construit en mémoire — voir
    /// [`Fdc::write_dsk_file`] pour la persister.
    pub fn blank_dsk_image() -> DskImage {
        const NUM_TRACKS: u8 = 40;
        const NUM_SECTORS: u8 = 9;
        const SECTOR_SIZE: usize = 512;
        const FILL: u8 = 0xE5;

        let tracks = (0..NUM_TRACKS)
            .map(|number| Track {
                number,
                side: 0,
                sector_size: 2, // 128 << 2 = 512 octets
                sectors: (0..NUM_SECTORS)
                    .map(|i| Sector {
                        id: 0xC1 + i,
                        size: SECTOR_SIZE,
                        data: vec![FILL; SECTOR_SIZE],
                        deleted: false,
                    })
                    .collect(),
            })
            .collect();

        DskImage { tracks }
    }

    /// Écrit une image disquette en mémoire vers un fichier .dsk (format
    /// standard, non-Extended).
    pub fn write_dsk_file(dsk: &DskImage, filename: &str) -> Result<(), String> {
        let num_tracks = dsk
            .tracks
            .iter()
            .map(|t| t.number)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        let num_sides = dsk
            .tracks
            .iter()
            .map(|t| t.side)
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);

        let max_track_payload = dsk
            .tracks
            .iter()
            .map(|t| t.sectors.iter().map(|s| s.size).sum::<usize>())
            .max()
            .unwrap_or(0);
        let track_size = 0x100 + max_track_payload;

        let mut out = Vec::new();

        // En-tête disque (0x100 octets)
        let mut header = vec![0u8; 0x100];
        header[0..8].copy_from_slice(b"MV - CPC");
        header[0x30] = num_tracks;
        header[0x31] = num_sides;
        header[0x32..0x34].copy_from_slice(&(track_size as u16).to_le_bytes());
        out.extend_from_slice(&header);

        for t_num in 0..num_tracks {
            for s_num in 0..num_sides {
                let mut track_block = vec![0u8; track_size];
                track_block[0..12].copy_from_slice(b"Track-Info\r\n");

                if let Some(track) = dsk
                    .tracks
                    .iter()
                    .find(|t| t.number == t_num && t.side == s_num)
                {
                    track_block[0x10] = track.number;
                    track_block[0x11] = track.side;
                    track_block[0x14] = track.sector_size;
                    track_block[0x15] = track.sectors.len() as u8;

                    let mut data_offset = 0x100usize;
                    for (i, sec) in track.sectors.iter().enumerate() {
                        let info_offset = 0x18 + i * 8;
                        if info_offset + 4 <= 0x100 {
                            track_block[info_offset] = track.number;
                            track_block[info_offset + 1] = track.side;
                            track_block[info_offset + 2] = sec.id;
                            let n = ((sec.size.max(128)) / 128).trailing_zeros() as u8;
                            track_block[info_offset + 3] = n;
                        }
                        let end = (data_offset + sec.size).min(track_block.len());
                        if data_offset < end {
                            let copy_len = end - data_offset;
                            track_block[data_offset..end].copy_from_slice(&sec.data[..copy_len]);
                        }
                        data_offset += sec.size;
                    }
                }

                out.extend_from_slice(&track_block);
            }
        }

        let mut f = File::create(filename).map_err(|e| e.to_string())?;
        f.write_all(&out).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Fait avancer le temps du contrôleur de `cpu_ticks` cycles Z80.
    pub fn tick(&mut self, cpu_ticks: u32) {
        self.time = self.time.wrapping_add(cpu_ticks as u64);
        self.busy_ticks = self.busy_ticks.saturating_sub(cpu_ticks);
    }

    /// Temps qui sépare deux identifiants de secteur consécutifs sur une
    /// piste, en cycles Z80.
    ///
    /// Ce n'est PAS un tour divisé par le nombre de secteurs : les secteurs
    /// n'occupent qu'une partie du tour (environ 85 % au format CPC
    /// habituel), le reste étant l'intervalle final qui précède le trou
    /// d'index. On calcule donc l'espacement réel à partir de la taille des
    /// secteurs, plafonné à un tour complet pour une piste anormalement
    /// chargée.
    fn sector_pitch_ticks(&self, track: u8, side: u8) -> u64 {
        let drv = self.drive();
        let (count, bytes) = drv
            .dsk
            .as_ref()
            .and_then(|dsk| {
                dsk.tracks
                    .iter()
                    .find(|t| t.number == track && t.side == side)
            })
            .map_or((0, 0), |t| {
                (
                    t.sectors.len() as u64,
                    t.sectors
                        .iter()
                        .map(|s| s.size as u64 + SECTOR_OVERHEAD_BYTES)
                        .sum::<u64>(),
                )
            });

        if count == 0 {
            return REVOLUTION_TICKS as u64;
        }
        let used = (bytes * BYTE_TICKS).min(REVOLUTION_TICKS as u64);
        (used / count).max(1)
    }

    /// Arme un délai avant la phase de résultat, en cycles Z80.
    fn set_busy(&mut self, ticks: u32) {
        self.busy_ticks = ticks;
    }

    /// Lecture du registre de statut (MSR) sur le port &FB7E
    pub fn read_msr(&self) -> u8 {
        // Commande en cours d'exécution : le contrôleur est occupé et n'a
        // rien à transférer. Sans cet état, toute commande semble aboutir
        // instantanément — ce qui casse les logiciels qui MESURENT le temps
        // de réponse du FDC pour en déduire la géométrie d'une piste (le
        // copieur de Discology compte ses interrogations du MSR jusqu'à
        // avoir couvert un tour de disquette, voir
        // doc/discology-copie.md).
        if self.busy_ticks > 0 {
            return 0x10; // CB seul : occupé, RQM=0
        }

        let mut msr = 0x00;

        // Bit 7: RQM (Request for Master) - toujours prêt à communiquer
        msr |= 0x80;

        // Bit 6: DIO (Data Input/Output) - sens du transfert
        // 0 = CPU -> FDC, 1 = FDC -> CPU
        if self.phase == FdcPhase::Result || self.phase == FdcPhase::ExecutionRead {
            msr |= 0x40;
        }

        // Bit 5: EXM (Execution Mode) - actif pendant les phases de transfert direct
        if self.phase == FdcPhase::ExecutionRead || self.phase == FdcPhase::ExecutionWrite {
            msr |= 0x20;
        }

        // Bit 4: CB (FDC Busy) - occupé pendant une commande ou un résultat
        if self.phase != FdcPhase::Command {
            msr |= 0x10;
        }

        // Bits 3-0 (DxB, "lecteur X occupé en seek") : nos seeks sont instantanés
        // dans cette émulation, donc ces bits ne sont jamais observables à 1 par le
        // logiciel — on ne les positionne pas (contrairement à l'ancienne version qui
        // y stockait à tort l'état "disque chargé", sans rapport avec leur vraie
        // signification matérielle).

        msr
    }

    /// Écriture d'un octet de données (port &FB7F)
    pub fn write_data(&mut self, val: u8) {
        if self.phase != FdcPhase::Command {
            if self.phase == FdcPhase::ExecutionWrite {
                if self.formatting {
                    // Phase d'exécution de Format Track : on accumule des groupes de
                    // 4 octets (C, H, R, N) décrivant chaque secteur à créer.
                    self.execution_buffer.push(val);
                    self.execution_index += 1;
                    let expected = self.format_sc as usize * 4;
                    if expected > 0 && self.execution_index >= expected {
                        self.finish_format_command();
                    }
                } else {
                    // Phase d'écriture de données secteur. La taille attendue est
                    // dérivée du champ N (command_buffer[5]) de la commande Write
                    // Data en cours, et non plus figée à 512 octets.
                    self.execution_buffer.push(val);
                    self.execution_index += 1;
                    let n = *self.command_buffer.get(5).unwrap_or(&2);
                    let expected_size = 128usize << n.min(6);
                    if self.execution_index >= expected_size {
                        self.finish_write_command();
                    }
                }
            }
            return;
        }

        if self.command_buffer.is_empty() {
            // Premier octet de commande
            self.command_buffer.push(val);
            self.command_len = match val & 0x1F {
                0x03 => 3, // Specify
                0x04 => 2, // Sense Drive Status
                0x07 => 2, // Recalibrate
                0x0F => 3, // Seek
                0x08 => 1, // Sense Interrupt Status
                0x0A => 2, // Read ID
                0x06 => 9, // Read Data
                0x0C => 9, // Read Deleted Data
                0x05 => 9, // Write Data
                0x0D => 6, // Format Track
                _ => 1,    // Par défaut, commandes inconnues à 1 octet
            };
        } else {
            self.command_buffer.push(val);
        }

        if self.command_buffer.len() >= self.command_len {
            self.execute_command();
        }
    }

    /// Lecture d'un octet de données (port &FB7F)
    pub fn read_data(&mut self) -> u8 {
        match self.phase {
            FdcPhase::ExecutionRead => {
                if self.execution_index < self.execution_buffer.len() {
                    let val = self.execution_buffer[self.execution_index];
                    self.execution_index += 1;
                    if self.execution_index >= self.execution_buffer.len() {
                        // Fin de transfert du secteur, on passe à la phase de résultat
                        self.phase = FdcPhase::Result;
                    }
                    val
                } else {
                    self.phase = FdcPhase::Result;
                    0x00
                }
            }
            FdcPhase::Result => {
                if self.result_index < self.result_buffer.len() {
                    let val = self.result_buffer[self.result_index];
                    self.result_index += 1;
                    if self.result_index >= self.result_buffer.len() {
                        // Fin de la phase de résultat, retour au mode commande
                        self.phase = FdcPhase::Command;
                        self.command_buffer.clear();
                    }
                    val
                } else {
                    self.phase = FdcPhase::Command;
                    self.command_buffer.clear();
                    0x00
                }
            }
            _ => 0x00,
        }
    }

    /// Exécute la commande FDC accumulée
    fn execute_command(&mut self) {
        let cmd = self.command_buffer[0] & 0x1F;
        self.result_buffer.clear();
        self.result_index = 0;

        // Bit 0 (US0) du second octet ("Drive/HD"), présent dans la quasi-totalité
        // des commandes, sélectionne le lecteur ciblé (0 = A, 1 = B). Specify (0x03)
        // et Sense Interrupt Status (0x08) n'ont pas ce champ et ne modifient donc
        // pas le lecteur sélectionné.
        if cmd != 0x03
            && cmd != 0x08
            && let Some(&b1) = self.command_buffer.get(1)
        {
            self.selected_drive = b1 & 0x01;
        }

        match cmd {
            0x03 => {
                // Specify
                // Pas de phase de résultat
                self.phase = FdcPhase::Command;
                self.command_buffer.clear();
            }
            0x04 => {
                // Sense Drive Status
                // Result phase: ST3 (Status Register 3)
                // ST3: Bit 5=DoubleSided, Bit 4=Track0, Bit 2=Ready
                let mut st3 = 0x24; // Ready + DoubleSided
                if !self.selected_drive_available() {
                    st3 = 0x08; // Not Ready : lecteur absent (B désactivé)
                } else if self.drive().current_track == 0 {
                    st3 |= 0x10; // Track 0
                }
                self.result_buffer.push(st3);
                self.phase = FdcPhase::Result;
            }
            0x07 => {
                // Recalibrate
                // Retourne la tête à la piste 0
                if self.selected_drive_available() {
                    self.drive_mut().current_track = 0;
                    self.st0 = 0x20 | self.seek_st0_unit_bits(); // Seek End
                } else {
                    self.st0 = 0x48 | (self.selected_drive & 0x03); // Abnormal termination + Not Ready
                }
                self.seek_interrupt_pending = true;
                self.phase = FdcPhase::Command;
                self.command_buffer.clear();
            }
            0x0F => {
                // Seek
                if self.selected_drive_available() {
                    if self.command_buffer.len() >= 3 {
                        // Le bit 2 (HD) du champ Drive/HD sélectionne la tête, donc
                        // la face : sur une disquette double face, un Seek vers la
                        // face 1 doit déplacer la tête ET changer de face.
                        self.drive_mut().current_side = (self.command_buffer[1] >> 2) & 0x01;
                        let nc = self.command_buffer[2];
                        self.drive_mut().current_track = nc;
                    }
                    self.st0 = 0x20 | self.seek_st0_unit_bits(); // Seek End
                } else {
                    self.st0 = 0x48 | (self.selected_drive & 0x03); // Abnormal termination + Not Ready
                }
                self.seek_interrupt_pending = true;
                self.phase = FdcPhase::Command;
                self.command_buffer.clear();
            }
            0x08 => {
                // Sense Interrupt Status
                // Sur le vrai µPD765A, appeler cette commande sans interruption de
                // seek/recalibrate en attente est une erreur (ST0 = invalid command).
                if self.seek_interrupt_pending {
                    self.result_buffer.push(self.st0);
                    self.result_buffer.push(self.drive().current_track);
                    self.seek_interrupt_pending = false;
                    self.st0 = 0x00;
                } else {
                    self.result_buffer.push(0x80); // Invalid command
                }
                self.phase = FdcPhase::Result;
            }
            0x0A => {
                // Read ID
                if !self.selected_drive_available() {
                    self.result_buffer.push(0x48); // ST0: Abnormal termination + Not Ready
                    self.result_buffer.push(0x01); // ST1: Missing Address Mark
                    self.result_buffer.push(0x00); // ST2
                    self.result_buffer.push(self.drive().current_track);
                    self.result_buffer.push(self.drive().current_side);
                    self.result_buffer.push(0x00);
                    self.result_buffer.push(0x00);
                } else {
                    // Le bit 2 (HD) du champ Drive/HD indique la face demandée
                    // (le bit 0, US0, a été consommé plus haut pour le lecteur).
                    if let Some(&b1) = self.command_buffer.get(1) {
                        self.drive_mut().current_side = (b1 >> 2) & 0x01;
                    }
                    let track = self.drive().current_track;
                    let side = self.drive().current_side;

                    // Quel identifiant se présentera sous la tête, et quand ?
                    // Les secteurs sont répartis sur le tour de piste : on
                    // cherche le prochain à passer et on fait patienter le
                    // contrôleur jusque-là. Renvoyer toujours le premier
                    // secteur, instantanément, bloquait net tout logiciel qui
                    // relève la carte d'une piste en enchaînant les Read ID
                    // (voir doc/discology-copie.md). Modéliser la POSITION
                    // plutôt qu'un simple délai fixe rend le temps de n
                    // commandes successives égal à un tour exactement, quel
                    // que soit le temps de traitement du logiciel entre deux
                    // commandes — c'est précisément ce que mesure le
                    // copieur de Discology pour décider qu'il a fait le tour.
                    let sector_count = {
                        let drv = self.drive();
                        drv.dsk
                            .as_ref()
                            .and_then(|dsk| {
                                dsk.tracks
                                    .iter()
                                    .find(|t| t.number == track && t.side == side)
                            })
                            .map_or(0, |t| t.sectors.len())
                    };

                    let index = if sector_count == 0 {
                        // Piste non formatée : le vrai FDC abandonne après
                        // deux tours sans trouver le moindre identifiant.
                        self.set_busy(2 * REVOLUTION_TICKS);
                        0
                    } else {
                        let pitch = self.sector_pitch_ticks(track, side);
                        let next = self.time / pitch + 1;
                        self.set_busy((next * pitch - self.time) as u32);
                        (next % sector_count as u64) as usize
                    };

                    let found = {
                        let drv = self.drive();
                        drv.dsk.as_ref().and_then(|dsk| {
                            dsk.tracks
                                .iter()
                                .find(|t| t.number == track && t.side == side)
                                .filter(|t| !t.sectors.is_empty())
                                .map(|t| {
                                    let s = &t.sectors[index % t.sectors.len()];
                                    (s.id, s.size)
                                })
                        })
                    };

                    if let Some((sec_id, sec_size)) = found {
                        let n = size_to_n(sec_size);
                        self.result_buffer.push(0x00); // ST0 Success
                        self.result_buffer.push(0x00); // ST1
                        self.result_buffer.push(0x00); // ST2
                        self.result_buffer.push(track); // C
                        self.result_buffer.push(side); // H
                        self.result_buffer.push(sec_id); // R
                        self.result_buffer.push(n); // N
                    } else {
                        // Pas de disque, ou piste inexistante sur l'image chargée
                        self.result_buffer.push(0x48); // ST0: Abnormal termination + Not Ready
                        self.result_buffer.push(0x01); // ST1: Missing Address Mark
                        self.result_buffer.push(0x00); // ST2
                        self.result_buffer.push(track);
                        self.result_buffer.push(side);
                        self.result_buffer.push(0x00);
                        self.result_buffer.push(0x00);
                    }
                }
                self.phase = FdcPhase::Result;
            }
            0x06 => {
                // Read Data : secteurs enregistrés normalement.
                self.read_data_command(false);
            }
            0x0C => {
                // Read Deleted Data : mêmes paramètres que Read Data, mais ne
                // cible que les secteurs enregistrés avec la marque d'adresse
                // "Deleted Data" (voir la doc de `Sector::deleted`). Plusieurs
                // protections CPC (dont Teenage Mutant Hero Turtles) marquent
                // volontairement un ou deux secteurs "deleted" sur une piste
                // donnée pour détecter une copie qui ne préserverait pas
                // cette marque : sans cette commande, le jeu boucle
                // indéfiniment à relire cette piste.
                self.read_data_command(true);
            }
            0x05 => {
                // Write Data
                // NB : contrairement à Read Data, cette implémentation ne gère
                // qu'un seul secteur par commande (cas très largement majoritaire
                // en usage réel — AMSDOS/CP-M écrivent secteur par secteur).
                if self.command_buffer.len() >= 9 {
                    if !self.selected_drive_available() {
                        let track = self.command_buffer[2];
                        let side = self.command_buffer[3] & 0x01;
                        let n = self.command_buffer[5];
                        self.result_buffer.push(0x48); // ST0: Abnormal termination + Not Ready
                        self.result_buffer.push(0x01); // ST1
                        self.result_buffer.push(0x00); // ST2
                        self.result_buffer.push(track);
                        self.result_buffer.push(side);
                        self.result_buffer.push(self.command_buffer[4]);
                        self.result_buffer.push(n);
                        self.phase = FdcPhase::Result;
                    } else {
                        self.drive_mut().current_track = self.command_buffer[2];
                        self.drive_mut().current_side = self.command_buffer[3] & 0x01;
                        self.drive_mut().current_sector = self.command_buffer[4];

                        self.execution_buffer.clear();
                        self.execution_index = 0;
                        self.phase = FdcPhase::ExecutionWrite;
                    }
                } else {
                    self.phase = FdcPhase::Command;
                    self.command_buffer.clear();
                }
            }
            0x0D => {
                // Format Track
                // Command format: [Cmd, HD/US, N, SC, GPL3, D]
                // Les descripteurs (C,H,R,N) de chaque secteur à créer arrivent
                // ensuite en phase d'exécution (SC groupes de 4 octets).
                if self.command_buffer.len() >= 6 {
                    if !self.selected_drive_available() {
                        self.result_buffer.push(0x48); // ST0: Abnormal termination + Not Ready
                        self.phase = FdcPhase::Result;
                    } else {
                        // Bit 2 (HD) du champ Drive/HD indique la face à formater
                        // (le bit 0, US0, a été consommé plus haut pour le lecteur).
                        self.drive_mut().current_side = (self.command_buffer[1] >> 2) & 0x01;
                        self.format_n = self.command_buffer[2];
                        self.format_sc = self.command_buffer[3];
                        self.format_fill = self.command_buffer[5];
                        self.execution_buffer.clear();
                        self.execution_index = 0;
                        self.formatting = true;
                        self.phase = FdcPhase::ExecutionWrite;
                    }
                } else {
                    self.phase = FdcPhase::Command;
                    self.command_buffer.clear();
                }
            }
            _ => {
                // Commande non supportée, phase de résultat par défaut (ST0 = invalid command)
                self.result_buffer.push(0x80); // ST0
                self.phase = FdcPhase::Result;
            }
        }
    }

    /// Commandes Read Data (0x06) et Read Deleted Data (0x0C) : mêmes
    /// paramètres et même transfert multi-secteurs, seule la marque
    /// d'adresse recherchée (`Sector::deleted`) diffère.
    ///
    /// Command format: [Cmd, Drive/HD, C, H, R, N, EOT, GPL, DTL]
    fn read_data_command(&mut self, want_deleted: bool) {
        if self.command_buffer.len() < 9 {
            self.phase = FdcPhase::Command;
            self.command_buffer.clear();
            return;
        }

        let track = self.command_buffer[2];
        let side = self.command_buffer[3] & 0x01;
        let start_sector = self.command_buffer[4];
        let n = self.command_buffer[5];
        let eot = self.command_buffer[6];

        if !self.selected_drive_available() {
            self.result_buffer.push(0x48); // ST0: Abnormal termination + Not Ready
            self.result_buffer.push(0x01); // ST1
            self.result_buffer.push(0x00); // ST2
            self.result_buffer.push(track);
            self.result_buffer.push(side);
            self.result_buffer.push(start_sector);
            self.result_buffer.push(n);
            self.phase = FdcPhase::Result;
            return;
        }

        self.drive_mut().current_track = track;
        self.drive_mut().current_side = side;
        self.drive_mut().current_sector = start_sector;

        // Transfert de tous les secteurs consécutifs entre R et EOT (inclus)
        // présents sur la piste ET portant la marque recherchée, comme le
        // ferait le vrai FDC pour une commande couvrant plusieurs secteurs.
        let (found_any, combined, last_id) = {
            let drv = self.drive();
            let mut combined = Vec::new();
            let mut last_id = start_sector;
            let mut found_any = false;

            if let Some(ref dsk) = drv.dsk
                && let Some(t) = dsk
                    .tracks
                    .iter()
                    .find(|t| t.number == track && t.side == side)
            {
                let mut matched: Vec<&Sector> = t
                    .sectors
                    .iter()
                    .filter(|s| s.id >= start_sector && s.id <= eot && s.deleted == want_deleted)
                    .collect();
                matched.sort_by_key(|s| s.id);
                for s in matched {
                    combined.extend_from_slice(&s.data);
                    last_id = s.id;
                    found_any = true;
                }
            }
            (found_any, combined, last_id)
        };

        if found_any {
            self.execution_buffer = combined;
            self.execution_index = 0;
            self.phase = FdcPhase::ExecutionRead;

            self.result_buffer.push(0x00); // ST0
            self.result_buffer.push(0x00); // ST1
            self.result_buffer.push(0x00); // ST2
            self.result_buffer.push(track);
            self.result_buffer.push(side);
            self.result_buffer.push(last_id.wrapping_add(1)); // Secteur suivant
            self.result_buffer.push(n); // N tel que demandé par la commande
        } else {
            // Secteur non trouvé (No Data error)
            self.result_buffer.push(0x40); // ST0: Abnormal termination
            self.result_buffer.push(0x04); // ST1: No Data
            self.result_buffer.push(0x00); // ST2
            self.result_buffer.push(track);
            self.result_buffer.push(side);
            self.result_buffer.push(start_sector);
            self.result_buffer.push(n);
            self.phase = FdcPhase::Result;
        }
    }

    /// Réécrit le fichier .dsk du lecteur sélectionné depuis l'image en
    /// mémoire, pour que les écritures faites par le logiciel émulé
    /// (SAVE BASIC, formatage...) survivent à un power cycle ou à une
    /// fermeture de l'émulateur — jusqu'ici, seule l'image en RAM changeait,
    /// jamais le fichier réellement sur disque. "None" signifie qu'aucun
    /// fichier ne soutient l'image (ne devrait pas arriver ici : Format
    /// Track sur un lecteur sans disquette insérée), et n'est donc pas
    /// persisté.
    fn persist_drive_dsk(&self) {
        let drv = self.drive();
        if drv.current_filename == "None" {
            return;
        }
        if let Some(ref dsk) = drv.dsk
            && let Err(e) = Self::write_dsk_file(dsk, &drv.current_filename)
        {
            println!(
                "Erreur d'ecriture de '{}' : {e}",
                drv.current_filename
            );
        }
    }

    /// Clôture de l'écriture d'un secteur
    fn finish_write_command(&mut self) {
        let track = self.drive().current_track;
        let side = self.drive().current_side;
        let sector_id = self.drive().current_sector;
        let data = self.execution_buffer.clone();
        let n = *self.command_buffer.get(5).unwrap_or(&2);

        // Mise à jour de l'image disquette du lecteur ciblé, en mémoire
        let mut updated = false;
        {
            let drv = self.drive_mut();
            if let Some(ref mut dsk) = drv.dsk {
                for t in &mut dsk.tracks {
                    if t.number == track && t.side == side {
                        for s in &mut t.sectors {
                            if s.id == sector_id {
                                s.size = data.len();
                                s.data = data.clone();
                                updated = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if updated {
            self.persist_drive_dsk();
        }

        self.result_buffer.clear();
        self.result_index = 0;

        if updated {
            self.result_buffer.push(0x00); // ST0 Success
            self.result_buffer.push(0x00); // ST1
            self.result_buffer.push(0x00); // ST2
            self.result_buffer.push(track);
            self.result_buffer.push(side);
            self.result_buffer.push(sector_id.wrapping_add(1)); // Prochain secteur
            self.result_buffer.push(n);
        } else {
            // Erreur d'écriture (secteur inexistant sur l'image : on ne crée pas de
            // nouveau secteur via Write Data, seul Format Track le fait, comme sur
            // le vrai matériel)
            self.result_buffer.push(0x40); // Abnormal
            self.result_buffer.push(0x04); // No Data
            self.result_buffer.push(0x00);
            self.result_buffer.push(track);
            self.result_buffer.push(side);
            self.result_buffer.push(sector_id);
            self.result_buffer.push(n);
        }
        self.phase = FdcPhase::Result;
    }

    /// Clôture de la commande Format Track : construit les secteurs de la piste à
    /// partir des descripteurs (C,H,R,N) reçus en phase d'exécution, remplis avec
    /// l'octet de bourrage demandé, sur le lecteur actuellement sélectionné.
    fn finish_format_command(&mut self) {
        let track_num = self.drive().current_track;
        let side_num = self.drive().current_side;
        let n = self.format_n.min(6);
        let sector_size = 128usize << n;
        let fill = self.format_fill;
        let exec_buffer = self.execution_buffer.clone();

        let mut sectors = Vec::new();
        for chunk in exec_buffer.chunks(4) {
            if chunk.len() < 4 {
                break;
            }
            let r = chunk[2]; // identifiant de secteur (R) du descripteur
            sectors.push(Sector {
                id: r,
                size: sector_size,
                data: vec![fill; sector_size],
                deleted: false,
            });
        }

        {
            let drv = self.drive_mut();
            let dsk = drv
                .dsk
                .get_or_insert_with(|| DskImage { tracks: Vec::new() });

            if let Some(t) = dsk
                .tracks
                .iter_mut()
                .find(|t| t.number == track_num && t.side == side_num)
            {
                t.sectors = sectors;
                t.sector_size = n;
            } else {
                dsk.tracks.push(Track {
                    number: track_num,
                    side: side_num,
                    sector_size: n,
                    sectors,
                });
            }
            drv.disk_loaded = true;
        }
        self.persist_drive_dsk();

        self.result_buffer.clear();
        self.result_index = 0;
        self.result_buffer.push(0x00); // ST0
        self.result_buffer.push(0x00); // ST1
        self.result_buffer.push(0x00); // ST2
        self.result_buffer.push(track_num);
        self.result_buffer.push(side_num);
        self.result_buffer.push(0x00); // R (non significatif après un formatage)
        self.result_buffer.push(n);
        self.phase = FdcPhase::Result;
        self.formatting = false;
    }
}

impl Default for Fdc {
    fn default() -> Self {
        Self::new()
    }
}

/// Convertit une taille de secteur en octets vers le code N (128 << N) attendu
/// par le protocole FDC. Retombe sur N=2 (512 octets, le standard AMSDOS) si la
/// taille ne correspond à aucune puissance de deux valide.
fn size_to_n(size: usize) -> u8 {
    let mut n = 0u8;
    let mut s = 128usize;
    while s < size && n < 6 {
        s <<= 1;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fdc_with_track(sectors: Vec<Sector>) -> Fdc {
        let mut fdc = Fdc::new();
        fdc.drive_a.disk_loaded = true;
        fdc.drive_a.dsk = Some(DskImage {
            tracks: vec![Track {
                number: 1,
                side: 0,
                sector_size: 2,
                sectors,
            }],
        });
        fdc
    }

    fn send_command(fdc: &mut Fdc, bytes: &[u8]) {
        for &b in bytes {
            fdc.write_data(b);
        }
    }

    /// Certaines protections CPC (dont Teenage Mutant Hero Turtles) marquent
    /// volontairement un secteur avec la marque d'adresse "Deleted Data" pour
    /// détecter une copie qui ne la préserverait pas : sans Read Deleted Data
    /// (0x0C), le jeu boucle indéfiniment à relire la piste. Read Data
    /// (0x06) ne doit pas voir ce secteur.
    #[test]
    fn read_deleted_data_finds_a_sector_read_data_cannot_see() {
        let mut fdc = fdc_with_track(vec![Sector {
            id: 0x88,
            size: 512,
            data: vec![0x42; 512],
            deleted: true,
        }]);

        // Read Data (0x06) : Cmd, Drive/HD, C, H, R, N, EOT, GPL, DTL
        send_command(
            &mut fdc,
            &[0x06, 0x00, 0x01, 0x00, 0x88, 0x02, 0x88, 0x2A, 0xFF],
        );
        assert_eq!(fdc.phase, FdcPhase::Result);
        assert_eq!(
            fdc.result_buffer[0] & 0x40,
            0x40,
            "ST0 doit signaler une terminaison anormale"
        );
        assert_eq!(
            fdc.result_buffer[1] & 0x04,
            0x04,
            "ST1 doit signaler No Data"
        );

        // Read Deleted Data (0x0C) : mêmes paramètres, doit réussir.
        let mut fdc = fdc_with_track(vec![Sector {
            id: 0x88,
            size: 512,
            data: vec![0x42; 512],
            deleted: true,
        }]);
        send_command(
            &mut fdc,
            &[0x0C, 0x00, 0x01, 0x00, 0x88, 0x02, 0x88, 0x2A, 0xFF],
        );
        assert_eq!(fdc.phase, FdcPhase::ExecutionRead);
        assert_eq!(fdc.result_buffer[0], 0x00, "ST0 doit signaler un succes");
        assert_eq!(fdc.execution_buffer, vec![0x42; 512]);
    }

    /// Symétrique du test précédent : un secteur enregistré normalement
    /// reste invisible à Read Deleted Data, comme sur le vrai µPD765A.
    #[test]
    fn read_deleted_data_does_not_see_a_normal_sector() {
        let mut fdc = fdc_with_track(vec![Sector {
            id: 0x41,
            size: 512,
            data: vec![0x99; 512],
            deleted: false,
        }]);
        send_command(
            &mut fdc,
            &[0x0C, 0x00, 0x01, 0x00, 0x41, 0x02, 0x41, 0x2A, 0xFF],
        );
        assert_eq!(fdc.phase, FdcPhase::Result);
        assert_eq!(
            fdc.result_buffer[1] & 0x04,
            0x04,
            "ST1 doit signaler No Data"
        );
    }

    /// La disquette tourne : deux Read ID consécutifs tombent sur des
    /// secteurs différents, dans l'ordre physique de la piste, et on
    /// retombe sur le premier après un tour complet. Renvoyer toujours le
    /// premier secteur (ce que faisait cette émulation) fige net tout
    /// logiciel qui relève la carte d'une piste, à commencer par le
    /// copieur de Discology (voir doc/discology-copie.md).
    #[test]
    fn successive_read_ids_walk_the_whole_track() {
        let mut fdc = fdc_with_track(
            [0xC1u8, 0xC6, 0xC2]
                .iter()
                .map(|&id| Sector {
                    id,
                    size: 512,
                    data: vec![0; 512],
                    deleted: false,
                })
                .collect(),
        );
        fdc.drive_a.current_track = 1;

        let mut lus = Vec::new();
        for _ in 0..6 {
            send_command(&mut fdc, &[0x0A, 0x00]);
            assert_eq!(fdc.phase, FdcPhase::Result);
            lus.push(fdc.result_buffer[5]);
            // La tête doit atteindre l'identifiant suivant avant que le
            // contrôleur ne réponde.
            assert!(fdc.busy_ticks > 0, "Read ID doit demander du temps");
            let attente = fdc.busy_ticks;
            fdc.tick(attente);
            // Consomme la phase de résultat.
            for _ in 0..7 {
                fdc.read_data();
            }
        }

        assert_eq!(
            lus,
            vec![0xC6, 0xC2, 0xC1, 0xC6, 0xC2, 0xC1],
            "les identifiants doivent défiler dans l'ordre de la piste, en boucle"
        );
    }

    /// Tant que le secteur voulu n'est pas passé sous la tête, le MSR
    /// annonce "occupé" : c'est ce temps que mesurent les copieurs pour
    /// déduire la géométrie d'une piste.
    #[test]
    fn the_status_register_says_busy_until_the_sector_comes_round() {
        let mut fdc = fdc_with_track(vec![Sector {
            id: 0xC1,
            size: 512,
            data: vec![0; 512],
            deleted: false,
        }]);
        fdc.drive_a.current_track = 1;

        send_command(&mut fdc, &[0x0A, 0x00]);
        assert_eq!(fdc.read_msr() & 0x80, 0x00, "RQM doit rester à 0");
        assert_eq!(fdc.read_msr() & 0x10, 0x10, "le FDC doit se dire occupé");

        let attente = fdc.busy_ticks;
        fdc.tick(attente - 1);
        assert_eq!(fdc.read_msr() & 0x80, 0x00);
        fdc.tick(1);
        assert_eq!(
            fdc.read_msr() & 0xC0,
            0xC0,
            "le résultat doit être disponible une fois le secteur arrivé"
        );
    }

    /// Un tour de piste dure un tour, pas plus : la somme des attentes de
    /// tous les secteurs d'une piste ne doit pas dépasser une révolution,
    /// sinon les logiciels qui chronomètrent un tour concluent que la piste
    /// contient moins de secteurs qu'en réalité.
    #[test]
    fn a_full_turn_of_read_ids_fits_in_one_revolution() {
        let mut fdc = fdc_with_track(
            (0..9)
                .map(|i| Sector {
                    id: 0xC1 + i,
                    size: 512,
                    data: vec![0; 512],
                    deleted: false,
                })
                .collect(),
        );
        fdc.drive_a.current_track = 1;

        let mut total = 0u64;
        for _ in 0..9 {
            send_command(&mut fdc, &[0x0A, 0x00]);
            let attente = fdc.busy_ticks;
            total += attente as u64;
            fdc.tick(attente);
            for _ in 0..7 {
                fdc.read_data();
            }
        }

        assert!(
            total <= REVOLUTION_TICKS as u64,
            "un tour de 9 secteurs prend {total} cycles, plus qu'une révolution"
        );
        assert!(
            total * 10 > REVOLUTION_TICKS as u64 * 7,
            "un tour de 9 secteurs ne prend que {total} cycles : bien trop rapide"
        );
    }

    /// Une disquette vierge écrite sur disque puis relue doit retrouver
    /// exactement le même nombre de pistes/secteurs et le même octet de
    /// bourrage : c'est ce que la commande console `blank` insère ensuite
    /// dans un lecteur, elle doit donc être un .dsk valide et cohérent.
    #[test]
    fn a_blank_disk_survives_a_round_trip_through_a_dsk_file() {
        let dsk = Fdc::blank_dsk_image();
        let path = std::env::temp_dir().join("amstrad_cpc_test_blank.dsk");
        let path = path.to_str().unwrap();

        Fdc::write_dsk_file(&dsk, path).expect("ecriture du .dsk vierge");
        let raw = std::fs::read(path).expect("lecture du .dsk vierge");
        std::fs::remove_file(path).ok();

        let reloaded = DskImage::parse(&raw).expect(".dsk vierge non reconnu");
        assert_eq!(reloaded.tracks.len(), dsk.tracks.len());
        for track in &reloaded.tracks {
            assert_eq!(track.sectors.len(), 9, "9 secteurs par piste attendus");
            for sec in &track.sectors {
                assert_eq!(sec.size, 512);
                assert!(
                    sec.data.iter().all(|&b| b == 0xE5),
                    "secteur vierge attendu (bourrage 0xE5)"
                );
            }
        }
    }

    /// Une écriture de secteur (SAVE BASIC, par exemple) doit être reflétée
    /// dans le fichier .dsk sur disque, pas seulement dans l'image en
    /// mémoire — sans quoi elle ne survit pas à un power cycle ou à la
    /// fermeture de l'émulateur, qui rechargent le fichier depuis le disque.
    #[test]
    fn writing_a_sector_persists_to_the_dsk_file_on_disk() {
        let dsk = Fdc::blank_dsk_image();
        let path = std::env::temp_dir().join("amstrad_cpc_test_write_persists.dsk");
        let path = path.to_str().unwrap();
        std::fs::remove_file(path).ok();
        Fdc::write_dsk_file(&dsk, path).expect("ecriture du .dsk vierge");

        let mut fdc = Fdc::new();
        fdc.load_disk(path).expect("chargement du .dsk vierge");

        // Write Data : Cmd, Drive/HD, C, H, R, N, EOT, GPL, DTL, puis les
        // 512 octets du secteur (piste 0, secteur 0xC1, comme le formatage
        // vierge standard).
        send_command(
            &mut fdc,
            &[0x05, 0x00, 0x00, 0x00, 0xC1, 0x02, 0xC1, 0x2A, 0xFF],
        );
        for _ in 0..512 {
            fdc.write_data(0xAA);
        }
        assert_eq!(
            fdc.phase,
            FdcPhase::Result,
            "l'ecriture du secteur doit s'etre terminee"
        );

        let raw = std::fs::read(path).expect("relecture du .dsk depuis le disque");
        std::fs::remove_file(path).ok();
        let reread = DskImage::parse(&raw).expect(".dsk relu invalide");
        let sector = reread
            .tracks
            .iter()
            .find(|t| t.number == 0 && t.side == 0)
            .and_then(|t| t.sectors.iter().find(|s| s.id == 0xC1))
            .expect("secteur 0xC1 absent du fichier relu");

        assert!(
            sector.data.iter().all(|&b| b == 0xAA),
            "le fichier sur disque doit refleter l'ecriture, pas rester au bourrage d'origine"
        );
    }
}
