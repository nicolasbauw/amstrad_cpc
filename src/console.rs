use crate::monitor::MonitorMessage;
use std::{io::Write, io::stdin, io::stdout, sync::mpsc, thread, time::Duration};

use crate::machine::MachineError;

pub fn launch(cmd_channel: mpsc::Sender<MonitorMessage>) -> Result<(), MachineError> {
    thread::Builder::new().name(String::from("Console")).spawn(
        move || -> Result<(), mpsc::SendError<MonitorMessage>> {
            print!("> ");
            let _ = stdout().flush();

            loop {
                let mut input = String::new();
                match stdin().read_line(&mut input) {
                    // Fin de flux : aucun terminal n'est attaché (lancement
                    // depuis le raccourci desktop, stdin déjà fermée...).
                    // read_line() renvoie Ok(0) dans ce cas, PAS une erreur —
                    // sans ce test, la boucle ne bloquait jamais et tournait
                    // à vide en continu, saturant un cœur du CPU hôte. Le
                    // fil n'a plus rien à faire : mieux vaut s'arrêter
                    // silencieusement que consommer du CPU pour rien.
                    Ok(0) => return Ok(()),
                    Err(_) => continue,
                    Ok(_) => {}
                }

                // Une ligne vide (juste ENTRÉE) n'est pas une commande : on
                // ne veut ni afficher l'aide ni signaler "Unknown command"
                // à chaque appui distrait sur ENTRÉE. Comme aucune commande
                // n'est envoyée dans ce cas, rien d'autre ne redessinera le
                // prompt à sa place : on s'en charge nous-mêmes.
                if input.trim().is_empty() {
                    print!("> ");
                    let _ = stdout().flush();
                    continue;
                }

                cmd_channel.send(crate::monitor::parse_command(&input))?;
                thread::sleep(Duration::from_millis(100));
                // Pas de réimpression de "> " ici : elle arriverait avant même
                // que la commande n'ait été traitée. C'est `sdl::run` qui s'en
                // charge, une fois la sortie de la commande affichée (voir
                // `Machine::console_handle`).
            }
        },
    )?;
    Ok(())
}
