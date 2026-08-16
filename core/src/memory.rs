use crate::app_log;

/// Nombre standard de banques de 16 Ko d'un 6128 non étendu (128 Ko).
const STANDARD_RAM_BANKS: usize = 8;

/// Nombre maximal de "groupes" de RAM étendue adressables par le protocole
/// d'extension mémoire tierce (Dk'tronics et consorts) que reconnaît la ROM
/// de diagnostic : 7 sections de port (0x78-0x7E, la section 0x7F restant
/// le port standard 128 Ko, jamais réinterprété) × 8 banques chacune. Un
/// groupe = 4 sous-banques de 16 Ko = 64 Ko, l'unité affichée par la ROM de
/// diagnostic ("BANK 00".."BANK 3F") et par `config.toml` (`extra_ram_banks`).
pub const MAX_EXTRA_RAM_GROUPS: u32 = 7 * 8;

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
    /// Nombre de banques de 64 Ko supplémentaires configurées au-delà des
    /// 128 Ko standard (config.toml, [memory] extra_ram_banks), plafonné à
    /// `MAX_EXTRA_RAM_GROUPS`. Détermine la taille allouée pour `ram`
    /// au-delà des 8 banques standard.
    pub extra_ram_banks: u32,
    /// Sous-banque étendue actuellement mappée en page 1 (0x4000-0x7FFF), si
    /// la dernière écriture MMU a sélectionné une section/banque/bloc
    /// installé (voir `write_mmu_register`). `None` = comportement standard :
    /// la page 1 suit `ram_config`, comme sur un 6128 non étendu.
    extended_page1_bank: Option<usize>,
}

