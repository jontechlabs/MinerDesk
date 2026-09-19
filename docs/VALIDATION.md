# Publication validation — 0.7.22

Source baseline: the author's 0.7.21 manual-start-command-fix archive. The 0.7.22 publication adds the LAN locality-header fix, cross-platform packaging, dependency lockfiles and documentation. Original Windows 0.7.21 executables are not redistributed as corrected builds.

## Local checks completed

- Frontend TypeScript/Vite production compilation.
- 17 existing Node command/transport regressions and 2 platform-config tests.
- 23 Rust library tests on Debian 12, including scheduler, configuration worker, tuning/uptime and new locality-middleware regressions.
- Windows PowerShell source/fixture checks: installer safety (35 assertions), task settings (28), windowless backend (15).
- Windows configuration overlay preserves both resource paths and the original NSIS install mode, languages and hook.

## Release build validation completed

Compiled application source: `bfdd4858b11d6ef22d83ecd7671713c5f20beaa1`. Subsequent publication edits only update documentation. [Windows and Linux CI run](https://github.com/jontechlabs/MinerDesk/actions/runs/35446343904): **success** for both jobs.

| Check | Result |
| --- | --- |
| Windows Server 2022 canonical release build | Desktop, both backends and NSIS installer compiled successfully. |
| Windows executable metadata | All four files report 0.7.22; desktop/backend/CLI are x64. Desktop and backend use GUI subsystem 2; CLI uses console subsystem 3. |
| Windows tests | 19 Node tests and 23 Rust library tests pass. Installer preflight: 36 assertions in CI; task settings: 28; windowless backend: 15. The standalone Rust scheduler/worker checks pass too. |
| Windows CLI smoke check | Downloaded `minerdesk-headless.exe --version` exits successfully and prints 0.7.22, before loading runtime configuration. |
| Ubuntu 22.04 CI | Frontend build, Node/source checks, Cargo check/tests, desktop and CLI compilation pass. |
| Debian 12 release build | Desktop/CLI, `.deb` and AppImage compile successfully using Node 24.21.0 and Rust 1.98.1 in Docker under WSL2. |
| Debian package | Final package installs in the isolated container; its file list contains desktop, standalone CLI and compatibility backend. Shared library resolution succeeds. |
| Desktop/AppImage smoke checks | Each starts as a non-root user in a virtual X display and exposes its 0.7.22 local backend. |
| Headless HTTP checks | Embedded frontend, health, engine list and status work; valid token and bearer access work; missing/invalid tokens and a forged locality header are rejected over a non-loopback connection. |
| Headless shutdown | SIGINT exits cleanly with status 0. |
| Secret scan | Gitleaks reports no leaks in the source directory or the two source commits. No personal config, logs, tokens, downloaded engines or generated executables are tracked. |
| Publication assets | Windows files match the CI build's hashes. Release assets include SHA-256 checksums and sanitized build provenance. |

The original Windows NSIS options/resource paths match the supplied 0.7.21 archive after merging the platform overlay. All new package targets use the same original application entry points.

## Limits

No live miner was started/stopped for validation. No real Windows installer, scheduled-task registration, Defender change, firewall change, hardware sleep/wake cycle or GPU tuning was exercised by the automated checks. Release binaries are unsigned. Linux compilation does not establish compatibility with every distro, and static tests do not establish physical hardware behavior.

Runtime behavior outside the explicitly documented security fix is preserved. Existing platform-specific unused/unreachable-code compiler warnings are not suppressed or treated as proof of a failed build.
