# Installation automatique des ROMs — implémentée

## Le problème

ByteBox ne fournit pas les ROMs système (OS, BASIC, AMSDOS, ROM de
diagnostic) : `Machine::load_roms` (`core/src/machine.rs`) attend qu'elles
soient déjà présentes dans `~/.bytebox/ROM/` (voir `config::default_resource_path`),
sans repli — décision explicite pour ne jamais distribuer de contenu dont
les droits ne sont pas clairs (voir `config/config.toml`, section `[rom]`).

Une demande de clarification a été envoyée à Amstrad ; pas de réponse à ce
jour. Décision retenue (voir plus bas) : proposer à l'utilisateur de les
télécharger lui-même, via un bouton dédié dans le panneau de configuration
(F6, onglet "ROMs"), plutôt qu'automatiquement et silencieusement à
l'installation.

## Sources retenues

Deux archives, chacune vérifiée manuellement avant d'être câblée dans le
code (contenu inspecté, stabilité du lien testée en conditions réelles) :

- **ROMs système AZERTY** (OS+BASIC combinés en un seul dump 32 Ko, plus
  AMSDOS) : <https://www.genesis8bit.fr/frontend/roms/azerty.zip> — fichier
  statique Apache ordinaire, aucune limitation de téléchargement constatée.
  Contenu déjà présent dans `~/.bytebox/ROM` de l'utilisateur au moment de
  l'implémentation (`CPC6128.ROM`, `CPCADOS.ROM`), comparé par hachage aux
  fichiers déjà installés manuellement (`AMSDOS.ROM`, `OS6128-AZERTY.rom`,
  origine antérieure inconnue) : `CPCADOS.ROM` et `amsdos.rom` (même
  archive) se sont révélés être des doublons strictement identiques par
  octet — l'archive `genesis8bit.fr` est bien la source déjà utilisée pour
  ces fichiers-là. Boot vérifié en conditions réelles (capture d'écran,
  bannière `BASIC 1.1` correcte) avant de retenir cette source dans le code.
- **ROM de diagnostic** : <https://github.com/llopis/amstrad-diagnostics/releases/download/v1.3/AmstradDiag.zip>
  — release GitHub, lien de redirection signé standard, stable. Seule
  l'entrée `AmstradDiagUpper.rom` en est extraite (les autres — ROM basse,
  `.cpr`/`.dsk`/`.cdt` — sont déjà suivies telles quelles dans `bin/`).

**Piste explorée puis abandonnée** : <https://cpcrulez.fr/f/14xp> (dumps
AZERTY, source suggérée initialement). Le lien s'est révélé peu fiable pour
un usage scripté : après un premier téléchargement réussi lors des tests,
toute requête suivante — y compris avec en-têtes navigateur complets et
cookies de session — renvoyait un corps vide, avec un
`Content-Disposition` littéralement préfixé `[AlreadyDownloaded]`,
signe d'une limite anti-script côté serveur (un seul téléchargement par
IP/fenêtre de temps). Un bouton "Install ROMs" construit autour de ce lien
aurait fonctionné une fois puis échoué silencieusement à toute nouvelle
tentative — y compris celle de l'utilisateur en conditions réelles.

## Verrouillage AZERTY : non un problème séparé à traiter

Le clavier virtuel (`bytebox/src/keyboard_panel.rs`, image
`assets/keyboard.png`) et plusieurs correspondances touche physique -> matrice
CPC codées en dur dans `core/src/psg.rs` (ex. `M` hôte -> `,` CPC, `Q`↔`A`,
`W`↔`Z`) sont calibrés spécifiquement pour la disposition AZERTY du 6128.
Une ROM QWERTY casserait donc la saisie. Contrairement à l'idée initiale
d'une validation par hachage contre une liste blanche, ce n'était finalement
pas nécessaire à implémenter séparément : les deux sources câblées dans le
code (`core/src/rom_installer.rs`) sont fixes et connues (AZERTY vérifié),
sans possibilité pour l'utilisateur de fournir une URL arbitraire — le
risque qu'une ROM QWERTY s'installe par ce chemin n'existe donc pas.

## Ce qui a été implémenté

