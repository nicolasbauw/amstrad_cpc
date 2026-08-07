use directories::UserDirs;
use serde::Deserialize;
use std::fs;
use std::path::Path;

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
    cfg.push(".config/dart/config.toml");
    // Absolute path fo release (production) build
    let config_path = if cfg!(debug_assertions) {
        "config/config.toml"
    } else {
        cfg.to_str().unwrap()
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
    fn the_audio_option_is_read_when_present() {
        let file = "[drives]\ndrive_b = false\n\n[debugger]\nkeyboard = true\naudio = true\n";
        let config: Config = toml::from_str(file).expect("fichier refuse");
        assert!(config.debugger.audio);
        assert!(config.debugger.keyboard);
        assert!(!config.drives.drive_b);
    }
}
