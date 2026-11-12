# MinerDesk Fleet

## English

Fleet lets paired MinerDesk instances see and control their Windows and Linux rigs through one coordinator. Each rig keeps its own mining engines, profiles, scheduler and operating-system permissions. The coordinator organizes remote access; it does not run another rig's miner.

Introduced in **0.8.0**, Fleet is **off by default**. It activates only when you choose to create or join a group.

### Set up a coordinator

1. Open **Fleet** on the computer that will coordinate the group. Choose a recognizable name, its reachable HTTPS address and its listening address. Use a stable LAN address or a VPN address that the other rigs can reach.
2. Create an invitation on that computer. The invitation contains the coordinator's address, its public TLS certificate and a short-lived, single-use enrollment secret. Treat the entire invitation key as private.
3. On another rig, open Fleet and join using the address and invitation key obtained directly from the coordinator's owner. Check the displayed certificate fingerprint through that trusted channel if the address or invitation came from someone else.
4. Repeat with a new invitation for each rig. An enrolled MinerDesk instance can operate the group; only enroll computers and accounts you trust with that access.

Since **0.8.1**, joining asks only for the coordinator address and invitation key. The rig label comes from the local computer's name. Existing names and pairings are retained during updates; older clients may still provide an explicit name.

**LAN discovery is optional.** Its Find action broadcasts a small discovery request and returns public coordinator hints. Finding a machine does not authorize it, verify its identity or pair it. Discovery does not include enrollment secrets or credentials. Separate networks normally require entering the coordinator address manually.

Agents contact the coordinator using outgoing HTTPS requests and a certificate pinned by the invitation. They have credentials separate from controller credentials. The default Fleet listener is **TCP 17889** and optional discovery is **UDP 17990**. No rig needs to expose its existing MinerDesk HTTP port to the Internet. For access across networks, use a trusted VPN, or deliberately configure a reachable coordinator HTTPS address, router port forwarding and host firewall rules. MinerDesk does not automatically configure UPnP mappings. The existing LAN API on port 17888 is a different interface: do not expose that plain HTTP interface publicly.

### What remote control changes

Fleet supports status and filtered recent logs, schedule-aware Start/Restart/Resume, Stop and an explicit Force action. Start respects the rig's configured windows; **Force mining** is the separate unlimited override. A command acknowledgement describes its actual result, including waiting for a schedule. A rig shown offline may still be mining; the dashboard shows when its state was last received.

**Open** displays the selected rig's dashboard summary and miner table together: hashrate, engine-reported watts, temperature, fan percentage when available, accepted/rejected shares and uptime. These use the same runtime metrics as that rig's local dashboard and refresh with the Fleet view every five seconds. Missing measurements remain unknown; disconnected/stale rigs do not contribute to current totals.

Profile edits apply to an existing profile. They can change its name, enable state, algorithm, pool, public receiving address, worker, GPU selection and supported tuning fields. The rig must stop that profile before a remote edit is saved. Changes do not start it automatically. This version deliberately keeps these fields local:

- engine selection, executable paths and Advanced arguments;
- pool passwords, miner API ports and MinerDesk support preferences;
- root/admin enrollment and private authorization references;
- local web tokens, interface preferences and application integration settings.

The operator configuration view contains public payout identifiers because operators need them to review profile edits. Periodic telemetry omits wallet identifiers, pool passwords, executable paths, tokens and private root references. Raw command lines and credential-bearing or local-path-bearing output are withheld from remote logs. Inspect the local console when a diagnostic is withheld. Third-party output is untrusted and this filtering is not a complete audit of arbitrary miner messages.

Remote saves include a configuration revision. A conflicting local edit causes the save to fail with a refresh message instead of overwriting the other change. Profile revisions cover that profile's remotely visible settings; schedule revisions cover the profiles and schedules. Local-only settings are preserved when the rig commits an edit.

### GPU settings and schedules

GPU identifiers belong to the mining engine. An operating-system inventory index may identify a different card. Fleet does not start a miner merely to obtain its device list, and it does not invent a GPU inventory when no verified inventory is cached. Confirm selected IDs in the engine's own console, particularly after changing hardware.

Tuning validation checks field types and broad parser limits. It does not establish that a frequency or power value is appropriate for a particular card, driver or engine. Core/memory locks, offsets, watts and fan percentage remain separate units. A blank field omits the corresponding MinerDesk override; it does not reset previous driver settings or cancel separate Advanced arguments. Engine updates and MinerDesk updates remain separate operations.

Schedule times remain in **the rig's local timezone**. The dashboard includes that rig's current local time and UTC offset. Fleet can edit mining windows and assignments, but it cannot change the system timezone. Existing sleep, hibernation, wake and countdown-confirmation settings remain local; a new remote window cannot add them. Review any already configured local power action before changing its mining window. Ordinary Stop retains the existing rule: it pauses the current dated occurrence rather than permanently disabling all future windows.

### Linux administrator mode

Fleet never requests, stores or transfers an administrator password. The existing [Linux authorization workflow](LINUX-ELEVATION.md) remains local to the rig. A remote start can use an already approved command, but it cannot enroll or repair that approval. No interactive password dialog is opened by a Fleet command.

Changing a root-enabled profile can make the saved command authorization invalid. Reopen the profile on the Linux computer, review the changed command and approve it there before another unattended start. A saved authorization reference is shown as **saved, not reverified** until the actual launch validates it. Its presence is not proof that a changed command remains approved. Engine files and supporting files are still checked by the existing constrained runner.

### Losing the coordinator or restarting a machine

A network interruption does not cancel the rig's local mining session or its scheduler. Cached status becomes stale and remote commands have expiration times. Expired commands must not become a surprise Start on reconnection; command identities and receipts prevent ordinary retries from starting the same action again. A failed command can report a partial or ambiguous outcome; inspect the current rig state before submitting a new action.

