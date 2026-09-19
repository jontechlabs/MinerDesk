# MinerDesk

**Vos mineurs, vos horaires, un seul tableau de bord.**

MinerDesk centralise les profils, le lancement, la surveillance et la planification de plusieurs moteurs de minage dans une application de bureau et une interface web. C'est un orchestrateur : le minage est effectué par des logiciels tiers téléchargés séparément.

[Télécharger la dernière version](https://github.com/jontechlabs/MinerDesk/releases/latest) · [Documentation complète en anglais](../README.md) · [Historique](../CHANGELOG.md)

![Tableau de bord MinerDesk](images/dashboard.png)

## Ce que cela vous apporte

- Conserver plusieurs profils avec leurs pools, portefeuilles publics, GPU et réglages, sans multiplier les scripts de lancement.
- Programmer des plages hebdomadaires, y compris à cheval sur minuit, avec veille ou hibernation facultative après le minage.
- Suivre hashrate, puissance, température, shares et durée depuis le bureau ou un navigateur de confiance.
- Arrêter manuellement une session planifiée sans que le planificateur la relance immédiatement.
- Utiliser SRBMiner-Multi, lolMiner, BzMiner, Rigel, lpminer, NPMiner ou un exécutable personnalisé.

Les réglages disponibles dépendent du moteur et du matériel. Les statistiques viennent de l'analyse de la sortie des mineurs ; elles ne constituent pas une garantie de performance ou de revenus. Le pourboire facultatif au développeur est désactivé par défaut (0 %).

## Installation

**Windows 10/11 x64 :** télécharger `MinerDesk_0.7.22_x64-setup.exe` depuis la Release. L'installeur configure le backend privilégié ; l'interface graphique reste exécutée comme utilisateur normal.

**Linux x64 compatible Debian :** télécharger le paquet `.deb`, puis :

```bash
sudo apt install ./MinerDesk_0.7.22_amd64.deb
```

Les binaires ne sont pas signés. Comparer leur SHA-256 au fichier `SHA256SUMS.txt` de la Release. Les notes de publication précisent les plateformes et tests réalisés.

Dans **Miners**, choisir un moteur, renseigner le pool et une adresse publique, sélectionner les GPU, sauvegarder et démarrer. **Console** permet de comprendre les erreurs. **Schedules** gère les plages horaires.

![Planificateur MinerDesk](images/schedules.png)

Captures réelles de l'interface web 0.7.21, conservée dans 0.7.22. Les valeurs illustrent une session et ne constituent pas un benchmark.

## Contrôle et sécurité

Un arrêt manuel suspend les occurrences de planification en cours. Un démarrage manuel réussi reprend le contrôle et n'est pas arrêté par la fin d'une plage. La prochaine occurrence reste disponible. Fermer un onglet web n'arrête pas le planificateur si le backend reste actif.

Sur Windows, quitter réellement le bureau arrête son backend et ses mineurs ; réduire dans la zone de notification ne quitte pas l'application. Le CLI autonome reste indépendant.

Ne jamais saisir de clé privée ou de phrase de récupération. Le mode local utilise `http://127.0.0.1:17888/`. L'accès LAN doit rester sur un réseau de confiance, avec un jeton fort ; privilégier un VPN et ne jamais exposer directement le service HTTP à Internet. Les profils, journaux et URL peuvent contenir des informations à masquer avant partage. Lire [SECURITY.md](../SECURITY.md).

## Compiler et contribuer

- [Compilation Windows](../SETUP-WINDOWS.md) : utiliser `scripts/build-windows.ps1`.
- [Compilation Linux](../BUILD-LINUX.md) : utiliser `bash scripts/build-linux.sh`.
- [Contribuer](../CONTRIBUTING.md) et [signaler un problème](https://github.com/jontechlabs/MinerDesk/issues).

MinerDesk est distribué sous [licence MIT](../LICENSE). Les moteurs tiers conservent leurs propres licences et frais.