impl Memory {
    /// Crée une nouvelle mémoire propre : 128 Ko standard, plus
    /// `extra_ram_banks` banques de 64 Ko (silencieusement plafonné à
    /// `MAX_EXTRA_RAM_GROUPS`, avec avertissement, si dépassé — mieux vaut
    /// continuer avec le maximum utilisable que refuser de démarrer sur une
    /// valeur trop généreuse dans config.toml).
    pub fn new(extra_ram_banks: u32) -> Self {
        let extra_ram_banks = if extra_ram_banks > MAX_EXTRA_RAM_GROUPS {
            app_log!(
                "Config: memory.extra_ram_banks={extra_ram_banks} exceeds the addressable maximum ({MAX_EXTRA_RAM_GROUPS}), capped."
            );
            MAX_EXTRA_RAM_GROUPS
        } else {
            extra_ram_banks
        };

        // Allocation directe sur le tas (heap) pour éviter de saturer la pile (stack)
        let ram_size = (STANDARD_RAM_BANKS + extra_ram_banks as usize * 4) * 16384;
        let ram_vec = vec![0u8; ram_size];
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
            extra_ram_banks,
            extended_page1_bank: None,
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

    /// Index de la ROM haute réellement lue pour la sélection courante.
    ///
    /// Les ROMs d'extension décodent elles-mêmes leur numéro sur le bus. Quand
    /// aucune ne répond, personne ne pilote le bus et c'est la ROM BASIC interne
    /// (numéro 0) qui reste en place : un numéro inexistant renvoie donc la ROM 0
    /// répétée, et non du vide. C'est ce sur quoi comptent les routines qui
    /// parcourent les ROMs, en sautant les slots de type &80 justement parce
    /// qu'ils sont "la ROM BASIC répétée ailleurs".
    ///
    /// Renvoyer 0xFF ferait passer un slot vide pour une ROM d'extension valide,
    /// dans l'en-tête de laquelle le firmware irait chercher un point d'entrée
    /// inexistant.
    pub fn effective_high_rom(&self) -> Option<usize> {
        let selected = self.selected_high_rom as usize;
        if self.rom_high_present[selected] {
            Some(selected)
        } else if self.rom_high_present[0] {
            Some(0)
        } else {
            None
        }
    }

    /// Retourne l'adresse physique dans `ram` pour une adresse CPU donnée.
    pub fn get_ram_physical_address(&self, address: u16) -> usize {
        let page = (address / 0x4000) as usize;
        let offset = (address % 0x4000) as usize;

        // Une extension mémoire installée qui a pris la main sur la page 1
        // (0x4000-0x7FFF) prime sur le config standard, exactement comme sur
        // le vrai matériel où c'est alors elle qui pilote le bus de données
        // pour cette page — voir `write_mmu_register`.
        if page == 1
            && let Some(extended) = self.extended_page1_bank
        {
            return (STANDARD_RAM_BANKS * 16384) + extended * 16384 + offset;
        }

        let bank = self.standard_bank_mapping()[page];

        (bank as usize * 16384) + offset
    }

    /// Correspondance page logique (0..4, 16 Ko chacune) -> banque physique
    /// pour la configuration RAM standard courante (`ram_config`) — la même
    /// table que `get_ram_physical_address` ci-dessus, ordonnée pareil.
    /// Utilisée aussi par le panneau de statut (F12) pour afficher un
    /// mapping lisible plutôt que le seul numéro de config brut.
    pub fn standard_bank_mapping(&self) -> [u8; 4] {
        match self.ram_config & 0x07 {
            0 => [0, 1, 2, 3],
            1 => [0, 1, 2, 7],
            2 => [4, 5, 6, 7],
            3 => [0, 3, 2, 7],
            4 => [0, 4, 2, 3],
            5 => [0, 5, 2, 3],
            6 => [0, 6, 2, 3],
            7 => [0, 7, 2, 3],
            _ => [0, 1, 2, 3],
        }
    }

    /// Sous-banque étendue actuellement mappée en page 1 (0x4000-0x7FFF),
    /// voir le champ `extended_page1_bank` — `None` si la page 1 suit
    /// simplement `ram_config` (comportement standard, pas d'extension
    /// active).
    pub fn extended_page1_bank(&self) -> Option<usize> {
        self.extended_page1_bank
    }

    /// Traite une écriture au registre MMU (bits 7-6 de la valeur à 1, voir
    /// `bus::write_io`) : met à jour la configuration RAM standard, et
    /// simule une extension mémoire tierce si le port et la valeur
    /// correspondent à une banque installée.
    ///
    /// Le vrai Gate Array d'un 6128 ne décode que les bits 15-14 du port :
    /// il réagit donc à `value & 0x07` quelle que soit l'adresse précise
    /// dans 0x4000-0x7FFF (pas seulement 0x7Fxx, la forme habituelle),
    /// exactement comme ci-dessous. Une extension mémoire tierce (façon
    /// Dk'tronics), elle, décode EN PLUS les bits bas de l'octet haut du
    /// port pour se distinguer de la base et des autres cartes : c'est le
    /// protocole que reconnaît la ROM de diagnostic (secteur "UPPER RAM
    /// TEST", banques affichées "00".."3F"), reproduit ici. Chaque port
    /// 0x78xx-0x7Exx (7 valeurs, la 8e — 0x7Fxx — restant le port standard)
    /// est une "section" de 8 banques (bits 5-3 de la valeur) de 4 blocs de
    /// 16 Ko chacune (bits 1-0) : un groupe = 4 blocs = 64 Ko, l'unité de
    /// `extra_ram_banks`.
    pub fn write_mmu_register(&mut self, port: u16, value: u8) {
        self.ram_config = value & 0x07;

        let port_high = (port >> 8) as u8;
        self.extended_page1_bank = if (0x78..0x7F).contains(&port_high) {
            let section = (0x7E - port_high) as u32; // 0x7E -> 0, ..., 0x78 -> 6
            let bank = ((value >> 3) & 0x07) as u32;
            let block = (value & 0x03) as u32;
            let group = section * 8 + bank; // 0..MAX_EXTRA_RAM_GROUPS
            (group < self.extra_ram_banks).then_some((group * 4 + block) as usize)
        } else {
            // Port standard (0x7Fxx) ou hors du protocole d'extension connu :
            // aucune carte ne répond, la page 1 suit le config de base.
            None
        };
    }

    /// Lecture directe de la RAM (utilisée par le moteur vidéo pour ignorer le banking ROM)
    pub fn read_ram_byte(&self, address: u16) -> u8 {
        let physical_addr = self.get_ram_physical_address(address);
        self.ram[physical_addr]
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
                match self.effective_high_rom() {
                    Some(rom) => {
                        let start = rom * 16 * 1024;
                        let offset = (address - 0xC000) as usize;
                        self.rom_high[start + offset]
                    }
                    None => 0xFF,
                }
            } else {
                // ROM haute désactivée : RAM classique (banking normal)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Sans banque étendue configurée (comportement par défaut, celui d'un
    /// 6128 non modifié), une écriture MMU sur n'importe lequel des 8 ports
    /// 0x78xx-0x7Fxx doit se comporter à l'identique : c'est justement parce
    /// que le vrai Gate Array ne décode que les bits 15-14 du port. Page 1
    /// doit donc toujours suivre le config standard, jamais une extension.
    #[test]
    fn without_extra_banks_every_port_behaves_like_the_standard_one() {
        for port_high in 0x78u16..=0x7F {
            let mut mem = Memory::new(0);
            mem.write_mmu_register(port_high << 8, 0xC4); // config standard 4 = [0,4,2,3]
            assert_eq!(
                mem.get_ram_physical_address(0x4000),
                4 * 16384,
                "port {port_high:#04X} : page 1 doit suivre le config standard"
            );
        }
    }

    /// Une banque étendue installée (section 0x7E, banque 0, bloc 0 —
    /// premier groupe configuré) doit remapper la page 1 sur une zone
    /// physique distincte des 128 Ko standard, sans les perturber : y écrire
    /// ne doit rien changer à ce qui est visible via le config standard.
    #[test]
    fn an_installed_extended_bank_remaps_page_one_without_touching_standard_ram() {
        let mut mem = Memory::new(1); // 1 groupe = section 0x7E, banque 0 seule installee

        // Ecrit un marqueur dans la banque standard 0 (page 0, toujours
        // presente) pour verifier plus tard qu'elle n'a pas bouge.
        mem.write_mmu_register(0x7F00, 0xC0); // config standard 0 = [0,1,2,3]
        mem.write_byte(0x0000, 0xAA);

        // Section 0x7E, banque 0, bloc 0 : premier groupe, installe.
        mem.write_mmu_register(0x7E00, 0xC4);
        let extended_addr = mem.get_ram_physical_address(0x4000);
        assert!(
            extended_addr >= 8 * 16384,
            "la page 1 doit pointer au-dela des 128 Ko standard, pas dedans"
        );
        mem.write_byte(0x4000, 0x55);

        // Le marqueur standard n'a pas bouge.
        mem.write_mmu_register(0x7F00, 0xC0);
        assert_eq!(mem.read_ram_byte(0x0000), 0xAA);
        assert_eq!(
            mem.read_ram_byte(0x4000),
            0,
            "page 1 en config standard 0 doit lire la banque physique 1, jamais touchee (encore a zero)"
        );

        // Et la donnee ecrite dans la banque etendue est bien retrouvee en
        // y retournant.
        mem.write_mmu_register(0x7E00, 0xC4);
        assert_eq!(mem.read_ram_byte(0x4000), 0x55);
    }

    /// Un groupe au-delà de `extra_ram_banks` (donc non installé) doit se
    /// comporter comme sur un 6128 non étendu : la page 1 retombe sur le
    /// config standard désigné par les bits bas de la valeur écrite, sans
    /// jamais toucher la zone étendue allouée.
    #[test]
    fn an_uninstalled_group_falls_back_to_the_standard_config() {
        let mut mem = Memory::new(1); // seul le groupe 0 (section 0x7E, banque 0) existe

        // Section 0x7E, banque 1 : groupe 1, jamais installe (extra_ram_banks=1).
        mem.write_mmu_register(0x7E00, 0xCC); // 0xC4 | (1<<3) = 0xCC
        assert_eq!(
            mem.get_ram_physical_address(0x4000),
            (0xCC_u8 & 0x07) as usize * 16384,
            "groupe non installe : page 1 doit suivre le config standard (bits bas de la valeur)"
        );
    }

    /// Revenir au port standard (0x7Fxx) doit rendre la main au config de
    /// base, même après avoir sélectionné une banque étendue : sinon la page
    /// 1 resterait bloquée sur l'extension après le `OUT (C),C` de reset que
    /// fait la ROM de diagnostic (et tout logiciel bien élevé) en fin de
    /// test.
    #[test]
    fn writing_to_the_standard_port_releases_the_extended_bank() {
        let mut mem = Memory::new(1);
        mem.write_mmu_register(0x7E00, 0xC4); // selectionne l'extension
        assert!(mem.get_ram_physical_address(0x4000) >= 8 * 16384);

        mem.write_mmu_register(0x7F00, 0xC0); // reset standard, comme le fait la ROM de diag
        assert_eq!(mem.get_ram_physical_address(0x4000), 16384);
    }

    /// Une valeur excessive dans config.toml ne doit pas faire échouer le
    /// démarrage : elle est plafonnée à ce que le protocole peut adresser.
    #[test]
    fn extra_ram_banks_is_capped_to_the_addressable_maximum() {
        let mem = Memory::new(MAX_EXTRA_RAM_GROUPS + 100);
        assert_eq!(mem.extra_ram_banks, MAX_EXTRA_RAM_GROUPS);
    }
}
