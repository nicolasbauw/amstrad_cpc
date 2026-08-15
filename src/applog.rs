//! Journal applicatif global : remplace tout `println!`/`print!` destiné à
//! l'utilisateur, pour que plus rien n'atteigne le terminal qui a lancé
//! l'émulateur — seules les fenêtres console (F10/F11, voir
//! `console_panel.rs`/`console_window.rs`) doivent afficher quoi que ce
//! soit (retour d'usage après le premier jet de M2, Plan V2.md).
//!
//! File d'attente globale plutôt qu'une référence transportée à travers des
//! dizaines de signatures (`Fdc`, `Tape`, `Audio`, `Memory`, `Snapshot`...),
//! qui n'ont sinon aucune raison de connaître l'existence d'une console.
//! Les messages émis avant même l'ouverture des fenêtres (config au
//! démarrage, chargement `--disk` en ligne de commande...) s'accumulent
//! normalement : `sdl::run` vide la file dans `ConsoleLog` dès qu'elle
//! existe, rien n'est perdu.

use std::sync::Mutex;

static QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Ajoute une ligne au journal. Passe par la macro [`app_log!`] plutôt que
/// cette fonction directement, dans la plupart des cas.
pub fn log(message: String) {
    QUEUE.lock().unwrap().push(message);
}

/// Retire et renvoie tous les messages en attente, dans leur ordre d'ajout.
/// Appelé à chaque trame par `sdl::run`.
pub fn drain() -> Vec<String> {
    std::mem::take(&mut QUEUE.lock().unwrap())
}

/// Formate et journalise un message, avec la même syntaxe que `println!` —
/// à utiliser à sa place pour tout texte destiné à l'utilisateur.
#[macro_export]
macro_rules! app_log {
    ($($arg:tt)*) => {
        $crate::applog::log(format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `QUEUE` est un état global partagé par tout le processus de test :
    /// d'autres tests, exécutés en parallèle, y poussent aussi des messages
    /// (tout ce qui passe par `Machine::new()`, `load_disk`...). Des
    /// marqueurs peu susceptibles de collision, et une vérification par
    /// position relative plutôt qu'une égalité stricte de tout le vidage,
    /// gardent ce test fiable malgré cette interférence de fond.
    #[test]
    fn drain_returns_pushed_messages_in_order_and_empties_the_queue() {
        log("applog test marker A 87f3".to_string());
        log("applog test marker B 87f3".to_string());
        let drained = drain();
        let pos_a = drained
            .iter()
            .position(|m| m == "applog test marker A 87f3")
            .expect("marqueur A absent du vidage");
        let pos_b = drained
            .iter()
            .position(|m| m == "applog test marker B 87f3")
            .expect("marqueur B absent du vidage");
        assert!(pos_a < pos_b, "l'ordre d'ajout doit être préservé");
        assert!(
            !drain().iter().any(|m| m.contains("applog test marker")),
            "un second vidage ne doit pas revoir des messages déjà retirés"
        );
    }
}
