# MinerDesk 0.7.19 — note de correction et comparaison des sources

## 1. Une fenêtre terminal n'est pas obligatoire

La nouvelle version emploie `minerdesk-backend.exe` pour le backend du Desktop.
Son point d'entrée porte `windows_subsystem = "windows"` et appelle directement
le même moteur Rust. Il n'est pas un wrapper laissant un enfant orphelin : c'est
lui qui détient les Job Objects existants des mineurs.

`minerdesk-headless.exe` conserve son point d'entrée console d'origine. Un
lancement CLI sans `--desktop-owned` reste indépendant du Desktop. L'application
Desktop, la tâche privilégiée, son lancement UAC de secours et Repair backend
cherchent et lancent désormais le nouveau backend sans fenêtre.

Le code conserve le heartbeat/PID Desktop et les Job Objects. Le chemin sans
fenêtre ne dépend pas de l'installation d'un gestionnaire Ctrl+C de console.
Les commandes auxiliaires nvidia-smi, powercfg, PowerShell et la découverte GPU
sont lancées avec CREATE_NO_WINDOW sous Windows. Les logs de mineurs restent dans
la page Console; le journal backend reste dans AppData\Roaming\MinerDesk\backend.log.
La présence de processus MinerDesk dans le Gestionnaire des tâches reste normale.

## 2. Pourquoi la 0.7.5 pouvait sembler différente

Comparaison effective de l'archive 0.7.5 jointe et de la base 0.7.18 :

- Le fichier `src-tauri/src/bin/minerdesk-headless.rs` des deux versions est
  identique : un main appelle run_headless(), sans attribut windows_subsystem.
  Il s'agit donc dans les deux cas du point d'entrée console par défaut.
- Les deux versions comportent un lancement UAC de secours avec
  `Start-Process ... -WindowStyle Hidden` dans src-tauri/src/lib.rs.
- La tâche privilégiée de la 0.7.5 lance directement minerdesk-headless.exe,
  comme celle de la 0.7.18, sans transformation du binaire en programme sans console.
- Le hook NSIS 0.7.5 appelle nsExec::ExecToLog pour enregistrer la tâche, puis écrit
  BackendTaskAdded=1 sans vérifier le code de retour de cette commande.
  Cette même commande contient encore RestartInterval de 15 secondes.
- La 0.7.18 vérifie la création effective de la tâche et utilise un intervalle
  valide d'une minute. L'erreur PT15S a déjà été constatée dans le journal fourni
  lors du diagnostic de la 0.7.17.

Conclusion limitée aux preuves disponibles : l'absence de blocage visible de la
0.7.5 ne certifiait pas la réussite de toutes ses opérations d'installation.
Un lancement via son secours caché est cohérent avec l'absence de terminal
rapportée, mais les sources seules ne permettent pas de prouver quel chemin a
été exécuté à l'époque sur le PC. La nouvelle version ne dépend plus de ce choix
de chemin pour éviter une console du backend Desktop.

Sources de la comparaison (numéros de lignes dans les archives extraites) :
- 0.7.5, `src-tauri/windows/hooks.nsh`, ligne(s) 28.
- 0.7.5, `src-tauri/src/lib.rs`, ligne(s) 2549, 2579.
- 0.7.18, `src-tauri/windows/maintenance.ps1`, ligne(s) 84, 85, 89, 93.
- 0.7.18, `src-tauri/src/lib.rs`, ligne(s) 1902.
- 0.7.5, `src-tauri/src/lib.rs`, ligne(s) 1504.

## 3. Cause du redémarrage après Stop

Dans la 0.7.18 comme dans la 0.7.5, start_scheduler parcourt les profils appartenant
à un créneau actif. Dès qu'un profil n'est pas running, il appelle start_miner
avec l'origine "schedule", puis attend une seconde avant de recommencer.
Aucune mémoire d'un Stop demandé par l'utilisateur n'intervient dans ce choix.
L'arrêt manuel pouvait donc être annulé au tick suivant. Cette logique est propre
au planificateur de minage, pas au RestartInterval de la tâche Windows.

