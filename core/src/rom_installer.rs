//! Téléchargement et installation des ROMs système (OS+BASIC combinés,
//! AMSDOS, ROM de diagnostic) dans `~/.bytebox/ROM` — voir
//! `doc/roms-installation.md` pour le contexte (statut légal non tranché,
//! "qui ne dit mot consent" en l'absence de réponse d'Amstrad) et
//! `bytebox::rom_install_panel` pour l'écran qui pilote ce module.
//!
//! Volontairement synchrone (pas d'exécuteur async) : chaque fonction ici
//! bloque le fil qui l'appelle le temps du téléchargement/de l'extraction —
//! c'est à l'appelant (l'écran F6, dans un thread dédié) de ne jamais
//! l'appeler depuis la boucle de rendu egui.
//!
//! Savoir si les ROMs sont déjà installées n'est PAS le rôle de ce module :
//! voir `Machine::rom_status`, qui répond à cette question en se basant sur
//! la configuration réelle (`config.toml` `[rom]`, y compris personnalisée),
//! pas sur l'origine des fichiers — une ROM d'ailleurs, tant qu'elle charge,
//! ne doit jamais faire réapparaître cet écran.

use crc32fast::hash as crc32;
use std::io::Read;
use std::path::PathBuf;

/// Un fichier installé avec succès, pour affichage/consignation côté écran
/// (résumé "X installé, Y octets, CRC32 Z, correspondait déjà / différait
/// de ce qui était présent").
#[derive(Debug, Clone)]
pub struct InstalledFile {
    /// Nom canonique dans `~/.bytebox/ROM` (voir `config::default_resource_path`).
    pub filename: String,
    pub bytes: usize,
    pub crc32: u32,
    /// CRC32 du fichier qui occupait déjà cet emplacement avant l'écrasement,
    /// s'il y en avait un — permet de signaler si une installation manuelle
    /// antérieure provenait bien de la même source (voir la discussion sur
    /// l'origine incertaine des fichiers déjà présents).
    pub previous_crc32: Option<u32>,
}

/// Une archive source : son URL, et la correspondance entre le nom de
/// chaque entrée dans le zip et le nom canonique sous lequel elle doit
/// atterrir dans `~/.bytebox/ROM` (voir `config::default_resource_path`,
/// utilisée telle quelle par `Machine::load_roms` sans configuration
/// supplémentaire).
struct RomSource {
    url: &'static str,
    /// (nom de l'entrée dans le zip, nom canonique de destination)
    entries: &'static [(&'static str, &'static str)],
}

/// OS+BASIC (dump combiné 32 Ko, `CPC6128.ROM` — `Machine::load_roms` le
/// détecte à sa taille et le scinde automatiquement, `basic` reste inutile)
/// et AMSDOS (`CPCADOS.ROM`, identique à `amsdos.rom` dans la même archive,
/// vérifié par comparaison d'octets avant de retenir cette source).
const AZERTY_ROMS: RomSource = RomSource {
    url: "https://www.genesis8bit.fr/frontend/roms/azerty.zip",
    entries: &[
        ("CPC6128.ROM", "OS6128-AZERTY.rom"),
        ("CPCADOS.ROM", "AMSDOS.ROM"),
    ],
};

/// Seule la ROM haute de diagnostic nous intéresse dans cette archive :
/// les autres entrées (ROM basse, .cpr/.dsk/.cdt) sont déjà suivies telles
/// quelles dans `bin/` (voir le README).
const DIAGNOSTIC_ROM: RomSource = RomSource {
    url: "https://github.com/llopis/amstrad-diagnostics/releases/download/v1.3/AmstradDiag.zip",
    entries: &[("AmstradDiagUpper.rom", "AmstradDiagUpper.rom")],
};

/// Télécharge une URL entière en mémoire. Pas de suivi de progression
/// (les archives visées font quelques dizaines de Ko, pas des Mo) : le
/// statut affiché par l'écran se limite à "téléchargement en cours" / "fait".
fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| format!("Download failed ({url}): {e}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read response ({url}): {e}"))?;
    Ok(bytes)
}

