/// Une commande et ses deux arguments textuels, telle qu'elle circule sur le
/// canal entre la façade qui la saisit et `Machine` qui l'exécute. Ce triplet
/// est le point d'entrée unique pour tout ce qui modifie l'état de la machine
/// en cours d'exécution (voir `Plan V2.md`) : le nommer évite de le réécrire
/// en toutes lettres à chaque extrémité du canal.
pub type MonitorMessage = (MonitorCmd, String, String);

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
}
