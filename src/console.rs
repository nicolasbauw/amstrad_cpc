use crate::monitor::MonitorCmd;
use std::{io::Write, io::stdin, io::stdout, sync::mpsc, thread, time::Duration};

use crate::machine::MachineError;

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
                    _ => MonitorCmd::Help,
                };

                cmd_channel.send((command, arg, arg2))?;
                thread::sleep(Duration::from_millis(100));
            }
        },
    )?;
    Ok(())
}
