/// Gestion de la mémoire de l'Amstrad CPC 6128.
///
/// L'Amstrad CPC 6128 possède :
/// - 128 Ko de RAM, répartis en 8 banques de 16 Ko (Banques 0 à 7).
///   - Banques 0-3 : Les 64 Ko de base.
///   - Banques 4-7 : Les 64 Ko d'extension.
/// - Une ROM basse (OS ou Diagnostic Lower) de 16 Ko.
/// - Plusieurs ROMs hautes de 16 Ko (BASIC, AMSDOS, Diagnostic Upper).
pub struct Memory {
    pub ram: Box<[u8; 128 * 1024]>,            // 128 Ko de RAM
    pub rom_low: Box<[u8; 16 * 1024]>,         // ROM basse de 16 Ko
    pub rom_high: Box<[[u8; 16 * 1024]; 256]>, // 256 ROMs hautes de 16 Ko

    pub rom_low_enabled: bool, // ROM basse activée en lecture ($0000-$3FFF)
    pub rom_high_enabled: bool, // ROM haute activée en lecture ($C000-$FFFF)
    pub selected_high_rom: u8, // Index de la ROM haute actuellement sélectionnée

    pub ram_config: u8, // Configuration actuelle du banking RAM (0 à 7, écrit via le Gate Array)
}

impl Memory {
    /// Crée une nouvelle mémoire propre de 128 Ko.
    pub fn new() -> Self {
        Self {
            ram: Box::new([0; 128 * 1024]),
            rom_low: Box::new([0; 16 * 1024]),
            rom_high: Box::new([[0; 16 * 1024]; 256]),
            rom_low_enabled: true,
            rom_high_enabled: true,
            selected_high_rom: 0,
            ram_config: 0, // Configuration standard (0xC0) correspondant au mode de base
        }
    }

    /// Charge la ROM basse (16 Ko).
    pub fn load_low_rom(&mut self, data: &[u8]) {
        let size = data.len().min(self.rom_low.len());
        self.rom_low[..size].copy_from_slice(&data[..size]);
    }

    /// Charge une ROM haute (16 Ko).
    pub fn load_high_rom(&mut self, index: u8, data: &[u8]) {
        let size = data.len().min(self.rom_high[index as usize].len());
        self.rom_high[index as usize][..size].copy_from_slice(&data[..size]);
    }

    /// Sélectionne la ROM haute active (via écriture I/O à $DF00).
    pub fn select_high_rom(&mut self, index: u8) {
        self.selected_high_rom = index;
    }

    /// Retourne l'adresse physique dans les 128 Ko de RAM pour une adresse CPU donnée (0x0000 - 0xFFFF).
    /// Dépend de la configuration de banking RAM active (ram_config).
    pub fn get_ram_physical_address(&self, address: u16) -> usize {
        let page = address / 0x4000; // Bloc CPU de 0 à 3
        let offset = (address % 0x4000) as usize;

        // Table de mapping pour les 8 configurations RAM de base du CPC 6128 :
        // Chaque configuration associe à chacun des 4 blocs du CPU (de 0 à 3) une banque de RAM de 0 à 7.
        let bank = match self.ram_config {
            0 => [0, 1, 2, 3][page as usize], // Config 0 (0xC0) : Standard
            1 => [0, 1, 2, 7][page as usize], // Config 1 (0xC1) : VRAM alternative ou extension
            2 => [0, 1, 2, 4][page as usize], // Config 2 (0xC2) : Utile pour le multitâche / copie
            3 => [0, 1, 2, 5][page as usize], // Config 3 (0xC3)
            4 => [0, 1, 2, 6][page as usize], // Config 4 (0xC4)
            5 => [0, 1, 2, 7][page as usize], // Config 5 (0xC5)
            6 => [0, 1, 2, 4][page as usize], // Config 6 (0xC6)
            7 => [0, 1, 2, 5][page as usize], // Config 7 (0xC7)
            _ => unreachable!(),
        };

        (bank * 16384) + offset
    }

    /// Lecture d'un octet en fonction du banking actif (RAM + ROM).
    pub fn read_byte(&self, address: u16) -> u8 {
        if address < 0x4000 {
            // Zone Basse ($0000 - $3FFF)
            if self.rom_low_enabled {
                self.rom_low[address as usize]
            } else {
                let physical_addr = self.get_ram_physical_address(address);
                self.ram[physical_addr]
            }
        } else if address >= 0xC000 {
            // Zone Haute ($C000 - $FFFF)
            if self.rom_high_enabled {
                self.rom_high[self.selected_high_rom as usize][(address - 0xC000) as usize]
            } else {
                let physical_addr = self.get_ram_physical_address(address);
                self.ram[physical_addr]
            }
        } else {
            // Zone Centrale ($4000 - $BFFF) est toujours de la RAM mappée
            let physical_addr = self.get_ram_physical_address(address);
            self.ram[physical_addr]
        }
    }

    /// Écriture d'un octet. L'écriture se fait TOUJOURS dans la RAM.
    pub fn write_byte(&mut self, address: u16, value: u8) {
        let physical_addr = self.get_ram_physical_address(address);
        self.ram[physical_addr] = value;
    }
}