Local autonomy still requires **the rig's MinerDesk backend to remain running**. While Fleet is enabled, closing the Windows desktop window detaches its interface and leaves the Fleet backend active; on Linux, closing the Fleet window hides it while its application remains running. The explicit Quit action, ending the operating-system session or shutting down the computer can still stop that backend and its miners. Fleet does not automatically install a boot service. Use an explicitly supervised headless instance if unattended operation after logout or restart is required. Do not run two backends against one configuration file at the same time.

This first version has a single coordinator. Its loss interrupts group administration, not an already running rig's local planner. It does not implement automatic coordinator election, high availability, arbitrary remote shells, system installation, remote root approval or full per-user/per-rig role management.

Once Fleet has activated background operation in a running backend, leaving or disabling the fleet does not stop that backend's local mining. Use **Quit** to end that session. A fresh standalone launch returns to the normal standalone desktop lifecycle.

Leaving also revokes that rig's coordinator credentials when the coordinator is reachable. If revocation cannot be confirmed, the local pairing is still removed and Fleet tells you to remove that rig on the coordinator. The coordinator's **Remove this rig** action invalidates both its agent and controller credentials without sending a mining-stop command.

Fleet identities, TLS material, membership credentials and receipts live in **`fleet-private/state.json` beside the local MinerDesk configuration**. They are separate from ordinary profile exports. The directory is private to the local account (0700/0600 on Linux; account/SYSTEM ACL protection on Windows). Back it up privately together with the rig configuration if you need to recover the same identity. A malformed state file fails closed instead of silently generating replacement credentials. Losing the coordinator's private state may require pairing again. Do not publish this directory in GitHub issues, screenshots or source archives.

## Français

Fleet permet de surveiller et piloter des rigs Windows et Linux depuis les instances MinerDesk associées à un même coordinateur. Chaque rig conserve ses mineurs, sa configuration, ses horaires et ses autorisations système.

Ajouté dans la version **0.8.0**, Fleet est **désactivé par défaut**. Il s'active lorsque vous choisissez de créer ou de rejoindre un groupe.

Sur le coordinateur, créez une invitation puis transmettez directement son adresse et sa clé au propriétaire du rig à associer. L'invitation est temporaire et à usage unique ; utilisez une nouvelle invitation pour chaque rig. Elle transporte le certificat public nécessaire à la connexion HTTPS, ainsi qu'un secret d'association : gardez la clé complète privée. Une instance associée peut piloter le groupe ; n'associez que des ordinateurs et comptes de confiance. La découverte LAN facilite la recherche d'un coordinateur mais n'accorde aucun accès.

Depuis **0.8.1**, rejoindre la flotte demande seulement l'adresse et la clé ; le nom de la machine est automatique. Les noms et associations existants sont conservés. **Ouvrir** présente le résumé et le tableau des mineurs avec les mêmes mesures que le dashboard local : hashrate, watts, température, ventilation disponible, shares et durée. La vue s'actualise toutes les cinq secondes ; les mesures absentes restent inconnues et les rigs déconnectés sont exclus des totaux actuels.

Les rigs établissent eux-mêmes leurs connexions HTTPS vers le coordinateur, sur **TCP 17889** par défaut. La découverte LAN facultative utilise **UDP 17990**. Pour plusieurs réseaux, préférez un VPN de confiance ou configurez volontairement une adresse HTTPS joignable, la redirection du port du coordinateur et son pare-feu. MinerDesk n'ajoute pas automatiquement de règle UPnP. N'exposez pas directement l'ancienne interface HTTP de MinerDesk sur Internet.

Les boutons Start, Restart et Resume respectent les horaires du rig. **Force mining** est l'action explicite pour miner sans limite horaire. Un rig « hors ligne » peut continuer à miner : consultez l'heure de la dernière remontée. Le minage et les horaires continuent pendant une panne réseau tant que le backend local reste ouvert. Avec Fleet actif, fermer la fenêtre Windows détache l'interface et conserve le backend ; sous Linux, la fenêtre est masquée et l'application reste ouverte. L'action Quitter, la fin de session et l'arrêt du PC peuvent arrêter le backend et ses mineurs. Fleet n'installe pas automatiquement un service au démarrage.

Les modifications distantes demandent l'arrêt du profil et vérifient sa révision avant l'enregistrement. Une modification locale concurrente provoque un conflit visible. Les chemins d'exécutables, arguments avancés, mots de passe de pool, jetons locaux et autorisations root ne sont pas remplacés. Les adresses de réception publiques sont visibles dans la configuration opérateur, mais ne figurent pas dans la télémétrie périodique. Les commandes complètes et messages contenant des identifiants sensibles sont filtrés dans les journaux distants.

Les horaires utilisent le fuseau local du rig. Les réglages de veille, réveil et confirmation restent locaux. Les indices GPU doivent être confirmés dans le moteur de minage ; un indice système ne suffit pas. Un champ de fréquence vide signifie l'absence d'argument MinerDesk, sans réinitialiser le pilote.

Sous Linux, aucun mot de passe administrateur n'est enregistré ou envoyé. Une commande root déjà approuvée peut être exécutée à distance. Si les réglages ont changé, l'autorisation doit être revue sur le rig Linux. Fleet ne déclenche pas de demande interactive de mot de passe et ne crée pas d'autorisation root à distance.

Sauvegardez **`fleet-private/state.json`**, situé à côté de la configuration MinerDesk, dans un emplacement privé. Ce fichier contient les identités et secrets Fleet ; il ne doit jamais être publié. La première version ne fournit ni remplacement automatique du coordinateur, ni terminal distant, ni gestion détaillée de rôles par utilisateur.