/// Extrait les entrées demandées d'une archive zip déjà en mémoire et les
/// écrit dans `~/.bytebox/ROM` sous leur nom canonique, en créant
/// l'arborescence si besoin (premier lancement : rien n'existe encore).
fn install_from_zip(
    zip_bytes: &[u8],
    entries: &[(&str, &str)],
) -> Result<Vec<InstalledFile>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| format!("Invalid zip archive: {e}"))?;

    let mut installed = Vec::new();
    for &(entry_name, dest_name) in entries {
        let mut content = Vec::new();
        {
            let mut entry = archive
                .by_name(entry_name)
                .map_err(|e| format!("Entry \"{entry_name}\" not found in the archive: {e}"))?;
            entry
                .read_to_end(&mut content)
                .map_err(|e| format!("Failed to read \"{entry_name}\": {e}"))?;
        }

        let dest_path: PathBuf = crate::config::default_resource_path("ROM", dest_name);
        if let Some(dir) = dest_path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
        }
        let previous_crc32 = std::fs::read(&dest_path).ok().map(|old| crc32(&old));
        std::fs::write(&dest_path, &content)
            .map_err(|e| format!("Can't write {}: {e}", dest_path.display()))?;

        installed.push(InstalledFile {
            filename: dest_name.to_string(),
            bytes: content.len(),
            crc32: crc32(&content),
            previous_crc32,
        });
    }
    Ok(installed)
}

/// Télécharge et installe une source (`RomSource`), en notifiant `progress`
/// avant chaque étape bloquante — l'écran s'en sert pour afficher où en est
/// l'installation, un fil dédié n'ayant sinon aucun moyen de le faire savoir
/// avant la toute fin.
fn install_source(
    source: &RomSource,
    progress: &mut dyn FnMut(&str),
) -> Result<Vec<InstalledFile>, String> {
    progress(&format!("Downloading {}...", source.url));
    let zip_bytes = download(source.url)?;
    progress("Extracting and installing...");
    install_from_zip(&zip_bytes, source.entries)
}

