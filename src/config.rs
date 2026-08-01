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
