use crate::monitor::MonitorCmd;
use std::{fmt::Display, io::Write, io::stdin, io::stdout, sync::mpsc, thread, time::Duration};

use crate::machine::MachineError;

/// Affiche une ligne côté console, puis réimprime le prompt "> " en dessous.
///
/// Le fil console (voir `launch`) n'imprime "> " qu'une fois avant de se
/// bloquer sur la saisie : tout message affiché entre-temps par un autre
/// fil (chargement disque, régulation audio...) le fait remonter hors de
/// vue sans jamais le redessiner, ce qui donne l'impression que la console
/// a cessé de répondre alors qu'elle attend toujours une entrée. À utiliser
/// pour ce genre de message ponctuel — pas pour une sortie de commande
/// console, déjà couverte par le réaffichage fait dans `sdl::run` après
/// `Machine::console_handle`.
pub fn notice(msg: impl Display) {
    println!("{msg}");
    print!("> ");
    let _ = stdout().flush();
}

pub fn launch(cmd_channel: mpsc::Sender<(MonitorCmd, String, String)>) -> Result<(), MachineError> {
    thread::Builder::new().name(String::from("Console")).spawn(
        move || -> Result<(), mpsc::SendError<(MonitorCmd, String, String)>> {
            loop {
                print!("> ");
                if stdout().flush().is_err() {
                    continue;
                };

                let mut input = String::new();
                if stdin().read_line(&mut input).is_err() {
                    continue;
                };

                let mut parts = input.split_whitespace();
                let cmd_part = parts.next().unwrap_or_default().to_string();
                let arg = parts.next().unwrap_or_default().to_string();
                let arg2 = parts.next().unwrap_or_default().to_string();

                // Une ligne vide (juste ENTRÉE) n'est pas une commande : on
                // ne veut ni afficher l'aide ni signaler "Unknown command"
                // à chaque appui distrait sur ENTRÉE.
                if cmd_part.is_empty() {
                    continue;
                }

                let command = match cmd_part.as_str() {
                    "h" | "help" => MonitorCmd::Help,
                    "r" => MonitorCmd::Registers,
                    "p" => MonitorCmd::Pause,
                    "g" => MonitorCmd::Resume,
                    "hw" => MonitorCmd::Hardware,
                    "l" => MonitorCmd::StepLine,
                    "n" => MonitorCmd::Step,
                    "d" => MonitorCmd::Disassemble,
                    "j" => MonitorCmd::Jump,
                    "f" => MonitorCmd::RemoveBreakpoint,
                    "fw" => MonitorCmd::RemoveWatchpoint,
                    "w" => {
                        if arg.is_empty() {
                            MonitorCmd::ListWatchpoints
                        } else {
                            MonitorCmd::AddWatchpoint
                        }
                    }
                    "b" => {
                        if arg.is_empty() {
                            MonitorCmd::ListBreakpoints
                        } else {
                            MonitorCmd::AddBreakpoint
                        }
                    }
                    "m" => {
                        if arg2.is_empty() {
                            MonitorCmd::ReadMem
                        } else {
                            MonitorCmd::WriteMem
                        }
                    }
                    "s" => MonitorCmd::SearchMem,
                    "disk" => MonitorCmd::Disk,
                    "blank" => MonitorCmd::Blank,
                    "pc" => MonitorCmd::PowerCycle,
                    "t" => MonitorCmd::Trace,
                    "mr" => MonitorCmd::ReadRam,
                    "vol" | "volume" => MonitorCmd::Volume,
                    _ => MonitorCmd::Unknown,
                };

                cmd_channel.send((command, arg, arg2))?;
                thread::sleep(Duration::from_millis(100));
            }
        },
    )?;
    Ok(())
}
