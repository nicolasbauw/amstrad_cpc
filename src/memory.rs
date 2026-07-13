/// Gestion de la mémoire de l'Amstrad CPC.
///
/// L'Amstrad CPC 464 possède 64 Ko de RAM, une ROM basse (OS) de 16 Ko,
/// et plusieurs ROMs hautes de 16 Ko configurables (BASIC, ROMs d'extension, Diag).
pub struct Memory {
    pub ram: Box<[u8; 64 * 1024]>, // 64 Ko de RAM (sur le tas pour éviter le stack overflow)
    pub rom_low: Box<[u8; 16 * 1024]>, // ROM basse de 16 Ko (sur le tas)
    pub rom_high: Box<[[u8; 16 * 1024]; 256]>, // 256 ROMs hautes de 16 Ko (sur le tas, 4 Mo)

    pub rom_low_enabled: bool, // ROM basse activée en lecture ($0000-$3FFF)
    pub rom_high_enabled: bool, // ROM haute activée en lecture ($C000-$FFFF)
    pub selected_high_rom: u8, // Index de la ROM haute actuellement sélectionnée
}

impl Memory {
    /// Crée une nouvelle mémoire propre avec les ROMs désactivées ou prêtes.
    pub fn new() -> Self {
        Self {
            ram: Box::new([0; 64 * 1024]),
            rom_low: Box::new([0; 16 * 1024]),
            // Utilisation d'un vecteur pour allouer proprement sur le tas avant de convertir en Box
            rom_high: Box::new([[0; 16 * 1024]; 256]),
            rom_low_enabled: true,  // Activée au boot
            rom_high_enabled: true, // Activée au boot
            selected_high_rom: 0,   // ROM 0 sélectionnée par défaut (BASIC)
        }
    }

    /// Charge la ROM basse (16 Ko).
    pub fn load_low_rom(&mut self, data: &[u8]) {
        let size = data.len().min(self.rom_low.len());
        self.rom_low[..size].copy_from_slice(&data[..size]);
    }

    /// Charge une ROM haute spécifique (16 Ko) à un index donné.
    pub fn load_high_rom(&mut self, index: u8, data: &[u8]) {
        let size = data.len().min(self.rom_high[index as usize].len());
        self.rom_high[index as usize][..size].copy_from_slice(&data[..size]);
    }

    /// Configure l'état du banking à partir de la commande d'I/O du Gate Array.
    /// Bit 0 : ROM basse (0 = activée, 1 = désactivée)
    /// Bit 1 : ROM haute (0 = activée, 1 = désactivée)
    pub fn configure_banking(&mut self, val: u8) {
        self.rom_low_enabled = (val & 0x01) == 0;
        self.rom_high_enabled = (val & 0x02) == 0;
    }

    /// Sélectionne la ROM haute active (généralement via l'écriture I/O à $DF00).
    pub fn select_high_rom(&mut self, index: u8) {
        self.selected_high_rom = index;
    }

    /// Lecture d'un octet en fonction du banking actif.
    pub fn read_byte(&self, address: u16) -> u8 {
        if address < 0x4000 {
            // Zone Basse ($0000 - $3FFF)
            if self.rom_low_enabled {
                self.rom_low[address as usize]
            } else {
                self.ram[address as usize]
            }
        } else if address >= 0xC000 {
            // Zone Haute ($C000 - $FFFF)
            if self.rom_high_enabled {
                self.rom_high[self.selected_high_rom as usize][(address - 0xC000) as usize]
            } else {
                self.ram[address as usize]
            }
        } else {
            // Zone Centrale ($4000 - $BFFF) est toujours de la RAM
            self.ram[address as usize]
        }
    }

    /// Écriture d'un octet. L'écriture se fait TOUJOURS dans la RAM.
    pub fn write_byte(&mut self, address: u16, value: u8) {
        self.ram[address as usize] = value;
    }
}
