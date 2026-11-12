# Update MinerDesk through Fleet

Starting with **0.8.2**, a trusted Fleet controller can request signed MinerDesk upgrades on enrolled rigs. Updates are optional and **off by default on every computer**. No administrator password is collected, stored or sent through Fleet.

## Activate once on each rig

1. Install 0.8.2 or newer using the rig's existing **Settings → Application updates** screen. Older agents cannot install a feature they do not yet contain, so this initial upgrade remains local.
2. On that rig, open **Fleet → Connection & fleet → Remote updates · this machine** and enable **Allow remote updates on this machine**.
3. On Debian/Ubuntu/Lubuntu, choose **Authorize once on Linux** and approve the operating system's local administrator dialog. This authorizes only the installed MinerDesk update helper. Writable AppImages do not need root enrollment. Windows needs its installed, elevated privileged backend.
4. Repeat the one-time activation on the other rigs. Existing Fleet invitations and identities are preserved.

All paired Fleet instances are trusted controllers. Enabling this option permits those controllers to request future signed MinerDesk upgrades; it does not grant a remote shell, arbitrary package installation, a URL override or access to an administrator password.

## Roll out an update from any paired PC

1. Open Fleet and choose **Check updates** (or select rigs and **Check selected updates**).
2. Select the rigs and choose **Update selected…**, or open one rig's **Updates** tab and choose **Update this rig…**.
3. Review the exact offered version, included/excluded rigs and mining interruption. Confirm the rollout explicitly.

The coordinator persists the plan and updates one rig at a time, with the coordinator itself last. Download and verification happen while mining continues. Only after the worker has independently verified and privately staged the package does the rig stop mining and exit for installation.

An accepted command means the update request was queued. Fleet confirms installation only after receiving a fresh report of the expected or a newer version. The next rig waits for that confirmation. A failure stops the remaining rollout; **Cancel remaining** leaves an already committed installation to finish. After 45 minutes without version confirmation, the plan reports an uncertain failure rather than silently retrying an installer.

Keep the coordinator backend running until its own final update. A member UI can close or lose its connection without losing the coordinator's saved plan. Local mining schedules retain their normal policy after restart; a manually started session may need starting again.

## Supported installations

| Rig installation | Requirement |
| --- | --- |
| Windows x64 NSIS installation | Registered installation and the elevated MinerDesk backend/task; local Fleet update opt-in. |
| Debian/Ubuntu/Lubuntu x64 package | Registered `miner-desk` package, local opt-in, and one-time local helper authorization. |
| Writable Linux x64 AppImage | Original writable AppImage and local opt-in; no root password or sudo grant. |
| Older agents, portable/development copies, read-only images, unsupported platforms | Excluded with a reason; use the local updater or GitHub download page. |

## Disable or revoke

Use the same local Fleet connection panel to disable future remote updates. On Debian systems, **Revoke Linux authorization** also removes that account's constrained system authorization. Disabling or revoking does not pretend to cancel a package installation that has already committed. It does not disable ordinary locally confirmed desktop updates.

The Debian grant is UID-bound. Its fixed helper is `/usr/lib/minerdesk/minerdesk-update-helper`; it accepts only the fixed apply mode and a bounded signed-update request. The root-owned grant and narrowly scoped sudo policy survive signed package upgrades and are removed on package removal/purge. Root installers never trust an agent-supplied command, process ID, identity, publisher key or arbitrary download URL.

Update requests freeze a stable version. Both the agent and detached worker verify the artifact against the embedded publisher key, including the authenticated version in the signature. The worker repeats the installation identity and package checks at its privilege boundary. Protected private staging prevents another process from swapping verified bytes before installation. Public progress contains bounded status data; detailed worker diagnostics remain private.

## Français

À partir de **0.8.2**, Fleet permet de déployer les futures mises à jour signées depuis une instance associée. Il faut d'abord installer cette fonctionnalité sur chaque rig avec l'outil habituel, puis l'activer localement dans **Fleet → Connexion & flotte → Mises à jour distantes · cette machine**. Sous Debian/Lubuntu, validez une fois **Autoriser une fois sur Linux**. Aucun mot de passe administrateur n'est conservé.

Depuis n'importe quel PC associé, vérifiez les mises à jour, sélectionnez les rigs puis confirmez la version et l'interruption du minage. Le déploiement avance rig par rig, coordinateur en dernier, et attend la reconnexion avec la version confirmée. Un échec arrête le restant. **Annuler le restant** ne force pas l'arrêt d'une installation déjà engagée.

Le téléchargement et la vérification précèdent l'arrêt du minage. Les profils, horaires, moteurs téléchargés et associations Fleet sont conservés ; les horaires reprennent selon leurs règles habituelles. Une session manuelle peut nécessiter un redémarrage. L'option reste désactivable localement, et l'autorisation Linux est révocable séparément.
