mod audio;
mod autotype;
mod bus;
mod config;
mod console;
mod console_panel;
mod crtc;
mod fdc;
mod gate_array;
mod hexconversion;
mod machine;
mod memory;
mod monitor;
mod ppi;
mod psg;
mod renderer;
mod sdl;
mod status_panel;
mod snapshot;
mod sound;
mod tape;
mod trace;
mod video;

use machine::Machine;
use std::env;

/// Cherche la valeur d'une option `--nom=valeur`, `--nom valeur`, `-nom=valeur`
/// ou `-nom valeur` parmi les arguments de la ligne de commande. `names`
/// donne les formes acceptées (par ex. `["--autocmd", "-a"]`), pour ne pas
/// obliger l'utilisateur à retenir laquelle est la forme longue.
fn cli_value(args: &[String], names: &[&str]) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        for name in names {
            if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
                return Some(v.to_string());
            }
            if arg == name {
                return args.get(i + 1).cloned();
            }
        }
    }
    None
}

/// Une commande `--autocmd` sans retour à la ligne resterait tapée mais
/// jamais validée : BASIC ne l'exécute qu'après ENTRÉE. On l'ajoute donc
/// systématiquement, sauf si l'appelant l'a déjà fournie — pour ne pas
/// envoyer un second ENTRÉE parasite, qui pourrait interagir avec ce que le
/// jeu affiche juste après (un menu, un choix de touche...).
fn ensure_validated(command: &str) -> String {
    if command.ends_with('\n') {
        command.to_string()
    } else {
        format!("{command}\n")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Amstrad CPC 6128 ===");

    // 1. Analyse des arguments de la ligne de commande pour le choix du mode
    let args: Vec<String> = env::args().collect();
    let mut diag_mode = false; // Par défaut, on démarre en mode normal

    if args.contains(&"--diag".to_string()) {
        diag_mode = true;
    }

    // Injection d'une commande BASIC au démarrage, comme le fait Caprice32
    // avec --autocmd. Utile pour lancer directement un jeu sans repasser par
    // le clavier à chaque essai — le cas d'usage qui a motivé cette option
    // est justement le débogage répété d'un même jeu, disquette après
    // disquette, pendant les séances de mise au point.
    let autocmd = cli_value(&args, &["--autocmd", "-a"]);

    // Chargement direct d'une image disque sur le lecteur A, sans passer par
    // la console. "-d" (forme courte) est accepté en plus de la forme longue
    // habituelle, pour rester proche de la syntaxe Caprice32.
    let disk = cli_value(&args, &["--disk", "-d"]);

    // Chargement direct d'une image cassette dans le lecteur, même principe
    // que --disk.
    let tape = cli_value(&args, &["--tape", "-t"]);

    // 2. Initialisation de la Machine
    let mut machine = Machine::new();
    machine.diagnostic_mode = diag_mode;
    machine.load_roms()?;

    if let Some(path) = &disk {
        if let Err(e) = machine.load_disk(path) {
            println!("Can't load disk '{path}': {e}");
        }
        // Contrairement aux commandes tapées dans la console (dont la sortie
        // est suivie d'un réaffichage de "> " par `sdl::run`, voir
        // `Machine::console_handle`), ce message s'affiche ici avant même
        // que le fil console ait fini son propre prompt initial : sans ce
        // réaffichage, il resterait sans prompt visible en dessous.
        print!("> ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }

    if let Some(path) = &tape {
        if let Err(e) = machine.load_tape(path) {
            println!("Can't load tape '{path}': {e}");
        }
        print!("> ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }

    let autotyper = autocmd
        .as_deref()
        .map(|cmd| autotype::AutoTyper::new(&ensure_validated(cmd)));

    // 3. Fenêtrage SDL2 et boucle principale, jusqu'à la fermeture.
    sdl::run(machine, autotyper)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cas qui a échappé aux tests du module `autotype` : une commande
    /// donnée sans ENTRÉE final se tapait, mais ne validait jamais rien.
    /// Repéré à l'usage, pas par un test — d'où celui-ci.
    #[test]
    fn a_command_without_a_trailing_newline_gets_one() {
        assert_eq!(ensure_validated("RUN\"BARBA.I"), "RUN\"BARBA.I\n");
    }

    #[test]
    fn a_command_that_already_ends_with_a_newline_is_left_alone() {
        assert_eq!(ensure_validated("RUN\"BARBA.I\n"), "RUN\"BARBA.I\n");
    }

    #[test]
    fn an_empty_command_still_gets_a_newline() {
        assert_eq!(ensure_validated(""), "\n");
    }

    #[test]
    fn cli_value_accepts_the_equals_and_the_space_forms() {
        let args: Vec<String> = ["prog", "--autocmd=RUN\"A", "-d", "d.dsk"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert_eq!(
            cli_value(&args, &["--autocmd", "-a"]),
            Some("RUN\"A".to_string())
        );
        assert_eq!(
            cli_value(&args, &["--disk", "-d"]),
            Some("d.dsk".to_string())
        );
        assert!(!args.contains(&"--diag".to_string()));
    }
}