- `core/src/rom_installer.rs` : téléchargement (`ureq`), extraction
  (`zip`), écriture sous le nom canonique attendu par `default_resource_path`
  (`OS6128-AZERTY.rom`, `AMSDOS.ROM`, `AmstradDiagUpper.rom` — aucun
  changement de `config.toml` nécessaire). Avant d'écraser un fichier déjà
  présent, son CRC32 (`crc32fast`) est comparé à celui du contenu
  fraîchement téléchargé et remonté à l'écran — répond directement à la
  question d'origine incertaine des fichiers déjà présents sur la machine
  de développement. La ROM de diagnostic, optionnelle (`Machine::load_roms`
  ne la charge qu'en mode diagnostic), échoue sans faire échouer
  l'installation des ROMs système si son téléchargement rate après coup.
- `bytebox/src/rom_install_panel.rs` : onglet "ROMs" du panneau F6 —
  avertissement légal (statut non tranché, tolérance généralement admise
  dans la communauté rétro-informatique), case à cocher obligatoire, bouton
  "Install ROMs" (téléchargement dans un thread dédié, jamais dans la
  boucle egui). Une fois l'installation terminée, envoie
  `MonitorCmd::PowerCycle` sur le canal existant — `Machine::power_cycle`
  recharge les ROMs et redémarre à froid, sans code spécifique côté
  `sdl.rs` pour le faire.
- `main.rs`/`sdl.rs` : `Machine::load_roms` peut désormais échouer sans
  empêcher le lancement de l'émulateur (jusqu'ici, l'échec arrêtait le
  programme avant même l'ouverture d'une fenêtre — aucun message
  exploitable pour qui que ce soit qui ne lit pas les journaux). Le
  panneau F6 s'ouvre alors automatiquement sur l'onglet "ROMs".

Vérifié en conditions réelles (voir la capture manuelle prise pendant le
développement, non conservée) : ROMs absentes au lancement -> panneau F6
routé automatiquement sur l'onglet ROMs -> installation -> power cycle
automatique -> BASIC démarre normalement, sans redémarrage manuel de
l'émulateur.

## Correctif : détection "déjà installées" basée sur `[rom]`, pas sur un CRC32 figé

Première implémentation : l'onglet ROMs affichait systématiquement le
formulaire, même quand tout était déjà en place. Corrigé une première fois
avec un `check_installed()` dans `rom_installer.rs`, comparant les fichiers
déjà présents dans `~/.bytebox/ROM` à une petite table de CRC32 attendus
(`EXPECTED_CRC32`), recalculée en téléchargeant réellement les deux sources.

Deux défauts constatés en usage réel, sur la propre machine de
l'utilisateur :

- **Ignorait `config.toml` `[rom]`** : `check_installed()` ne regardait que
  les chemins par défaut (`default_resource_path`), jamais une
  personnalisation. Un utilisateur ayant redirigé `[rom]` vers d'autres
  fichiers se serait vu proposer une réinstallation inutile — et pire, une
  fois installée, `check_installed()` continuerait à ignorer SES fichiers à
  lui, cochés comme "non installés" indéfiniment.
- **Trop strict sur l'origine** : l'`OS6128-AZERTY.rom` déjà présent sur la
  machine de développement (antérieur à cette fonctionnalité, origine
  inconnue mais parfaitement fonctionnel — l'émulateur démarre très bien
  avec) ne matchait pas le CRC32 de la source `genesis8bit.fr`, donc
  `check_installed()` répondait `Missing` alors que les ROMs étaient bel et
  bien installées et utilisées avec succès à chaque lancement.

**Correctif** : remplacé par `Machine::rom_status()` (`core/src/machine.rs`),
qui répond à une question différente et plus juste — "`load_roms`
réussirait-il avec la configuration actuelle ?" — plutôt que "ce fichier
vient-il bien de nos deux sources ?". Mêmes chemins que `load_roms` (`[rom]`
personnalisée comprise, via un nouveau `resolve_rom_path` partagé par les
deux), simple test d'existence (et de taille pour `system`, pour savoir si
`basic` compte aussi), aucun hachage. Une ROM d'ailleurs qui charge est
donc reconnue installée au même titre qu'une ROM posée par le bouton.

Le CRC32 n'a pas disparu : `install_from_zip` continue de comparer le
contenu fraîchement téléchargé à celui qu'il écrase (`InstalledFile::previous_crc32`),
affiché dans le résumé après une installation — utile pour savoir si on
vient de remplacer un fichier identique ou différent, mais plus jamais
utilisé pour décider si le formulaire doit s'afficher.

## Statut

Implémenté. Testé par un vrai téléchargement des deux sources
(`rom_installer::tests::install_everything_downloads_and_installs_from_the_real_sources`,
`#[ignore]` — ne s'exécute pas dans `cargo test` par défaut, seulement via
`-- --ignored`, pour ne jamais dépendre du réseau en usage courant ou en CI).
