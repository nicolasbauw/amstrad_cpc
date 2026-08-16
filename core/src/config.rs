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
    #[serde(default)]
    pub keyboard: KeyboardConfig,
}

/// Réglages du clavier virtuel (F7) enregistrés depuis le panneau F6, sur le
/// même modèle que `CrtConfig` ci-dessous (un seul champ pour l'instant,
/// mais même schéma `Option`/enregistrement partiel/`save_*` dédié, pour
/// rester cohérent si d'autres réglages du panneau F7 s'y ajoutent).
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
pub struct KeyboardConfig {
    /// Taille par défaut à l'ouverture du panneau F7, en fraction de la
    /// hauteur de la fenêtre CPC (0.0..=1.0) — voir `KeyboardSettings`
    /// (`bytebox::keyboard_panel`) pour le rôle exact de cette valeur.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_size_percent: Option<f32>,
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
    /// Si `true`, le shader est actif dès le lancement de l'émulateur, sans
    /// attendre un F5. Absent ou `false` : comportement d'origine (shader
    /// désactivé au démarrage). Contrairement aux huit champs ci-dessus, ne
    /// participe pas au rendu (`CrtSettings`/`CrtParams`, uniforme GPU) —
    /// c'est un simple bool que `sdl.rs` lit une fois au lancement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_at_startup: Option<bool>,
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
    /// Même rôle que `dsk_path` ci-dessus, mais pour les images cassette
    /// (console `tape`) — un répertoire séparé plutôt que réutiliser
    /// `dsk_path` : les deux types d'images vivent dans des dossiers
    /// distincts (`~/.bytebox/DSK` et `~/.bytebox/CDT`).
    #[serde(default)]
    pub cdt_path: Option<String>,
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

/// Étend un `~` ou `~/...` en tête de chemin vers le répertoire personnel de
/// l'utilisateur. Rust ne le fait pas tout seul, contrairement au shell :
/// `File::open("~/foo")` cherche littéralement un dossier nommé `~`, qui
/// n'existe jamais, et échoue en silence vers les valeurs par défaut du
/// programme — piège classique en configurant `config.toml` à la main.
/// `~utilisateur` (un autre utilisateur que soi) n'est volontairement pas
/// géré : bien plus rare, et demanderait une dépendance système
/// supplémentaire pour résoudre son répertoire personnel.
pub fn expand_tilde(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix('~') else {
        return PathBuf::from(path);
    };
    // "~foo" (un autre utilisateur) : volontairement pas notre "~", laissé
    // tel quel plutôt que de le tronquer à tort.
    if !rest.is_empty() && !rest.starts_with('/') {
        return PathBuf::from(path);
    }
    match UserDirs::new() {
        Some(user_dirs) => user_dirs.home_dir().join(rest.trim_start_matches('/')),
        None => PathBuf::from(path),
    }
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
            let candidate = expand_tilde(dir).join(filename);
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
            return expand_tilde(dir).join(filename).to_string_lossy().into_owned();
        }
        filename.to_string()
    }

    /// Résout un nom d'image cassette en un chemin utilisable — même logique
    /// que [`Config::resolve_disk_path`], mais via `cdt_path` plutôt que
    /// `dsk_path` : les deux types d'images ont chacun leur répertoire.
    pub fn resolve_tape_path(&self, filename: &str) -> String {
        if Path::new(filename).is_file() {
            return filename.to_string();
        }
        if let Some(dir) = &self.file.cdt_path {
            let candidate = expand_tilde(dir).join(filename);
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

/// Emplacement du fichier de configuration : toujours `~/.config/bytebox/
/// config.toml`, identique en debug et en release — `config/config.toml`, à
/// la racine du dépôt, n'est qu'un exemple de référence à copier là-bas,
/// jamais lu par le programme lui-même (voir son commentaire d'en-tête).
/// Une ancienne version de cette fonction lisait `config/config.toml` en
/// debug, un choix pratique en apparence mais qui rendait la résolution de
/// chemins (`dsk_path`, ROM...) différente entre profils de build — piège
/// pour peu qu'on veuille reproduire un comportement de production en
/// développement, ou tester la résolution de chemins elle-même.
///
/// On garde un `PathBuf` de bout en bout plutôt que de repasser par une
/// chaîne : `to_str()` échoue sur un chemin qui n'est pas de l'UTF-8 valide,
/// ce qui faisait paniquer l'émulateur au démarrage pour un répertoire
/// personnel exotique.
pub fn config_path() -> Result<PathBuf, MachineError> {
    let user_dirs = UserDirs::new().ok_or(MachineError::ConfigFile)?;
    let mut cfg = user_dirs.home_dir().to_path_buf();
    cfg.push(".config/bytebox/config.toml");
    Ok(cfg)
}

/// Chemin par défaut d'une ressource (ROM, image du clavier virtuel...) qui
/// n'a pas été explicitement configurée dans `config.toml` : toujours dans
/// `~/.bytebox/<sous-répertoire>/<nom>` — l'arborescence que les paquets
/// d'installation sont censés créer et peupler (voir `Plan V2.md`, jalon M6).
///
/// Aucun repli vers un chemin relatif au répertoire de lancement (une
/// ancienne version en avait un vers `bin/<nom>`, le dossier de
/// développement local) : décision explicite, pour que builds debug et
/// release se comportent identiquement, et pour ne pas masquer par
/// inadvertance une arborescence `~/.bytebox` incomplète — si une ROM y
/// manque, `load_roms` doit échouer franchement, pas retomber en silence sur
/// un dossier qui n'a de sens qu'en clone de développement local (`bin/`
/// n'est d'ailleurs même pas suivi par git : jamais présent dans une
/// installation réelle).
pub fn default_resource_path(subdir: &str, filename: &str) -> PathBuf {
    match UserDirs::new() {
        Some(user_dirs) => user_dirs.home_dir().join(".bytebox").join(subdir).join(filename),
        // Cas limite : pas de répertoire personnel détectable (utilisateur
        // système sans HOME, conteneur minimal...). Le chemin obtenu ne
        // désignera rien de réel, mais `File::open` échouera alors avec un
        // message exploitable plutôt que de faire semblant d'avoir résolu
        // quelque chose.
        None => PathBuf::from(filename),
    }
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

/// Réécrit la seule section `[keyboard]` du fichier de configuration —
/// même mécanisme que [`save_crt_config`] ci-dessus, voir son commentaire.
pub fn save_keyboard_config(keyboard: &KeyboardConfig) -> Result<(), MachineError> {
    let path = config_path()?;
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let body = toml::to_string(keyboard).map_err(|_e| MachineError::ConfigFileFmt)?;
    fs::write(&path, replace_section(&existing, "keyboard", &body))?;
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
            horizontal_blur: Some(0.75),
            enabled_at_startup: Some(true),
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

    /// Même mécanisme que `a_saved_crt_section_reads_back_identically`
    /// ci-dessus, pour la section `[keyboard]` (F7) — même fonction
    /// `replace_section`, même schéma `Option`, rien de spécifique à
    /// re-tester en profondeur.
    #[test]
    fn a_saved_keyboard_section_reads_back_identically() {
        let keyboard = KeyboardConfig {
            default_size_percent: Some(0.75),
        };
        let original = "[drives]\ndrive_b = true\n\n[debugger]\nkeyboard = false\n";
        let body = toml::to_string(&keyboard).expect("serialisation refusee");
        let updated = replace_section(original, "keyboard", &body);
        let reread: Config = toml::from_str(&updated).expect("fichier reecrit invalide");
        assert_eq!(reread.keyboard, keyboard);
        assert!(reread.drives.drive_b, "le reste du fichier doit survivre");
    }

    #[test]
    fn a_missing_keyboard_section_leaves_the_field_absent() {
        let file = "[drives]\ndrive_b = false\n\n[debugger]\nkeyboard = false\n";
        let config: Config = toml::from_str(file).expect("fichier refuse");
        assert_eq!(config.keyboard.default_size_percent, None);
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
                cdt_path: None,
            },
            display: DisplayConfig::default(),
            memory: MemoryConfig::default(),
            rom: RomConfig::default(),
            crt: CrtConfig::default(),
            keyboard: KeyboardConfig::default(),
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

    /// `cdt_path` doit avoir son propre repertoire, distinct de `dsk_path`
    /// (avant l'introduction de `cdt_path`, `load_tape` reutilisait
    /// `resolve_disk_path`/`dsk_path` — regression a eviter).
    #[test]
    fn resolve_tape_path_uses_cdt_path_not_dsk_path() {
        let config = Config {
            drives: DriveConfig { drive_b: false },
            debugger: Debugger {
                keyboard: false,
                audio: false,
            },
            file: FileConfig {
                dsk_path: Some("bin".to_string()),
                cdt_path: Some("dossier_cdt_qui_n_existe_pas".to_string()),
            },
            display: DisplayConfig::default(),
            memory: MemoryConfig::default(),
            rom: RomConfig::default(),
            crt: CrtConfig::default(),
            keyboard: KeyboardConfig::default(),
        };
        // AmstradDiag.cdt existe bien dans bin/ (dsk_path), mais
        // resolve_tape_path ne doit chercher que dans cdt_path : le nom doit
        // etre renvoye tel quel, pas recompose via dsk_path.
        assert_eq!(
            config.resolve_tape_path("AmstradDiag.cdt"),
            "AmstradDiag.cdt",
            "dsk_path ne doit pas servir de repli pour les cassettes"
        );
    }

    /// `bin/AmstradDiag.cdt` est suivi par git (contrairement au reste de
    /// `bin/`, ignore) : verifie la resolution contre un fichier reellement
    /// present sur toute machine qui clone le depot.
    #[test]
    fn resolve_tape_path_finds_a_real_file_via_cdt_path() {
        let config = Config {
            drives: DriveConfig { drive_b: false },
            debugger: Debugger {
                keyboard: false,
                audio: false,
            },
            file: FileConfig {
                dsk_path: None,
                cdt_path: Some("bin".to_string()),
            },
            display: DisplayConfig::default(),
            memory: MemoryConfig::default(),
            rom: RomConfig::default(),
            crt: CrtConfig::default(),
            keyboard: KeyboardConfig::default(),
        };
        assert_eq!(
            config.resolve_tape_path("AmstradDiag.cdt"),
            "bin/AmstradDiag.cdt"
        );
    }

    #[test]
    fn expand_tilde_replaces_a_leading_tilde_with_the_home_directory() {
        let home = UserDirs::new().expect("pas de repertoire personnel").home_dir().to_path_buf();
        assert_eq!(expand_tilde("~/.bytebox/DSK"), home.join(".bytebox/DSK"));
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn expand_tilde_leaves_other_paths_untouched() {
        assert_eq!(expand_tilde("bin"), PathBuf::from("bin"));
        assert_eq!(expand_tilde("/absolute/path"), PathBuf::from("/absolute/path"));
        assert_eq!(expand_tilde("../relative"), PathBuf::from("../relative"));
        // "~bob" (un autre utilisateur) : volontairement pas gere, doit
        // rester tel quel plutot que d'etre tronque a tort.
        assert_eq!(expand_tilde("~bob/dsk"), PathBuf::from("~bob/dsk"));
    }

    /// `resolve_disk_path`/`resolve_new_disk_path` doivent, elles aussi,
    /// developper un `dsk_path` commencant par `~` — c'est le bug concret
    /// signale : un config.toml avec `dsk_path = "~/.bytebox/DSK"` retombait
    /// silencieusement sur les chemins par defaut, `~` n'etant jamais un
    /// dossier reel.
    #[test]
    fn resolve_new_disk_path_expands_a_tilde_in_dsk_path() {
        let home = UserDirs::new().expect("pas de repertoire personnel").home_dir().to_path_buf();
        let config = Config {
            drives: DriveConfig { drive_b: false },
            debugger: Debugger {
                keyboard: false,
                audio: false,
            },
            file: FileConfig {
                dsk_path: Some("~/.bytebox/DSK".to_string()),
                cdt_path: None,
            },
            display: DisplayConfig::default(),
            memory: MemoryConfig::default(),
            rom: RomConfig::default(),
            crt: CrtConfig::default(),
            keyboard: KeyboardConfig::default(),
        };
        assert_eq!(
            config.resolve_new_disk_path("d.dsk"),
            home.join(".bytebox/DSK/d.dsk").to_string_lossy()
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



