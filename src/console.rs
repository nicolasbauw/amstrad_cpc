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
                let command = match parts.next().unwrap_or_default().to_string().as_str() {
                    "h" => MonitorCmd::Help,
                    "r" => MonitorCmd::Registers,
                    "p" => MonitorCmd::Pause,
                    "g" => MonitorCmd::Resume,
                    "m" => MonitorCmd::ReadMem,
                    _ => MonitorCmd::Resume,
                };
                let arg = parts.next().unwrap_or_default().to_string();
                let arg2 = parts.next().unwrap_or_default().to_string();

                cmd_channel.send((command, arg, arg2))?;
                thread::sleep(Duration::from_millis(100));
            }
        },
    )?;
    Ok(())
}
