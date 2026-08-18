class Bytebox < Formula
  desc "Amstrad CPC 6128 emulator"
  homepage "https://github.com/nicolasbauw/amstrad_cpc"
  url "https://github.com/nicolasbauw/amstrad_cpc/archive/refs/tags/2.0.0.tar.gz"
  sha256 "00cf0ec4b4bf90d4232f2f4c3418a7c0a2bb8ffa4daf7f0707e151290c7606b9"
  license "MIT"
  # Pour tester contre les derniers commits avant qu'un tag n'existe pour
  # eux (icns/wix/static-sdl2 ajoutés après le tag 2.0.0, par exemple) :
  # `brew install --HEAD ...` clone directement cette branche au lieu de
  # télécharger l'archive du tag ci-dessus. À retirer une fois cette
  # branche mergée et un nouveau tag créé pour elle.
  head "https://github.com/nicolasbauw/amstrad_cpc.git", branch: "dev"

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

    # Bundle .app, en plus du binaire nu dans bin/ : épingler ce dernier
    # tel quel à la barre des tâches macOS ne se comporte pas comme une
    # vraie appli (icône générique du Terminal, et le lancement passe par
    # lui) — macOS n'associe une icône et un comportement d'appli GUI qu'à
    # un vrai bundle .app, jamais à un exécutable Unix nu. Construit
    # nous-mêmes plutôt que via un Cask (qui suppose distribuer un binaire
    # pré-compilé, donc signé/notarié pour passer Gatekeeper) : celui-ci
    # est compilé localement par `brew`, jamais marqué "quarantaine" par
    # macOS (contrairement à un artefact téléchargé) — pas de notarization
    # à gérer.
    if OS.mac?
      app = prefix/"ByteBox.app"
      (app/"Contents/MacOS").mkpath
      (app/"Contents/Resources").mkpath
      ln_s bin/"bytebox", app/"Contents/MacOS/bytebox"
      cp "assets/bytebox_icon.icns", app/"Contents/Resources/bytebox.icns"
      (app/"Contents/Info.plist").write <<~XML
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0">
        <dict>
          <key>CFBundleName</key><string>ByteBox</string>
          <key>CFBundleDisplayName</key><string>ByteBox</string>
          <key>CFBundleIdentifier</key><string>net.bauw.bytebox</string>
          <key>CFBundleVersion</key><string>#{version}</string>
          <key>CFBundleShortVersionString</key><string>#{version}</string>
          <key>CFBundleExecutable</key><string>bytebox</string>
          <key>CFBundleIconFile</key><string>bytebox.icns</string>
          <key>CFBundlePackageType</key><string>APPL</string>
          <key>LSMinimumSystemVersion</key><string>11.0</string>
        </dict>
        </plist>
      XML
    end
  end

  def caveats
    return unless OS.mac?

    <<~EOS
      ByteBox s'installe aussi comme bundle macOS (#{prefix}/ByteBox.app),
      pour un lancement/épinglage normal depuis le Dock ou Launchpad — au
      lieu du binaire nu de #{bin}. Pour l'ajouter à Applications :

        ln -s "#{prefix}/ByteBox.app" /Applications/ByteBox.app
    EOS
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
