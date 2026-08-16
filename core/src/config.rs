use directories::UserDirs;
use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    pub crt: CrtConfig,
}

/// Réglages du shader CRT (F5) enregistrés depuis le panneau F6.
///
/// Tout est `Option` : un champ absent laisse la valeur par défaut compilée
/// dans le shader (`CrtSettings::default`, côté binaire), un champ présent
/// l'outrepasse. C'est ce qui permet d'enregistrer un réglage partiel, et
/// surtout de faire évoluer les valeurs par défaut sans écraser silencieusement
/// le choix d'un utilisateur qui, lui, ne verrait rien changer.
///
/// `Serialize` en plus de `Deserialize`, contrairement au reste de ce
/// fichier : c'est la seule section que l'émulateur réécrit lui-même
/// (`save_crt_config`), le reste restant à la main de l'utilisateur.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
pub struct CrtConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask_cell_px: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask_min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask_strength: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scanline_beam: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scanline_strength: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beam_bloom: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bright_boost: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizontal_blur: Option<f32>,
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

/// Emplacement du fichier de configuration. En release (production) il vit
/// dans le répertoire personnel ; en debug, on prend celui du dépôt. On garde
/// un `PathBuf` de bout en bout plutôt que de repasser par une chaîne :
/// `to_str()` échoue sur un chemin qui n'est pas de l'UTF-8 valide, ce qui
/// faisait paniquer l'émulateur au démarrage pour un répertoire personnel
/// exotique.
pub fn config_path() -> Result<PathBuf, MachineError> {
    if cfg!(debug_assertions) {
        return Ok(PathBuf::from("config/config.toml"));
    }
    let user_dirs = UserDirs::new().ok_or(MachineError::ConfigFile)?;
    let mut cfg = user_dirs.home_dir().to_path_buf();
    cfg.push(".config/bytebox/config.toml");
    Ok(cfg)
}

pub fn load_config_file() -> Result<Config, MachineError> {
    let buf = fs::read_to_string(config_path()?)?;
    let config: Config = toml::from_str(&buf).map_err(|_e| MachineError::ConfigFileFmt)?;
    Ok(config)
}

/// Réécrit la seule section `[crt]` du fichier de configuration, en laissant
/// tout le reste — y compris les commentaires — intact.
///
/// Sérialiser `Config` en entier serait plus court, mais réécrirait le
/// fichier de l'utilisateur de bout en bout : commentaires perdus, sections
/// réordonnées, valeurs par défaut soudain écrites en dur. Pour un fichier
/// que l'utilisateur édite à la main, c'est un prix trop élevé.
pub fn save_crt_config(crt: &CrtConfig) -> Result<(), MachineError> {
    let path = config_path()?;
    // Fichier absent : on repart d'un contenu vide plutôt que d'échouer —
    // enregistrer ses réglages doit marcher même au tout premier lancement.
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let body = toml::to_string(crt).map_err(|_e| MachineError::ConfigFileFmt)?;
    fs::write(&path, replace_section(&existing, "crt", &body))?;
    Ok(())
}

/// Remplace le corps de la section TOML `section` par `body`, ou l'ajoute si
/// elle est absente. La section réécrite est toujours placée en fin de
/// fichier (l'ordre des sections n'a aucune importance en TOML) ; seul effet
/// de bord notable, un commentaire qui précédait immédiatement l'ancienne
/// section se retrouve rattaché à ce qui la suivait.
fn replace_section(content: &str, section: &str, body: &str) -> String {
    let header = format!("[{section}]");
    let mut kept: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == header {
            skipping = true;
            continue;
        }
        // Toute autre en-tête de section met fin au saut : on ne supprime que
        // le corps de celle qui nous intéresse.
        if skipping {
            if trimmed.starts_with('[') {
                skipping = false;
            } else {
                continue;
            }
        }
        kept.push(line);
    }
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }

    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(&header);
    out.push('\n');
    out.push_str(body.trim_end());
    out.push('\n');
    out
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
        assert_eq!(
            config.crt,
            CrtConfig::default(),
            "sans section [crt], aucun reglage ne doit outrepasser ceux du shader"
        );
    }

    /// Une section `[crt]` partielle doit rester valable : seuls les champs
    /// présents outrepassent les valeurs par défaut du shader, les autres
    /// restent absents (et donc à leur valeur compilée).
    #[test]
    fn a_partial_crt_section_only_overrides_what_it_names() {
        let file = "[drives]\ndrive_b = false\n\n[debugger]\nkeyboard = false\n\n[crt]\nscanline_beam = 9.0\nmask_cell_px = 2.0\n";
        let config: Config = toml::from_str(file).expect("fichier refuse");
        assert_eq!(config.crt.scanline_beam, Some(9.0));
        assert_eq!(config.crt.mask_cell_px, Some(2.0));
        assert_eq!(config.crt.beam_bloom, None);
        assert_eq!(config.crt.bright_boost, None);
    }

    /// Le tour complet enregistrement -> relecture : ce qui sort de
    /// `replace_section` doit rester un TOML valide qui redonne exactement
    /// les mêmes réglages. C'est ce test qui rattraperait l'oubli d'un champ
    /// dans `CrtConfig` si un réglage venait à s'ajouter au shader.
    #[test]
    fn a_saved_crt_section_reads_back_identically() {
        let crt = CrtConfig {
            mask_cell_px: Some(2.0),
            mask_min: Some(0.6),
            mask_strength: Some(0.35),
            scanline_beam: Some(9.0),
            scanline_strength: Some(0.6),
            beam_bloom: Some(0.66),
            bright_boost: Some(1.6),
            horizontal_blur: Some(0.5),
        };
        let original = "[drives]\ndrive_b = true\n\n[debugger]\nkeyboard = false\n";
        let body = toml::to_string(&crt).expect("serialisation refusee");
        let updated = replace_section(original, "crt", &body);
        let reread: Config = toml::from_str(&updated).expect("fichier reecrit invalide");
        assert_eq!(reread.crt, crt);
        assert!(reread.drives.drive_b, "le reste du fichier doit survivre");
    }

    /// Enregistrer deux fois de suite ne doit pas empiler deux sections
    /// `[crt]` (TOML refuserait le fichier), ni toucher aux commentaires que
    /// l'utilisateur a écrits ailleurs.
    #[test]
    fn saving_twice_replaces_the_section_and_keeps_comments() {
        let original =
            "# mon commentaire\n[drives]\ndrive_b = true\n\n[crt]\nmask_cell_px = 1.0\n\n[debugger]\nkeyboard = true\n";
        let once = replace_section(original, "crt", "mask_cell_px = 2.0\n");
        let twice = replace_section(&once, "crt", "mask_cell_px = 3.0\n");
        assert_eq!(
            twice.matches("[crt]").count(),
            1,
            "une seule section [crt] doit subsister"
        );
        assert!(twice.contains("mask_cell_px = 3.0"));
        assert!(!twice.contains("mask_cell_px = 1.0"));
        assert!(twice.contains("# mon commentaire"));
        assert!(
            twice.contains("keyboard = true"),
            "la section qui suivait [crt] ne doit pas etre emportee"
        );
        let reread: Config = toml::from_str(&twice).expect("fichier reecrit invalide");
        assert_eq!(reread.crt.mask_cell_px, Some(3.0));
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
            crt: CrtConfig::default(),
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

