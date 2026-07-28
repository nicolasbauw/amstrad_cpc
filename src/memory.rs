/// Gestion de la mémoire de l'Amstrad CPC 6128.
pub struct Memory {
    pub ram: Box<[u8]>,
    pub rom_low: Box<[u8]>,
    pub rom_high: Box<[u8]>, // Stocké à plat pour plus de simplicité (256 * 16 * 1024 = 4 Mo)
    pub rom_high_present: [bool; 256],
    pub rom_low_enabled: bool,
    pub rom_high_enabled: bool,
    pub selected_high_rom: u8,
    pub ram_config: u8,
}

impl Memory {
    /// Crée une nouvelle mémoire propre de 128 Ko.
    pub fn new() -> Self {
        // Allocation directe sur le tas (heap) pour éviter de saturer la pile (stack)
        let ram_vec = vec![0u8; 128 * 1024];
        let rom_low_vec = vec![0u8; 16 * 1024];

        // 256 banques de 16 Ko = 4 194 304 octets (4 Mo)
        let rom_high_vec = vec![0u8; 256 * 16 * 1024];

        Self {
            ram: ram_vec.into_boxed_slice(),
            rom_low: rom_low_vec.into_boxed_slice(),
            rom_high: rom_high_vec.into_boxed_slice(),
            rom_high_present: [false; 256],
            rom_low_enabled: true,
            rom_high_enabled: true,
            selected_high_rom: 0,
            ram_config: 0,
        }
    }

    /// Charge la ROM basse (16 Ko).
    pub fn load_low_rom(&mut self, data: &[u8]) {
        let size = data.len().min(self.rom_low.len());
        self.rom_low[..size].copy_from_slice(&data[..size]);
    }

    /// Charge une ROM haute (16 Ko) à un index donné.
    pub fn load_high_rom(&mut self, index: u8, data: &[u8]) {
        let start = (index as usize) * 16 * 1024;
        let size = data.len().min(16 * 1024);
        self.rom_high[start..start + size].copy_from_slice(&data[..size]);
        self.rom_high_present[index as usize] = true; // On marque la ROM comme présente !
    }

    /// Sélectionne la ROM haute active (via écriture I/O à $DF00).
    pub fn select_high_rom(&mut self, index: u8) {
        self.selected_high_rom = index;
    }

    /// Retourne l'adresse physique dans les 128 Ko de RAM pour une adresse CPU donnée.
    pub fn get_ram_physical_address(&self, address: u16) -> usize {
        let page = (address / 0x4000) as usize;
        let offset = (address % 0x4000) as usize;

        let bank = match self.ram_config & 0x07 {
            0 => [0, 1, 2, 3][page],
            1 => [0, 1, 2, 7][page],
            2 => [4, 5, 6, 7][page],
            3 => [0, 3, 2, 7][page],
            4 => [0, 4, 2, 3][page],
            5 => [0, 5, 2, 3][page],
            6 => [0, 6, 2, 3][page],
            7 => [0, 7, 2, 3][page],
            _ => [0, 1, 2, 3][page],
        };

        (bank * 16384) + offset
    }

    /// Lecture directe de la RAM (utilisée par le moteur vidéo pour ignorer le banking ROM)
    /* pub fn read_ram_byte(&self, address: u16) -> u8 {
        let physical_addr = self.get_ram_physical_address(address);
        self.ram[physical_addr]
    } */

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
            let selected = self.selected_high_rom as usize;
            if self.rom_high_enabled && self.rom_high_present[selected] {
                let start = selected * 16 * 1024;
                let offset = (address - 0xC000) as usize;
                self.rom_high[start + offset]
            } else {
                let physical_addr = self.get_ram_physical_address(address);
                self.ram[physical_addr]
            }
        } else {
            // Zone Centrale ($4000 - $BFFF) est toujours de la RAM
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
