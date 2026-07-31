use std::fs::File;
use std::io::Read;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FdcPhase {
    Command,
    ExecutionRead,
    ExecutionWrite,
    Result,
}

pub struct Sector {
    pub id: u8,
    pub size: usize,
    pub data: Vec<u8>,
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
            return Err("DSK size is too short".to_string());
        }

        let signature = std::str::from_utf8(&data[0..8]).unwrap_or("");
        if signature.starts_with("MV - CPC") {
            // Standard DSK
            let num_tracks = data[0x30];
            let num_sides = data[0x31];
            let track_size = u16::from_le_bytes([data[0x32], data[0x33]]) as usize;

            let mut tracks = Vec::new();
            let mut offset = 0x100;

            for _t in 0..num_tracks {
                for _s in 0..num_sides {
                    if offset + 0x100 > data.len() {
                        break;
                    }
                    let track_header = &data[offset..offset + 0x100];
                    let track_num = track_header[0x10];
                    let side_num = track_header[0x11];
                    let sec_size_code = track_header[0x14];
                    let num_sectors = track_header[0x15];

                    let mut sectors = Vec::new();
                    let mut sector_data_offset = offset + 0x100;

                    for sec_idx in 0..num_sectors {
                        let info_offset = 0x18 + (sec_idx as usize * 8);
                        let sec_id = track_header[info_offset + 2];
                        let sec_size_code_inf = track_header[info_offset + 3];
                        let sec_size = 128 << sec_size_code_inf;

                        if sector_data_offset + sec_size > data.len() {
                            break;
                        }
                        let sec_data =
                            data[sector_data_offset..sector_data_offset + sec_size].to_vec();

                        sectors.push(Sector {
                            id: sec_id,
                            size: sec_size,
                            data: sec_data,
                        });
                        sector_data_offset += sec_size;
                    }

                    tracks.push(Track {
                        number: track_num,
                        side: side_num,
                        sector_size: sec_size_code,
                        sectors,
                    });

                    offset += track_size;
                }
            }
            Ok(DskImage { tracks })
        } else if signature.starts_with("EXTENDED") {
            // Extended DSK
            let num_tracks = data[0x30];
            let num_sides = data[0x31];

            let mut tracks = Vec::new();
            let mut offset = 0x100;

            for t in 0..num_tracks {
                for s in 0..num_sides {
                    let track_size_code =
                        data[0x34 + (t as usize * num_sides as usize) + s as usize];
                    let track_size = (track_size_code as usize) * 256;
                    if track_size == 0 {
                        continue;
                    }

                    if offset + 0x100 > data.len() {
                        break;
                    }
                    let track_header = &data[offset..offset + 0x100];
                    let track_num = track_header[0x10];
                    let side_num = track_header[0x11];
                    let sec_size_code = track_header[0x14];
                    let num_sectors = track_header[0x15];

                    let mut sectors = Vec::new();
                    let mut sector_data_offset = offset + 0x100;

                    for sec_idx in 0..num_sectors {
                        let info_offset = 0x18 + (sec_idx as usize * 8);
                        let sec_id = track_header[info_offset + 2];
                        let sec_size_code_inf = track_header[info_offset + 3];
                        let sec_size = 128 << sec_size_code_inf;

                        let actual_size = u16::from_le_bytes([
                            track_header[info_offset + 6],
                            track_header[info_offset + 7],
                        ]) as usize;
                        let size_to_read = if actual_size > 0 {
                            actual_size
                        } else {
                            sec_size
                        };

                        if sector_data_offset + size_to_read > data.len() {
                            break;
                        }
                        let sec_data =
                            data[sector_data_offset..sector_data_offset + size_to_read].to_vec();

                        sectors.push(Sector {
                            id: sec_id,
                            size: size_to_read,
                            data: sec_data,
                        });
                        sector_data_offset += size_to_read;
                    }

                    tracks.push(Track {
                        number: track_num,
                        side: side_num,
                        sector_size: sec_size_code,
                        sectors,
                    });

                    offset += track_size;
                }
            }
            Ok(DskImage { tracks })
        } else {
            Err("Unsupported DSK format".to_string())
        }
    }
}

