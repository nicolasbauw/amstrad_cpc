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
    // sdl.rs lit BYTEBOX_PACKAGED_BUILD via option_env! (icône avec ou sans
    // cadre "dev build", titre hash-vs-version). `rerun-if-env-changed`
    // SEUL ne suffit pas : il ne fait rejouer que CE build.rs, pas la
    // compilation de sdl.rs, qui lit la variable directement — Cargo est
    // censé suivre les env! /option_env! d'un fichier source via son propre
    // dep-info, mais ça s'est avéré peu fiable avec un cache externe entre
    // deux runs CI (Swatinem/rust-cache) : une AppImage construite avec
    // BYTEBOX_PACKAGED_BUILD=1 gardait quand même le cadre rouge de dev. On
    // fait donc PASSER la valeur par ce build script, comme BYTEBOX_GIT_HASH
    // ci-dessous — la sortie d'un build script fait, elle, toujours partie
    // du fingerprint de l'unité qui la consomme, sans ce genre de zone
    // grise.
    println!("cargo:rerun-if-env-changed=BYTEBOX_PACKAGED_BUILD");
    if std::env::var_os("BYTEBOX_PACKAGED_BUILD").is_some() {
        println!("cargo:rustc-env=BYTEBOX_PACKAGED_BUILD=1");
    }

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
