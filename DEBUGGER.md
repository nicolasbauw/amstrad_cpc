# Moniteur de Contrôle et Debug (CLI)

L'émulateur intègre un moniteur en ligne de commande (exécuté dans son propre thread) qui communique avec la boucle principale via des canaux asynchrones (`std::sync::mpsc`). Il permet d'inspecter l'état interne de la machine sans bloquer le rendu graphique SDL2.

## Commandes Disponibles

| Commande | Syntaxe | Description |
| :--- | :--- | :--- |
| **Disassemble** | `d <addr>` | Désassemble l'instruction à l'adresse et les 20 suivantes. |
| **Memory Read** | `m <addr>` | Affiche le contenu de la mémoire (et indique la banque RAM physique sous-jacente). |
| **Memory Write**| `m <addr> <val>` | Écrit la valeur `<val>` à l'adresse `<addr>`. |
| **Jump** | `j <addr>` | Force le pointeur de programme (`PC`) du Z80 à l'adresse donnée. |
| **Step** | `s` | Exécute une seule instruction Z80, affiche ses registres et se remet en pause. |
| **Step Line** | `l` | Exécute l'équivalent d'une ligne de balayage CRTC (64 cycles), puis pause. |
| **Breakpoint** | `b <addr>` | Ajoute un point d'arrêt à l'adresse spécifiée. |
| **Free BP** | `f <addr>` | Supprime le point d'arrêt à l'adresse spécifiée. |
| **List BPs** | `b` | Liste tous les breakpoints actifs. |
| **Registers** | `r` | Affiche les registres Z80 (`AF`, `BC`, `DE`, `HL`, `IX`, `IY`, `PC`, `SP`) et l'état de `IFF1`/`IFF2`. |
| **Hardware** | `hw` | Affiche l'état du Gate Array (ROMs actives, config RAM) et du CRTC. |
| **Go / Resume** | `g` | Reprend l'exécution normale de l'émulateur. |