pub struct Fdc {
    pub phase: FdcPhase,
    pub command_buffer: Vec<u8>,
    pub command_len: usize,
    pub result_buffer: Vec<u8>,
    pub result_index: usize,

    // État logique
    pub current_track: u8,
    pub current_sector: u8,
    pub current_side: u8,
    pub motor_on: bool,
    pub disk_loaded: bool,
    pub current_filename: String,

    // Données de disquette
    pub dsk: Option<DskImage>,

    // Execution phase (transfert de données secteur)
    pub execution_buffer: Vec<u8>,
    pub execution_index: usize,

    // Status registers (renvoyés dans les phases de résultat)
    pub st0: u8,
}

impl Fdc {
    pub fn new() -> Self {
        Self {
            phase: FdcPhase::Command,
            command_buffer: Vec::new(),
            command_len: 0,
            result_buffer: Vec::new(),
            result_index: 0,
            current_track: 0,
            current_sector: 0xC1, // Premier secteur standard d'une disquette CPC (système AMSDOS)
            current_side: 0,
            motor_on: false,
            disk_loaded: false,
            current_filename: "None".to_string(),
            dsk: None,
            execution_buffer: Vec::new(),
            execution_index: 0,
            st0: 0,
        }
    }

    /// Charge un fichier disquette .dsk
    pub fn load_disk(&mut self, filename: &str) -> Result<(), String> {
        let mut f = File::open(filename).map_err(|e| e.to_string())?;
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        let dsk = DskImage::parse(&buffer)?;
        self.dsk = Some(dsk);
        self.disk_loaded = true;
        self.current_filename = filename.to_string();
        self.current_track = 0;
        self.current_sector = 0xC1;

        println!("Floppy DSK Loaded: {}", filename);
        Ok(())
    }

    /// Éjecte la disquette
    pub fn eject_disk(&mut self) {
        self.dsk = None;
        self.disk_loaded = false;
        self.current_filename = "None".to_string();
        println!("Floppy DSK Ejected");
    }

    /// Initialise le FDC avec les valeurs par défaut du CPC
    pub fn init_defaults(&mut self) {
        self.current_track = 0;
        self.current_sector = 0xC1;
        self.current_side = 0;
        self.motor_on = false;
        self.disk_loaded = false;
        self.current_filename = "None".to_string();
        self.dsk = None;
        self.execution_buffer.clear();
        self.execution_index = 0;
        self.st0 = 0;
    }

    /// Lecture du registre de statut (MSR) sur le port &FB7E
    pub fn read_msr(&self) -> u8 {
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

        // Bit 0: Drive 0 seek active (si la disquette est chargée)
        if self.disk_loaded {
            msr |= 0x01;
        }

        msr
    }

