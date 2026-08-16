# Installation automatique des ROMs — réflexion en cours, rien d'implémenté

## Le problème

ByteBox ne fournit pas les ROMs système (OS, BASIC, AMSDOS, ROM de
diagnostic) : `Machine::load_roms` (`core/src/machine.rs`) attend qu'elles
soient déjà présentes dans `~/.bytebox/ROM/` (voir `config::default_resource_path`),
sans repli — décision explicite pour ne jamais distribuer de contenu dont
les droits ne sont pas clairs (voir `config/config.toml`, section `[rom]`).

Une demande de clarification a été envoyée à Amstrad ; pas de réponse à ce
jour. En attendant, piste alternative : proposer à l'utilisateur de
télécharger les ROMs lui-même depuis une archive communautaire, soit
automatiquement à l'installation, soit via un bouton dédié dans le panneau
de configuration (F6).

## Source identifiée (non vérifiée par nous)

Selon une recherche de l'utilisateur, les dumps AZERTY système et BASIC du
CPC 464/6128 seraient légalement téléchargeables pour un usage d'émulation
depuis des archives communautaires type Genesis 8bit ou CPC rulez, qui
hébergent ces dumps au titre de la préservation rétro-informatique.

ROMs concrètement visées : <https://cpcrulez.fr/f/14xp>

**Non vérifié à ce stade** : le statut légal exact de cette autorisation
(qui l'accorde, sous quelles conditions, si elle couvre la redistribution
via un outil tiers comme un installeur ByteBox ou seulement le
téléchargement manuel par l'utilisateur final) reste à confirmer avant
d'implémenter quoi que ce soit qui automatise le téléchargement depuis
cette source.

## Deux options envisagées

1. **Téléchargement automatique à l'installation.**
2. **Bouton "Install ROMs" dans le panneau F6.**

L'option 2 est jugée préférable pour l'instant : démarche explicite de
l'utilisateur (pas de téléchargement déclenché sans qu'il l'ait
consciemment demandé), point naturel où afficher une source/un
avertissement avant de télécharger.

Mais aucune des deux n'est retenue tant que le point légal ci-dessus n'est
pas éclairci : c'est le blocage principal, pas la difficulté technique.

## Contraintes techniques déjà identifiées, à respecter le jour où on implémente

### Verrouillage AZERTY, pas une simple préférence

Le clavier virtuel (`bytebox/src/keyboard_panel.rs`, image
`assets/keyboard.png`) et plusieurs correspondances touche physique -> matrice
CPC codées en dur dans `core/src/psg.rs` (ex. `M` hôte -> `,` CPC, `Q`↔`A`,
`W`↔`Z`) sont calibrés spécifiquement pour la disposition AZERTY du 6128.
Installer une ROM QWERTY casserait donc la saisie sans qu'aucun autre code
n'ait besoin de changer pour que ça devienne visible — ce n'est pas une
restriction arbitraire, c'est une exigence de correction.

Recommandation (à confirmer le moment venu) : valider par **hachage
(SHA-256) du contenu téléchargé** contre une petite table de hachages connus
des ROMs AZERTY attendues, plutôt que par nom de fichier — protège aussi
contre un téléchargement corrompu ou mal étiqueté par l'archive source.

### Nommage variable selon les archives : déjà pris en charge

`config.toml` (`[rom]`) découple déjà le rôle logique de chaque ROM
(`system`, `basic`, `amsdos`, `diagnostic_upper`) de son nom de fichier
réel. Un futur installeur n'a donc qu'à enregistrer chaque fichier
téléchargé sous le nom canonique déjà utilisé par défaut
(`OS6128-AZERTY.rom`, `BASIC1-1-AZERTY.ROM`, `AMSDOS.ROM`,
`AmstradDiagUpper.rom`) dans `~/.bytebox/ROM/`, quel que soit le nom
d'origine côté archive — pas de changement de mécanisme nécessaire côté
config.

## Décision

L'utilisateur a contacté Amstrad pour clarifier le statut des ROMs ; en
l'absence de réponse d'ici à la mise en œuvre, il retient le principe "qui
ne dit mot consent" plutôt que d'attendre indéfiniment. L'option 2 (bouton
"Install ROMs" dans le panneau F6) est retenue comme direction cible,
plutôt que le téléchargement automatique à l'installation — mêmes raisons
qu'exposées plus haut (démarche explicite de l'utilisateur, point naturel
pour afficher la source et un avertissement).

## Statut

Direction actée (option 2), implémentation non commencée. À reprendre
quand le hachage des ROMs attendues aura été établi (voir "Verrouillage
AZERTY" ci-dessus) — c'est la seule étape technique qui reste avant de
pouvoir coder le bouton.
