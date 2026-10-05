# Publication validation — 0.7.23

Compiled source revision: `261d169ac3544e7b95fc73030b6e7441d3515700`. [Windows/Linux build and test run](https://github.com/jontechlabs/MinerDesk/actions/runs/37282941849): **success**. The tested pull-request merge and the resulting main merge have the same Git tree as that revision. Subsequent edits update only publication documentation.

## Completed checks

| Check | Result |
| --- | --- |
| Frontend | 23 Node regressions and TypeScript/Vite production build pass. |
| Rust | 29 library tests pass on Windows and Linux, including six Quantus support regressions. |
| Quantus behavior | Algorithm/pool/address detection, preserved inline and separate workers, correct command payout arguments, saved user-wallet preservation, custom-engine exclusion, 0% default, secondary-payout exclusion and existing Pearl merge behavior are covered. |
| Windows build | GitHub Actions `windows-2022` produces desktop, headless CLI, windowless backend and NSIS installer. Installer/task/windowless policy preflight and standalone scheduler/configuration-worker tests pass. |
| Windows files | All four files report 0.7.23. PE headers confirm x64 application executables; desktop/backend use GUI subsystem 2 and headless CLI uses console subsystem 3. Downloaded hashes match CI build information. |
| Linux build | GitHub Actions `ubuntu-22.04` builds desktop, standalone CLI, Debian package and AppImage using the canonical release script. Downloaded file hashes match the CI checksum manifest. |
| Debian contents | Version 0.7.23, architecture amd64 and desktop/CLI/compatibility-backend executable entries are verified. |
| Linux CLI runtime | Downloaded CLI reports 0.7.23, serves its embedded frontend, health, engine list and status, then exits cleanly on SIGINT. |
| LAN authentication | Requests from the WSL non-loopback address with missing/incorrect token or forged locality header are rejected; valid header token and bearer token are accepted. |
| Linux desktop runtime | The executable extracted from the Debian package and the final AppImage each launch as a non-root user under Xvfb and expose their 0.7.23 backend. Runtime checks use Ubuntu 26.04 WSL with isolated temporary configuration/data directories. |
| Wallet/source separation | A targeted scan checks that generated recovery phrase, wallet password and wallet keystore are absent from source and distributed files. Only the public QTC receiving address is included. |
| Publication | Release files include sanitized build provenance, source archive and SHA-256 checksums. |

## Limits

Release executables are **unsigned**. The local Windows application-control policy blocked execution of the downloaded headless CLI; it was not disabled. Windows compilation and Rust tests passed in CI, but local Windows CLI runtime and a real NSIS installation were not exercised.

The Debian package was extracted and its application was launched; it was not installed into the user's WSL system. Linux runtime checks establish the documented environments, not compatibility with every distribution. Runtime libraries are still required.

No live GPU miner, accepted-share test, actual support payout, private-key transaction, scheduled-task registration, firewall/Defender change, physical sleep/wake or GPU tuning was performed. Support rewards still depend on the engine, pool rules, share difficulty and payout thresholds. The user's running MinerDesk instance and mining sessions were not modified.