    /// Écriture d'un octet de données (port &FB7F)
    pub fn write_data(&mut self, val: u8) {
        if self.phase != FdcPhase::Command {
            if self.phase == FdcPhase::ExecutionWrite {
                // Phase d'écriture de données secteur
                self.execution_buffer.push(val);
                self.execution_index += 1;
                // Si on a écrit tout le secteur, on termine la commande
                if self.execution_index >= 512 {
                    self.finish_write_command();
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
                0x05 => 9, // Write Data
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
                if self.current_track == 0 {
                    st3 |= 0x10; // Track 0
                }
                self.result_buffer.push(st3);
                self.phase = FdcPhase::Result;
            }
            0x07 => {
                // Recalibrate
                // Retourne la tête à la piste 0
                self.current_track = 0;
                self.st0 = 0x20; // Seek End
                self.phase = FdcPhase::Command;
                self.command_buffer.clear();
            }
            0x0F => {
                // Seek
                if self.command_buffer.len() >= 3 {
                    self.current_track = self.command_buffer[2];
                }
                self.st0 = 0x20; // Seek End
                self.phase = FdcPhase::Command;
                self.command_buffer.clear();
            }
            0x08 => {
                // Sense Interrupt Status
                // Result: ST0, PCN (Present Cylinder Number)
                self.result_buffer.push(self.st0);
                self.result_buffer.push(self.current_track);
                self.phase = FdcPhase::Result;
                // Reset st0
                self.st0 = 0x00;
            }
            0x0A => {
                // Read ID
                // Result: ST0, ST1, ST2, C, H, R, N
                self.result_buffer.push(0x00); // ST0 Success
                self.result_buffer.push(0x00); // ST1
                self.result_buffer.push(0x00); // ST2
                self.result_buffer.push(self.current_track); // C
                self.result_buffer.push(self.current_side); // H
                self.result_buffer.push(self.current_sector); // R
                self.result_buffer.push(2); // N (512 bytes)
                self.phase = FdcPhase::Result;
            }
            0x06 => {
                // Read Data
                // Command format: [Cmd, Drive, C, H, R, N, EOT, GPL, DTL]
                if self.command_buffer.len() >= 6 {
                    let track = self.command_buffer[2];
                    let side = self.command_buffer[3] & 0x01;
                    let sector_id = self.command_buffer[4];

                    self.current_track = track;
                    self.current_side = side;
                    self.current_sector = sector_id;

                    // Recherche du secteur dans l'image disquette
                    let mut found_data = None;
                    if let Some(ref dsk) = self.dsk {
                        for t in &dsk.tracks {
                            if t.number == track && t.side == side {
                                for s in &t.sectors {
                                    if s.id == sector_id {
                                        found_data = Some(s.data.clone());
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(data) = found_data {
                        self.execution_buffer = data;
                        self.execution_index = 0;
                        self.phase = FdcPhase::ExecutionRead;

                        // Préparer la phase de résultat pour la fin de la lecture
                        self.result_buffer.push(0x00); // ST0
                        self.result_buffer.push(0x00); // ST1
                        self.result_buffer.push(0x00); // ST2
                        self.result_buffer.push(track);
                        self.result_buffer.push(side);
                        self.result_buffer.push(sector_id + 1); // Secteur suivant
                        self.result_buffer.push(2); // Taille
                    } else {
                        // Secteur non trouvé (No Data error)
                        self.result_buffer.push(0x40); // ST0: Abormal termination
                        self.result_buffer.push(0x04); // ST1: No Data
                        self.result_buffer.push(0x00); // ST2
                        self.result_buffer.push(track);
                        self.result_buffer.push(side);
                        self.result_buffer.push(sector_id);
                        self.result_buffer.push(2);
                        self.phase = FdcPhase::Result;
                    }
                } else {
                    self.phase = FdcPhase::Command;
                    self.command_buffer.clear();
                }
            }
            0x05 => {
                // Write Data
                if self.command_buffer.len() >= 6 {
                    self.current_track = self.command_buffer[2];
                    self.current_side = self.command_buffer[3] & 0x01;
                    self.current_sector = self.command_buffer[4];

                    self.execution_buffer.clear();
                    self.execution_index = 0;
                    self.phase = FdcPhase::ExecutionWrite;
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

    /// Clôture de l'écriture d'un secteur
    fn finish_write_command(&mut self) {
        let track = self.current_track;
        let side = self.current_side;
        let sector_id = self.current_sector;

        // Mise à jour de l'image disquette en mémoire
        let mut updated = false;
        if let Some(ref mut dsk) = self.dsk {
            for t in &mut dsk.tracks {
                if t.number == track && t.side == side {
                    for s in &mut t.sectors {
                        if s.id == sector_id {
                            s.data = self.execution_buffer.clone();
                            updated = true;
                            break;
                        }
                    }
                }
            }
        }

        self.result_buffer.clear();
        self.result_index = 0;

        if updated {
            self.result_buffer.push(0x00); // ST0 Success
            self.result_buffer.push(0x00); // ST1
            self.result_buffer.push(0x00); // ST2
            self.result_buffer.push(track);
            self.result_buffer.push(side);
            self.result_buffer.push(sector_id + 1); // Prochain secteur
            self.result_buffer.push(2); // Taille 512
        } else {
            // Erreur d'écriture
            self.result_buffer.push(0x40); // Abnormal
            self.result_buffer.push(0x04); // No Data
            self.result_buffer.push(0x00);
            self.result_buffer.push(track);
            self.result_buffer.push(side);
            self.result_buffer.push(sector_id);
            self.result_buffer.push(2);
        }
        self.phase = FdcPhase::Result;
    }
}
