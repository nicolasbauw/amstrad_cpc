use crate::monitor::MonitorCmd;
use std::{io::Write, io::stdin, io::stdout, sync::mpsc, thread, time::Duration};

use crate::machine::MachineError;

pub fn launch(cmd_channel: mpsc::Sender<(MonitorCmd, String, String)>) -> Result<(), MachineError> {
    thread::Builder::new().name(String::from("Console")).spawn(
        move || -> Result<(), mpsc::SendError<(MonitorCmd, String, String)>> {
            print!("> ");
            let _ = stdout().flush();

            loop {
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
                // à chaque appui distrait sur ENTRÉE. Comme aucune commande
                // n'est envoyée dans ce cas, rien d'autre ne redessinera le
                // prompt à sa place : on s'en charge nous-mêmes.
                if cmd_part.is_empty() {
                    print!("> ");
                    let _ = stdout().flush();
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
                    "tape" => MonitorCmd::Tape,
                    "pc" => MonitorCmd::PowerCycle,
                    "t" => MonitorCmd::Trace,
                    "mr" => MonitorCmd::ReadRam,
                    "vol" | "volume" => MonitorCmd::Volume,
                    _ => MonitorCmd::Unknown,
                };

                cmd_channel.send((command, arg, arg2))?;
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
