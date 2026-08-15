use directories::UserDirs;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::machine::MachineError;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub drives: DriveConfig,
    pub debugger: Debugger,
    /// Absent des fichiers de configuration existants : sans valeur par
    /// défaut, ajouter cette section rejetterait d'un bloc tout
    /// config.toml écrit avant elle.
    #[serde(default)]
    pub file: FileConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub rom: RomConfig,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct FileConfig {
    /// Répertoire où chercher une image disque désignée par son seul nom de
    /// fichier (console `disk`, option `--disk`), à la manière du
    /// `dsk_path` de Caprice32. N'intervient que si le nom donné ne désigne
    /// déjà un fichier existant tel quel : un chemin complet ou relatif au
    /// répertoire courant garde toujours la priorité.
    #[serde(default)]
    pub dsk_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct DisplayConfig {
    /// Niveau de zoom au démarrage : "x1", "x2", "x3" ou "fullscreen".
    /// Absent ou non reconnu, la fenêtre démarre en taille normale (x1) —
    /// voir `main.rs`, qui journalise un avertissement sur une valeur non
    /// reconnue plutôt que d'échouer au démarrage.
    #[serde(default)]
    pub default_zoom: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RomConfig {
    /// ROM basse (système/OS, 16 Ko). Certains dumps (ex. Caprice32,
    /// rom/cpc6128.rom) empaquettent le système et le BASIC en un seul
    /// fichier de 32 Ko (système dans la première moitié, BASIC dans la
    /// seconde) : dans ce cas, un fichier de 32 Ko ou plus donné ici est
    /// automatiquement découpé en deux, et `basic` ci-dessous est ignoré.
    #[serde(default)]
    pub system: Option<String>,
    /// ROM haute 0 (BASIC 1.1, 16 Ko). Ignoré si `system` désigne un
    /// fichier combiné de 32 Ko.
    #[serde(default)]
    pub basic: Option<String>,
    /// ROM haute 7 (AMSDOS, 16 Ko).
    #[serde(default)]
    pub amsdos: Option<String>,
    /// ROM haute 15 (Diagnostics Amstrad, 16 Ko), utilisée uniquement en
    /// mode diagnostic (`Machine::diagnostic_mode`).
    #[serde(default)]
    pub diagnostic_upper: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct MemoryConfig {
    /// Nombre de banques de 64 Ko supplémentaires au-delà des 128 Ko
    /// standard du 6128. Visibles par le protocole d'extension mémoire
    /// tierce (Dk'tronics et consorts) que reconnaît la ROM de diagnostic —
    /// voir `memory::Memory::write_mmu_register`. 0 par défaut (comportement
    /// standard, inchangé) ; plafonné à `memory::MAX_EXTRA_RAM_GROUPS` (avec
    /// avertissement) si la valeur donnée dépasse ce que le protocole peut
    /// adresser.
    #[serde(default)]
    pub extra_ram_banks: u32,
}

impl Config {
    /// Résout un nom de disquette en un chemin utilisable.
    ///
    /// Le nom donné est essayé tel quel en premier : c'est ce qui permet à
    /// un chemin absolu ou relatif au répertoire de lancement de continuer à
    /// fonctionner sans surprise. Ce n'est que s'il ne désigne aucun fichier
    /// existant, et qu'un `dsk_path` est configuré, qu'on cherche dans ce
    /// répertoire. Si rien ne convainc, le nom d'origine est renvoyé tel
    /// quel : le message d'erreur du chargeur de disque reste ainsi
    /// exploitable plutôt que de désigner un chemin recomposé surprenant.
    pub fn resolve_disk_path(&self, filename: &str) -> String {
        if Path::new(filename).is_file() {
            return filename.to_string();
        }
        if let Some(dir) = &self.file.dsk_path {
            let candidate = Path::new(dir).join(filename);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
        filename.to_string()
    }

    /// Résout le chemin où créer une *nouvelle* disquette (commande `blank`).
    ///
    /// Contrairement à [`Config::resolve_disk_path`], le fichier n'existe pas
    /// encore : on ne peut donc pas se fier à sa présence pour décider où le
    /// placer. Un nom qui contient déjà un séparateur de chemin (ou qui est
    /// absolu) est laissé tel quel — il désigne explicitement un emplacement.
    /// Un simple nom de fichier est en revanche créé dans `dsk_path` s'il est
    /// configuré, à la manière de `resolve_disk_path` pour la lecture.
    pub fn resolve_new_disk_path(&self, filename: &str) -> String {
        let has_explicit_dir = Path::new(filename)
            .parent()
            .is_some_and(|p| !p.as_os_str().is_empty());
        if has_explicit_dir {
            return filename.to_string();
        }
        if let Some(dir) = &self.file.dsk_path {
            return Path::new(dir).join(filename).to_string_lossy().into_owned();
        }
        filename.to_string()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DriveConfig {
    pub drive_b: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Debugger {
    pub keyboard: bool,
    /// Signale sur la console les interventions de la régulation audio.
    /// Absent des fichiers de configuration existants, d'où la valeur par
    /// défaut : ajouter une option ne doit pas rendre illisible un
    /// config.toml qui marchait.
    #[serde(default)]
    pub audio: bool,
}

pub fn load_config_file() -> Result<Config, MachineError> {
    let user_dirs = UserDirs::new().ok_or(MachineError::ConfigFile);
    let mut cfg = user_dirs?.home_dir().to_path_buf();
    cfg.push(".config/bytebox/config.toml");
    // En release (production), le fichier vit dans le répertoire personnel ;
    // en debug, on prend celui du dépôt. On garde un `PathBuf` de bout en
    // bout plutôt que de repasser par une chaîne : `to_str()` échoue sur un
    // chemin qui n'est pas de l'UTF-8 valide, ce qui faisait paniquer
    // l'émulateur au démarrage pour un répertoire personnel exotique.
    let config_path = if cfg!(debug_assertions) {
        PathBuf::from("config/config.toml")
    } else {
        cfg
    };
    let buf = fs::read_to_string(config_path)?;
    let config: Config = toml::from_str(&buf).map_err(|_e| MachineError::ConfigFileFmt)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une option ajoutée ne doit pas invalider les config.toml existants :
    /// sans valeur par défaut, tout fichier écrit avant cette version serait
    /// rejeté d'un bloc, et l'émulateur repartirait en configuration minimale
    /// (lecteur B désactivé) sans que la cause soit visible.
    #[test]
    fn an_older_file_without_the_audio_option_still_loads() {
        let older = "[drives]\ndrive_b = true\n\n[debugger]\nkeyboard = false\n";
        let config: Config = toml::from_str(older).expect("ancien fichier refuse");
        assert!(config.drives.drive_b);
        assert!(!config.debugger.keyboard);
        assert!(
            !config.debugger.audio,
            "l'option doit etre eteinte par defaut"
        );
        assert!(
            config.display.default_zoom.is_none(),
            "sans section [display], le zoom par defaut doit rester absent"
        );
        assert_eq!(
            config.memory.extra_ram_banks, 0,
            "sans section [memory], aucune banque supplementaire ne doit etre supposee"
        );
        assert!(
            config.rom.system.is_none(),
            "sans section [rom], les chemins doivent rester absents (secours code en dur)"
        );
    }

    #[test]
    fn rom_paths_are_read_when_present() {
        let file = "[drives]\ndrive_b = false\n\n[debugger]\nkeyboard = false\n\n[rom]\nsystem = \"custom/os.rom\"\nbasic = \"custom/basic.rom\"\namsdos = \"custom/amsdos.rom\"\ndiagnostic_upper = \"custom/diag.rom\"\n";
        let config: Config = toml::from_str(file).expect("fichier refuse");
        assert_eq!(config.rom.system.as_deref(), Some("custom/os.rom"));
        assert_eq!(config.rom.basic.as_deref(), Some("custom/basic.rom"));
        assert_eq!(config.rom.amsdos.as_deref(), Some("custom/amsdos.rom"));
        assert_eq!(
            config.rom.diagnostic_upper.as_deref(),
            Some("custom/diag.rom")
        );
    }

    #[test]
    fn the_default_zoom_is_read_when_present() {
        let file = "[drives]\ndrive_b = false\n\n[debugger]\nkeyboard = false\n\n[display]\ndefault_zoom = \"x2\"\n";
        let config: Config = toml::from_str(file).expect("fichier refuse");
        assert_eq!(config.display.default_zoom.as_deref(), Some("x2"));
    }

    #[test]
    fn extra_ram_banks_is_read_when_present() {
        let file = "[drives]\ndrive_b = false\n\n[debugger]\nkeyboard = false\n\n[memory]\nextra_ram_banks = 16\n";
        let config: Config = toml::from_str(file).expect("fichier refuse");
        assert_eq!(config.memory.extra_ram_banks, 16);
    }

    #[test]
    fn resolve_new_disk_path_joins_a_bare_filename_with_dsk_path() {
        let mut config = Config {
            drives: DriveConfig { drive_b: false },
            debugger: Debugger {
                keyboard: false,
                audio: false,
            },
            file: FileConfig {
                dsk_path: Some("bin".to_string()),
            },
            display: DisplayConfig::default(),
            memory: MemoryConfig::default(),
            rom: RomConfig::default(),
        };
        assert_eq!(config.resolve_new_disk_path("d.dsk"), "bin/d.dsk");

        config.file.dsk_path = None;
        assert_eq!(
            config.resolve_new_disk_path("d.dsk"),
            "d.dsk",
            "sans dsk_path configure, le nom est laisse tel quel"
        );

        config.file.dsk_path = Some("bin".to_string());
        assert_eq!(
            config.resolve_new_disk_path("other/d.dsk"),
            "other/d.dsk",
            "un chemin qui contient deja un repertoire ne doit pas etre recompose"
        );
    }

    #[test]
    fn the_audio_option_is_read_when_present() {
        let file = "[drives]\ndrive_b = false\n\n[debugger]\nkeyboard = true\naudio = true\n";
        let config: Config = toml::from_str(file).expect("fichier refuse");
        assert!(config.debugger.audio);
        assert!(config.debugger.keyboard);
        assert!(!config.drives.drive_b);
    }
}
