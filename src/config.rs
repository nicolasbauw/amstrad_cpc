use directories::UserDirs;
use serde::Deserialize;
use std::fs;

use crate::machine::MachineError;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub drives: DriveConfig,
    pub debugger: Debugger,
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
