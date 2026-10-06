# MinerDesk updates

Starting with **0.7.24**, MinerDesk checks for stable GitHub releases shortly after startup and every **6 hours while the interface is open**. In **Settings → Application updates**, choose **Check now** or turn automatic checks off. This preference is local to each desktop/browser. Offline or rate-limited automatic checks stay quiet and retry later.

An available release shows its version, release notes, **Update…**, **GitHub downloads**, and **Later (24 hours)**. A different new version is not hidden by a previous version's snooze. No download or installation starts until you confirm **Download, install & restart**. Save pending configuration edits first.

| Distribution | Update behavior |
| --- | --- |
| Installed Windows x64 NSIS application | Signed installer download, verification, backend/miner shutdown, installer launch, application restart. Windows UAC may require administrator approval. |
| Linux x64 AppImage in a writable directory | Signed AppImage download, verification, miner shutdown, replacement and restart. |
| Installed Linux x64 Debian package with `/usr/bin/pkexec` | Signed Debian download, verification, miner shutdown, the operating system's administrator prompt, `dpkg -i`, restart. No password is collected by MinerDesk. |
| Debian without graphical elevation, read-only AppImage, portable executable, development build, unsupported architecture/OS | Notification and link to the GitHub release page. |
| Headless/browser/LAN dashboard | Notification and GitHub link only. Install on the host manually; remote clients cannot request application installation. |

Only the Windows installer, Linux `.deb` and Linux AppImage are distributed for x64. There are no macOS/ARM packages in this release. An update with no valid manifest/artifact for the current distribution falls back to the release page. Debian package manager errors or cancelled administrator approval also show that link.

Downloads use HTTPS and are verified by Tauri's updater against the public key embedded in MinerDesk. The private signing key is separate from Windows Authenticode and the pending SignPath application. A valid update signature does **not** remove Windows SmartScreen warnings. The updater rejects an unexpected repository, platform, version or artifact URL and never bypasses signature verification.

Miners continue running during download and signature verification. Only after successful verification does MinerDesk stop the runtime for installation. Profiles, public wallet addresses, schedules and downloaded mining engines are preserved. A manual mining session must be started again; saved schedules may resume according to their configured times. If installation fails after shutdown, restart mining manually if needed. Windows also refuses to proceed while another desktop instance or an independent headless backend prevents safe shutdown.

Older MinerDesk versions do not contain this feature: install 0.7.24 once from GitHub to receive future update prompts. Check interval and snooze apply while the application/web interface is running; there is no new scheduled background updater.

## Publishing future updates

1. Change package, Tauri, Cargo, frontend and health/CLI version metadata together. Add `docs/releases/<version>.md`.
2. Review and merge the source to `main`. Wait for **Build and checks** to succeed for that exact revision.
3. Run **Prepare release with signed updates** on `main`, supplying that successful CI run ID. The protected `app-updates` environment contains `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and permits only `main`.
4. The workflow verifies build provenance, packages both operating systems, signs and independently verifies the installer/AppImage/Debian package, and creates a complete **draft** containing `latest.json`, `.sig` files, binaries, source and SHA-256 checksums. Review and publish it. Never replace a published version's assets.

The packaging host needs Node/npm dependencies, Python 3.11+ and `minisign`. The public key is in `src-tauri/tauri.conf.json`; private keys/passwords must never enter the repository, source ZIP, logs or release. Keep an offline backup: replacing/losing the key prevents existing installations from trusting future releases. There is no automatic unsigned fallback.

Once Authenticode is approved, sign the final Windows executable **before** generating its update signature. Modifying bytes afterward invalidates the update signature. The separate Signed Windows release workflow remains disabled pending SignPath; its draft must be combined with Linux assets and given an update manifest/signatures before final publication.