## 4. Comportement implémenté en 0.7.19

Stop pose une suspension sur les occurrences de créneaux actives à cet instant
pour le profil. Stop all fait de même pour tous les profils actuellement concernés
par un créneau, même ceux encore arrêtés ou en échec. Le planificateur consulte
cette suspension avant tout démarrage. Start/Restart la lève après démarrage
réussi; la fin normale du créneau continue de s'appliquer à cette reprise.

L'identité d'une occurrence comprend l'ID du créneau et sa date locale de début.
La suspension reste donc valable après minuit pour un créneau nocturne, sans
bloquer l'occurrence du lendemain. Elle est conservée dans
`scheduler-manual-stops.json` à côté du fichier de configuration, puis relue au
démarrage. Une sauvegarde échouée est signalée : l'arrêt est quand même exécuté
et reste pris en compte en mémoire pendant cette exécution.

Les créneaux déjà actifs lors de Stop sont tous suspendus pour ce profil. Tant
qu'au moins l'un reste actif, un nouveau créneau qui le chevauche ne force pas la
reprise. Lorsque toutes les occurrences suspendues sont terminées, une autre
occurrence peut démarrer normalement. Les autres profils restent indépendants.

Les actions manuelles, les ticks de planification et les redémarrages internes
partagent un verrou de cycle de vie. La fermeture interdit les nouveaux démarrages.
L'interface affiche « Planification suspendue manuellement ». Stop ne dépend plus
de la sauvegarde préalable d'un formulaire; Start/Restart sauvegarde toujours les
modifications avant lancement. L'uptime arrêté reste à zéro.

Exemple : créneau 08:30–16:00, Stop à 11:00 => profil arrêté pour le reste de cette
occurrence, sauf Start/Restart manuel. Le créneau du lendemain reste actif.
Les tests de la politique comprennent aussi la nuit et les chevauchements.

## 5. Installation et livrables

La correction part de la 0.7.18, sans retour à l'ancien installateur moins contrôlé.
La politique Windows PT1M, les journaux persistants et la détection par chemins
sont conservés. Les ressources et le nettoyage incluent les deux binaires. Le
nouveau Setup sait traiter l'installation 0.7.18; données AppData et mineurs
préservés par le code de migration.

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Après BUILD SUCCEEDED: MinerDesk 0.7.19 :

```powershell
.\publish\windows-x64\MinerDesk_0.7.19_x64-setup.exe
(Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend').Actions |
    Format-List Execute, Arguments
```

Action attendue : `C:\Program Files\MinerDesk\minerdesk-backend.exe` et
`--desktop-owned`, ou le chemin d'installation choisi par l'utilisateur.

## 6. Validation et limites

Exécutés ici : contrôles statiques NSIS/PowerShell, contrainte XML de reprise,
25 invariants du nouveau code, transpilation/syntaxe TypeScript, syntaxe Node,
vérification lexicale Rust, comparaison inchangée des Job Objects et du main CLI,
intégrité de l'archive et application du patch de reproduction.

Non exécutés ici : compilation Rust/Windows, véritables tests PowerShell/Rust,
installation Windows et minage. Le build Windows exécutera les tests préalables,
dont 13 tests Rust sur le module de production et le contrôle du subsystem PE des
binaires compilés. Ils ne remplacent pas le test final sur Windows.
Voir VALIDATION-0.7.19.md dans l'archive pour le détail.

## 7. Documentation de plateforme consultée

Ces références expliquent le comportement de Windows/Rust; elles ne prouvent pas
le chemin de lancement historiquement utilisé sur le PC de l'utilisateur.

- Rust, sous-système console par défaut et sous-système Windows détaché :
  https://doc.rust-lang.org/reference/runtime.html#the-windows_subsystem-attribute
- Microsoft, minimum d'une minute pour RestartInterval :
  https://learn.microsoft.com/en-us/windows/win32/taskschd/tasksettings-restartinterval
- Microsoft, Hidden masque la tâche dans l'interface du planificateur :
  https://learn.microsoft.com/en-us/windows/win32/taskschd/tasksettings-hidden
