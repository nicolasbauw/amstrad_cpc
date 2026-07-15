Prochaines étapes

Pour rendre notre émulateur d'Amstrad CPC 6128 encore plus complet et fidèle au modèle d'origine, nous allons :
- Permettre le chargement des ROMs officielles du CPC 6128 (la ROM système OS en ROM basse, la ROM BASIC en ROM haute 0, et la ROM AMSDOS en ROM haute 7). Les ROMs sont déjà présentes dans le répertoire bin.
- Nettoyer et organiser notre code pour qu'il soit impeccable. J'aimerais ne garder dans le main() que ce qui est relatif à SDL, et créer un machine.rs qui implémente un objet de type Machine qui détient toutes les structs qui constituent le cpc. Dans la boucle SDL ne resterait qu'une déclaration du style cpc = Machine::new(), ainsi que des appels du genre cpc.update(). Si tu as une meilleure idée pour modulariser le code et nettoyer le main(), je suis preneur ;-)