/// Séquence complète appelée par le bouton "Install ROMs" : les ROMs
/// système (OS+BASIC, AMSDOS) d'abord, obligatoires — un échec ici fait
/// échouer toute la fonction. La ROM de diagnostic ensuite, mais son échec
/// ne doit PAS faire échouer le reste : `Machine::load_roms` ne la charge
/// que si `diagnostic_mode` est actif (voir son commentaire), l'émulateur
/// démarre très bien sans elle. Un pépin réseau sur ce second
/// téléchargement, après que les ROMs essentielles ont déjà été écrites
/// avec succès sur le disque, ne doit donc pas se présenter comme un échec
/// total à l'écran.
pub fn install_everything(
    mut progress: impl FnMut(&str),
) -> Result<Vec<InstalledFile>, String> {
    let mut installed = install_source(&AZERTY_ROMS, &mut progress)?;
    match install_source(&DIAGNOSTIC_ROM, &mut progress) {
        Ok(diag) => installed.extend(diag),
        Err(e) => progress(&format!(
            "Diagnostic ROM install failed (optional, skipped): {e}"
        )),
    }
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Construit un petit zip en mémoire avec les entrées données, sans
    /// dépendre du réseau — c'est `install_from_zip` qui est testée ici, pas
    /// `download`.
    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for &(name, content) in entries {
                writer.start_file(name, options).unwrap();
                writer.write_all(content).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    /// Répertoire `~/.bytebox/ROM` isolé par test : `HOME` est global au
    /// processus, donc chaque test qui le modifie doit passer par un mutex
    /// pour ne jamais courir en parallèle d'un autre test qui en dépend —
    /// même précaution que les tests de `config.rs` sur `config_path`.
    fn with_isolated_home<R>(f: impl FnOnce(&std::path::Path) -> R) -> R {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap();

        let dir = std::env::temp_dir().join(format!(
            "bytebox_rom_installer_test_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let previous_home = std::env::var("HOME").ok();
        // Sûr : verrouillé ci-dessus contre toute exécution concurrente
        // d'un autre test qui lirait `HOME` pendant la fenêtre où il est
        // temporairement détourné.
        unsafe {
            std::env::set_var("HOME", &dir);
        }

        let result = f(&dir);

        unsafe {
            match &previous_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    #[test]
    fn install_from_zip_extracts_only_the_requested_entries_under_their_canonical_name() {
        with_isolated_home(|home| {
            let zip = build_zip(&[
                ("CPC6128.ROM", b"os+basic content"),
                ("CPCADOS.ROM", b"amsdos content"),
                ("FILE_ID.DIZ", b"ignored"),
            ]);

            let installed = install_from_zip(
                &zip,
                &[
                    ("CPC6128.ROM", "OS6128-AZERTY.rom"),
                    ("CPCADOS.ROM", "AMSDOS.ROM"),
                ],
            )
            .expect("l'installation doit reussir");

            assert_eq!(installed.len(), 2);
            assert_eq!(installed[0].filename, "OS6128-AZERTY.rom");
            assert_eq!(installed[0].bytes, "os+basic content".len());
            assert_eq!(installed[0].previous_crc32, None, "rien n'existait avant");

            let written = std::fs::read(home.join(".bytebox/ROM/OS6128-AZERTY.rom")).unwrap();
            assert_eq!(written, b"os+basic content");
            assert!(
                !home.join(".bytebox/ROM/FILE_ID.DIZ").exists(),
                "seules les entrees demandees doivent etre extraites"
            );
        });
    }

    #[test]
    fn install_from_zip_reports_the_previous_crc32_when_a_file_is_overwritten() {
        with_isolated_home(|home| {
            let rom_dir = home.join(".bytebox/ROM");
            std::fs::create_dir_all(&rom_dir).unwrap();
            std::fs::write(rom_dir.join("AMSDOS.ROM"), b"old content").unwrap();

            let zip = build_zip(&[("CPCADOS.ROM", b"new content")]);
            let installed =
                install_from_zip(&zip, &[("CPCADOS.ROM", "AMSDOS.ROM")]).expect("doit reussir");

            assert_eq!(
                installed[0].previous_crc32,
                Some(crc32(b"old content")),
                "le CRC32 de l'ancien fichier doit etre remonte avant l'ecrasement"
            );
            assert_eq!(installed[0].crc32, crc32(b"new content"));
        });
    }

    #[test]
    fn install_from_zip_fails_clearly_when_an_entry_is_missing() {
        with_isolated_home(|_home| {
            let zip = build_zip(&[("SOMETHING_ELSE.ROM", b"content")]);
            let err = install_from_zip(&zip, &[("CPC6128.ROM", "OS6128-AZERTY.rom")])
                .expect_err("l'entree demandee n'existe pas dans ce zip");
            assert!(err.contains("CPC6128.ROM"), "erreur peu exploitable : {err}");
        });
    }

    /// Test réseau réel, volontairement `#[ignore]` (même convention que
    /// `discology_copies_a_disk_track_by_track`) : ne doit jamais faire
    /// échouer `cargo test` sur une machine hors ligne ou en CI.
    #[test]
    #[ignore]
    fn install_everything_downloads_and_installs_from_the_real_sources() {
        with_isolated_home(|home| {
            let mut steps = Vec::new();
            let installed =
                install_everything(|msg| steps.push(msg.to_string())).expect("doit reussir");

            assert_eq!(installed.len(), 3, "OS+BASIC, AMSDOS, ROM de diagnostic");
            for file in &installed {
                assert!(file.bytes > 0);
                assert!(
                    home.join(".bytebox/ROM").join(&file.filename).exists(),
                    "{} doit exister apres l'installation",
                    file.filename
                );
            }
            assert!(!steps.is_empty(), "la progression doit etre rapportee");
        });
    }
}
