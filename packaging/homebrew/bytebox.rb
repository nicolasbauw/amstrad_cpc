class Bytebox < Formula
  desc "Amstrad CPC 6128 emulator"
  homepage "https://github.com/nicolasbauw/amstrad_cpc"
  url "https://github.com/nicolasbauw/amstrad_cpc/archive/refs/tags/2.0.0.tar.gz"
  sha256 "00cf0ec4b4bf90d4232f2f4c3418a7c0a2bb8ffa4daf7f0707e151290c7606b9"
  license "MIT"

  depends_on "pkg-config" => :build
  depends_on "rust" => :build
  depends_on "sdl2"

  def install
    # Marque ce build comme "officiel" : sans ça, l'icône de fenêtre/dock
    # porte un cadre rouge de "dev build" (voir README, "Development
    # builds"). --profile dist : profil réservé aux binaires distribués
    # (LTO, un seul codegen-unit), voir le Cargo.toml racine.
    ENV["BYTEBOX_PACKAGED_BUILD"] = "1"
    system "cargo", "build", "--profile", "dist", "--locked", "-p", "bytebox"
    bin.install "target/dist/bytebox"
  end

  test do
    # Pas de sous-commande "--version" ni "--help" côté ByteBox (voir
    # bytebox/src/main.rs) : lancer le binaire sans argument ouvrirait une
    # vraie fenêtre SDL2/GPU, ce qu'un `brew test` headless en CI ne peut
    # pas faire. Seule vérification possible ici : le binaire a bien été
    # installé et est exécutable.
    assert_predicate bin/"bytebox", :exist?
    assert_predicate bin/"bytebox", :executable?
  end
end
