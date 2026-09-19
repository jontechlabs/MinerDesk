# Publication validation — 0.7.22

Source baseline: the author's 0.7.21 manual-start-command-fix archive. The 0.7.22 publication adds the LAN locality-header fix, cross-platform packaging, dependency lockfiles and documentation. Original Windows 0.7.21 executables are not redistributed as corrected builds.

## Local checks completed

- Frontend TypeScript/Vite production compilation.
- 17 existing Node command/transport regressions and 2 platform-config tests.
- 23 Rust library tests on Debian 12, including scheduler, configuration worker, tuning/uptime and new locality-middleware regressions.
- Windows PowerShell source/fixture checks: installer safety (35 assertions), task settings (28), windowless backend (15).
- Windows configuration overlay preserves both resource paths and the original NSIS install mode, languages and hook.

## Release build validation

Linux release compilation and packaging, Windows CI compilation/packaging, repository secret scanning and final asset verification are completed before publishing the release. The release notes provide the final build provenance and verification results.

## Limits

No live miner was started/stopped for validation. No real Windows installer, scheduled-task registration, Defender change, firewall change, hardware sleep/wake cycle or GPU tuning was exercised by the automated checks. Release binaries are unsigned. Linux compilation does not establish compatibility with every distro, and static tests do not establish physical hardware behavior.

Runtime behavior outside the explicitly documented security fix is preserved. Existing platform-specific unused/unreachable-code compiler warnings are not suppressed or treated as proof of a failed build.
