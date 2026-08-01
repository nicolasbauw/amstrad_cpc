use directories::UserDirs;
use serde_derive::Deserialize;
use std::{fs, path::PathBuf};

use crate::machine::MachineError;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub drives: DriveConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DriveConfig {
    pub drive_b: bool,
}

pub fn load_config_file() -> Result<Config, MachineError> {
    let user_dirs = UserDirs::new().ok_or(MachineError::ConfigFile);
    let mut cfg = user_dirs?.home_dir().to_path_buf();
    cfg.push(".config/dart/config.toml");
    // Absolute path fo release (production) build
    let config_path = if cfg!(debug_assertions) {
        "config/config.toml"
    } else {
        cfg
    };
    let buf = fs::read_to_string(cfg)?;
    let config: Config = toml::from_str(&buf).map_err(|_e| MachineError::ConfigFileFmt)?;
    Ok(config)
}
