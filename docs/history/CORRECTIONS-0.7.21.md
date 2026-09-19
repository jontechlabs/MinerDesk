# MinerDesk 0.7.21 — Start manuel et erreurs visibles

## Diagnostic issu des sources 0.7.20

L'ancien `src/App.tsx` appelle systématiquement `saveConfig(config)` avant
`POST /api/miners/:id/start` ou `POST /api/miners/start-all`, même sans modification.
Son transport HTTP abandonne l'attente après 5 000 ms. `saveConfig` intercepte
l'échec et renvoie `false`; le traitement de Start retourne alors sans envoyer
la commande. Le message n'apparaît que dans le petit texte d'en-tête.

Côté Rust, `CoreState::replace_config` exécute directement la configuration du
pare-feu et `sync_windows_wake_tasks`. Cette dernière lance une commande PowerShell
pour supprimer les anciennes tâches, puis une autre par jour et créneau de réveil.
Ces opérations font partie de l'attente HTTP. Leur durée sur le PC de l'utilisateur
n'est pas connue ici, faute de journal de ce clic.

Une autre faiblesse est confirmée : `/api/miners/start-all` peut renvoyer HTTP 200
avec une erreur dans `result` pour un ou plusieurs profils. L'interface 0.7.20
ignore ce tableau. Un échec de chemin, d'arguments ou de conflit GPU peut donc
ne pas apparaître après Start all.

## Reproduction exécutée

Le test `tests/miner-controls.test.cjs` exécute les fonctions exactes `api`,
`saveConfig` et `minerAction` extraites de la 0.7.20, avec leurs dépendances UI
simulées et un vrai serveur HTTP local éphémère. Celui-ci répond au PUT après
5,5 secondes. Le délai original de 5 secondes reste inchangé.

Résultat : l'ancien code envoie uniquement `PUT /api/config`, aucun POST Start,
puis remet busy à false. Ce résultat reproduit le symptôme décrit, mais ne prouve
pas qu'il n'existe aucune autre erreur de profil sur la machine de l'utilisateur.

Avec le nouveau module de commande, un profil sans changement envoie directement
Start. Un profil modifié est enregistré avant démarrage; le test avec sauvegarde
simulée de 5,5 secondes envoie bien PUT puis POST.

## Changements livrés

- Start / Restart / Start all n'enregistrent la configuration que si elle a changé.
- Stop / Stop all ne dépendent jamais de la sauvegarde d'un formulaire.
- Le backend enregistre atomiquement la configuration, puis place la synchronisation
  Windows dans une file de travail unique. L'API n'attend plus PowerShell.
- La file conserve le dernier état demandé lorsqu'il y a plusieurs sauvegardes.
  Le worker ne recrée pas les tâches de réveil après une modification de fréquence,
  de pool ou de portefeuille si les créneaux sont inchangés; il ne reconfigure
  pas le pare-feu si l'exposition LAN et le port sont inchangés.
- Le worker appartient au backend HTTP. Le Desktop ne concurrence plus cette
  initialisation avec son propre chargement de configuration.
- Les handlers de sauvegarde et de commandes utilisent `spawn_blocking` pour leur
  travail synchrone. La préférence de démarrage Windows est également appliquée
  hors de la chaîne bloquante du clic Start.
- Le délai couvre toute la réponse HTTP : 15 s pour la sauvegarde, 30 s pour une
  commande, 5 s par défaut pour la consultation. Une écriture expirée n'est jamais
  relancée automatiquement : son résultat côté serveur peut être indéterminé.
- Les erreurs ont un bandeau persistant FR/EN et ne sont pas effacées par les polls.
  Start all examine chaque résultat. Les erreurs du backend sont inscrites dans
  `backend.log`, dans la console du profil et dans `runtime.last_error` affiché
  dans le tableau. Les boutons de profil sont désactivés pendant une commande,
  avec protection supplémentaire contre les doubles clics.
- Un Start manuel d'une session déjà planifiée en prend explicitement la propriété
  manuelle (notamment pour Start all). Le scheduler ne l'arrête pas en fin de plage.

Le code des Job Objects, les exécutables sans console/CLI, le module des pauses
manuelles du scheduler et les fonctions de focus de confirmation restent conservés.
La politique Windows PT1M est inchangée. La migration reconnaît la 0.7.20.

## Construire et installer

Depuis un NOUVEAU dossier extrait, à la racine contenant package.json :

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Attendre `BUILD SUCCEEDED: MinerDesk 0.7.21`, puis lancer :

```powershell
.\publish\windows-x64\MinerDesk_0.7.21_x64-setup.exe
```

Les trois exécutables doivent être reconstruits et installés ensemble. Le ZIP
contient du code source et les placeholders de ressources; aucun EXE Windows
nouvellement compilé n'a été produit dans l'environnement de préparation.

Après installation, vérifier la version Desktop et backend. Tester Start hors
créneau, Stop, une modification de Core clock suivie de Start, puis Start all.
Un profil désactivé ou un conflit GPU doit rester refusé, avec une raison visible.

Journal runtime :

```powershell
Get-Content -LiteralPath "$env:APPDATA\MinerDesk\backend.log" -Tail 100
```

La nouvelle trace `miner command start requested for profile ...` permet de
vérifier que l'API a réellement reçu le clic; elle est suivie d'une trace de résultat.
Les messages de synchronisation Windows asynchrone sont aussi dans ce fichier.

## Validation réalisée et limites

Exécutés dans l'environnement de préparation :

- 17 tests Node automatisés : fonctions 0.7.20 réelles sous dépendances UI simulées,
  nouveaux modules TypeScript de production, HTTP local, délais, échec de sauvegarde,
  priorités Stop, erreurs Start all, annulation et absence de retry automatique.
  Deux de ces tests contrôlent l'assemblage des sources, pas un runtime Rust/Windows.
- Vérification stricte des types de `src/api.ts` et `src/minerCommands.ts` avec tsc.
- Syntaxe/transpilation des 6 fichiers TypeScript/TSX de src, sans prétendre à un
  build React complet (dépendances React/Tauri/Vite absentes ici).
- 11 contrôles statiques de l'installateur; contrainte XML de reprise PT1M;
  26 invariants backend/scheduler, y compris comparaison inchangée des Job Objects.
- Vérification des versions et intégrité ZIP; application du patch et comparaison
  de ses fichiers avec l'archive complète.

Non exécutés ici : compilation Rust, véritables tests Rust, installation NSIS,
minage GPU et comportement de fenêtre sous Windows. Les exécutables rustc/cargo et
les outils Windows ne sont pas disponibles dans cet environnement.

Le build Windows exécute désormais les 17 tests Node, les 15 tests de politique
scheduler existants et 2 nouveaux tests Rust du worker de synchronisation
(`config_sync.rs`), en plus des précontrôles PowerShell existants. Les deux tests
Rust bloquent volontairement le worker avec un canal pour vérifier que les
sauvegardes suivantes ne l'attendent pas et que le dernier état est appliqué.
Ils ne touchent à aucun mineur, au registre ni à une tâche Windows réelle.

Référence technique utilisée pour le déplacement du travail bloquant :
Tokio `spawn_blocking`, https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html.
Elle justifie l'usage de l'API Tokio; le diagnostic du bug provient des sources et
la reproduction ci-dessus, pas de cette documentation.
