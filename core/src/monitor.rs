/// Une commande et ses deux arguments textuels, telle qu'elle circule sur le
/// canal entre la façade qui la saisit et `Machine` qui l'exécute. Ce triplet
/// est le point d'entrée unique pour tout ce qui modifie l'état de la machine
/// en cours d'exécution (voir `Plan V2.md`) : le nommer évite de le réécrire
/// en toutes lettres à chaque extrémité du canal.
pub type MonitorMessage = (MonitorCmd, String, String);

/// Traduit une ligne de commande texte (telle que tapée sur `stdin` ou dans
/// le panneau console F11) en `MonitorMessage`. Point de correspondance
/// unique entre les deux façades (Plan V2.md, jalon M2) : `console.rs` et le
/// panneau F11 doivent reconnaître exactement les mêmes commandes, sans
/// entretenir séparément la même liste `match`.
pub fn parse_command(line: &str) -> MonitorMessage {
    let mut parts = line.split_whitespace();
    let cmd_part = parts.next().unwrap_or_default();
    let arg = parts.next().unwrap_or_default().to_string();
    let arg2 = parts.next().unwrap_or_default().to_string();

    let command = match cmd_part {
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
        "snap" => MonitorCmd::Snapshot,
        "pc" => MonitorCmd::PowerCycle,
        "t" => MonitorCmd::Trace,
        "mr" => MonitorCmd::ReadRam,
        "vol" | "volume" => MonitorCmd::Volume,
        "driveb" => MonitorCmd::DriveB,
        "ram" => MonitorCmd::ExtraRamBanks,
        "tapevol" => MonitorCmd::TapeAmplitude,
        "diag" => MonitorCmd::DiagnosticMode,
        _ => MonitorCmd::Unknown,
    };

    (command, arg, arg2)
}

pub enum MonitorCmd {
    Help,
    Unknown,
    ReadMem,
    WriteMem,
    SearchMem,
    Jump,
    Step,
    StepLine,
    ListBreakpoints,
    AddBreakpoint,
    RemoveBreakpoint,
    Registers,
    Hardware,
    Resume,
    Pause,
    Disassemble,
    AddWatchpoint,
    ListWatchpoints,
    RemoveWatchpoint,
    Disk,
    Blank,
    Tape,
    Snapshot,
    PowerCycle,
    Trace,
    ReadRam,
    Volume,
    /// Active/désactive le lecteur B à chaud ("driveb on"/"driveb off") —
    /// contrairement à `config.toml [drives] drive_b`, effet immédiat, sans
    /// redémarrage. Introduite pour le panneau de configuration (F6, Plan
    /// V2.md jalon M3).
    DriveB,
    /// Change le nombre de banques de RAM étendue ("ram <n>"). Ne peut pas
    /// avoir d'effet immédiat : `Memory` est dimensionnée à la construction
    /// (voir `Machine::power_on`), donc la nouvelle valeur ne s'applique
    /// qu'au prochain cycle d'alimentation (`pc`).
    ExtraRamBanks,
    /// Amplitude du signal cassette réinjecté dans le mixage audio
    /// ("tapevol <0-100>"), reprise du TODO v1 comme réglage F6 (Plan V2.md
    /// jalon M3). Distincte du volume de sortie global ("vol").
    TapeAmplitude,
    /// Active/désactive la ROM de Diagnostic Amstrad, en ROM haute 15
    /// ("0F") ("diag on"/"diag off"). Comme `ExtraRamBanks`, ne peut
    /// s'appliquer qu'au prochain cycle d'alimentation : les ROMs sont
    /// chargées par `Machine::load_roms`, appelée depuis `power_on`, pas à
    /// la volée pendant que le Z80 tourne.
    DiagnosticMode,
}
