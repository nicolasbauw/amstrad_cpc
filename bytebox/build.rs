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

    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        && output.status.success()
    {
        let hash = String::from_utf8_lossy(&output.stdout);
        println!("cargo:rustc-env=BYTEBOX_GIT_HASH={}", hash.trim());
    }
}
