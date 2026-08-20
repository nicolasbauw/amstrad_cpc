use std::process::Command;

// Titre de fenêtre (sdl.rs) : "ByteBox - <hash>" pour un build depuis un
// checkout git (dépôt cloné, `cargo build` local ou packaging/PKGBUILD-git),
// "ByteBox - <version>" pour un build depuis l'archive d'une release taguée
// (packaging/PKGBUILD, qui construit depuis un tarball GitHub sans .git) —
// `git rev-parse` échoue naturellement dans ce dernier cas, d'où le fallback
// sur CARGO_PKG_VERSION côté sdl.rs plutôt qu'un test explicite ici.
fn main() {
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
    // sdl.rs lit aussi BYTEBOX_PACKAGED_BUILD via option_env! (icône avec ou
    // sans cadre "dev build") — sans cette ligne, Cargo ne sait pas que la
    // fraîcheur de sdl.rs dépend de cette variable externe (elle n'apparaît
    // dans aucun fichier suivi ci-dessus) et peut réutiliser un objet compilé
    // lors d'un run précédent SANS elle, restauré par un cache externe
    // (Swatinem/rust-cache en CI) — observé en pratique : une AppImage
    // construite avec BYTEBOX_PACKAGED_BUILD=1 gardait quand même le cadre
    // rouge de dev.
    println!("cargo:rerun-if-env-changed=BYTEBOX_PACKAGED_BUILD");

    if let Ok(output) = Command::new("git").args(["rev-parse", "HEAD"]).output()
        && output.status.success()
    {
        let hash = String::from_utf8_lossy(&output.stdout);
        let hash = hash.trim();
        // Les 7 DERNIERS caractères du hash complet, pas les 7 premiers
        // (= `git rev-parse --short`, l'abréviation habituelle) : demandé
        // tel quel, pour distinguer ce hash de version des hash courts déjà
        // utilisés ailleurs (logs de commit...).
        let suffix = &hash[hash.len().saturating_sub(7)..];
        println!("cargo:rustc-env=BYTEBOX_GIT_HASH={suffix}");
    }
}
