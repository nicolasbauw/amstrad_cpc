/// Gestion de la mémoire de l'Amstrad CPC.
///
/// L'Amstrad CPC 464 possède de base 64 Ko de RAM et des ROMs système de 16 Ko.
/// Le système de banking permet de connecter ou déconnecter des ROMs sur certains espaces :
/// - ROM basse (OS) de 16 Ko mappée de $0000 à $3FFF en lecture seule si activée.
/// - ROM haute (BASIC ou autre) de 16 Ko mappée de $C000 à $FFFF en lecture seule si activée.
/// - La RAM de 64 Ko est accessible sur l'ensemble de l'espace d'adressage (et toujours en écriture).
pub struct Memory {
    pub ram: [u8; 64 * 1024],  // 64 Ko de RAM
    pub rom: [u8; 16 * 1024],  // Espace pour stocker une ROM de 16 Ko (OS ou Diagnostic)
    pub rom_low_enabled: bool, // Indique si la ROM basse ($0000-$3FFF) est connectée en lecture
}

impl Memory {
    /// Crée une nouvelle mémoire propre (remplie de zéros).
    pub fn new() -> Self {
        Self {
            ram: [0; 64 * 1024],
            rom: [0; 16 * 1024],
            rom_low_enabled: true, // Activée par défaut au démarrage pour le boot
        }
    }

    /// Charge une ROM de 16 Ko depuis un slice d'octets.
    pub fn load_rom(&mut self, data: &[u8]) {
        let size = data.len().min(self.rom.len());
        self.rom[..size].copy_from_slice(&data[..size]);
    }

    /// Lecture d'un octet en fonction du banking actif.
    pub fn read_byte(&self, address: u16) -> u8 {
        // Si la ROM basse est active, les lectures entre $0000 et $3FFF renvoient la ROM.
        if self.rom_low_enabled && address < 0x4000 {
            self.rom[address as usize]
        } else {
            self.ram[address as usize]
        }
    }

    /// Écriture d'un octet. Sur l'Amstrad CPC, l'écriture se fait TOUJOURS dans la RAM,
    /// même si une ROM est connectée en lecture sur la même zone.
    pub fn write_byte(&mut self, address: u16, value: u8) {
        self.ram[address as usize] = value;
    }
}
