# Validation — MinerDesk 0.7.19

## Périmètre

Version préparée à partir de MinerDesk-Tauri-0.7.18-task-interval-fix.zip.
Comparaison réalisée avec MinerDesk-Tauri-0.7.5-console-headless-lifecycle(1).zip
fourni par l'utilisateur. Livrable : code source, pas un installateur précompilé.

## Vérifications réellement exécutées dans l'environnement de préparation

- `tests/verify_installer_sources.py` : 11 contrôles statiques passent. Guillemets
  NSIS, commande partagée, consommation de pile, 13 fonctions développées,
  références de sauts et variables, délimiteurs des 14 fichiers PowerShell,
  versions et garde-fous du build. Le contrôle PowerShell local est lexical,
  pas une exécution du véritable parseur Windows PowerShell.
- `tests/verify_task_restart_policy.py` : contrainte XML testée, PT15S rejeté,
  PT1M accepté, trois chemins de création de tâche à trois reprises/60 secondes,
  contrôles d'ajout au journal conservés. Aucune tâche Windows créée.
- `tests/verify_windowless_scheduler_sources.py --baseline <0.7.18>` : 25
  invariants du code passent. Présence des deux points d'entrée, bons chemins,
  API Stop/Restart, branche du scheduler, persistance et garde de fermeture.
- TypeScript `transpileModule` : syntaxe/transpilation des quatre fichiers src
  TypeScript/TSX et de vite.config.ts réussie. Ce n'est pas une vérification
  complète des types du projet avec ses dépendances React/Tauri installées.
- `node --check scripts/dev.mjs` : syntaxe valide.
- Contrôle lexical Rust sur les cinq fichiers .rs sous src : délimiteurs
  équilibrés, pas de jeton signalé en erreur par le lexer Pygments. Ce n'est pas
  un contrôle de types, d'emprunts ou une compilation Rust.
- Comparaison à la 0.7.18 : module Windows Job Object et implémentation
  ManagedChild identiques octet par octet; point d'entrée CLI inchangé.
- Vérification finale du ZIP, du manifeste des fichiers, et application du patch
  sur une copie de la 0.7.18 pour reconstruire exactement les fichiers 0.7.19.

## Tests ajoutés qui seront exécutés sur le PC Windows lors du build

Le script habituel reste :

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\build-windows.ps1
```

Avant de compiler le produit, il lance :

1. le vrai parseur PowerShell et les tests d'installation à processus synthétiques;
2. les tests des paramètres de reprise de tâche;
3. `rustc --test` sur le module de production `schedule_control.rs` puis le
   programme de test obtenu : 13 tests Rust de règles de planification;
4. les tests de présence/chemins du backend sans fenêtre et de sa sélection
   par la maintenance.

Après compilation, il lit l'en-tête PE des binaires réels : subsystem=2 requis
pour minerdesk-backend.exe, subsystem=3 requis pour minerdesk-headless.exe.
Ces vérifications n'ajoutent pas de dépendance Python au build de l'utilisateur.
Les prétests ne lancent aucun mineur et ne modifient aucune tâche réelle.

## Les 13 scénarios de la politique de planification

Arrêt manuel contre répétition des ticks; reprise manuelle; occurrence du jour
suivant même sans tick observé à la fin; restauration après redémarrage du backend;
Stop all incluant les profils éligibles sans processus; indépendance des profils;
chevauchement de plusieurs créneaux déjà actifs; nouveau créneau qui chevauche un
créneau suspendu; créneaux consécutifs; nuit dimanche/lundi; début inclusif et fin
exclusive; créneaux invalides/désactivés/vides; arrêt hors planification.

## Ce qui n'a PAS été exécuté ici

Rust/Cargo, PowerShell et NSIS Windows ne sont pas disponibles dans cet environnement.
Les 13 tests Rust ont été écrits mais n'y ont pas été exécutés. La compilation
Windows, le lancement réel de la tâche, l'installation/désinstallation, la
vérification visuelle de l'absence de terminal, le minage GPU et les tests de crash
restent à valider sur Windows. Les vérifications de source ci-dessus ne sont pas
présentées comme un substitut à ces tests.

## Vérification fonctionnelle conseillée après installation

Installer le nouveau Setup 0.7.19, pas un ancien exe. Contrôler que l'action de la
tâche utilise le backend sans fenêtre. Laisser un créneau démarrer un profil,
cliquer Stop, attendre au moins dix secondes : état STOP, uptime 00:00:00 et pause
manuelle doivent rester affichés. Tester Start, Stop all et un créneau suivant.
Tester séparément la persistance en redémarrant le backend durant la pause.
