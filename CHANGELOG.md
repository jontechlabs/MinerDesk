# Changelog

## 0.7.22

- Fix LAN authentication bypass: discard client-supplied local-trust headers and derive local access from the actual connection peer. Add middleware tests for remote IPv4/IPv6, duplicate headers and legitimate loopback access.
- Split shared, Windows and Linux Tauri configuration while preserving the NSIS settings and both Windows backend resources.
- Add clean-checkout Windows resource preparation, reproducible dependency lockfiles, Linux build script, Windows/Linux CI, release checksums and publication documentation.
- Replace the accumulated README with current installation/use guides, a French introduction and real application screenshots.
- Preserve all 0.7.21 mining and scheduling behavior. Adjust a synthetic installer-test path so the test does not require an `F:` drive.

## 0.7.21

Clean manual Start no longer waits for an unnecessary save. Windows configuration synchronization runs in a serial/coalescing background worker. Dirty tuning still saves before Start, Stop does not save, partial Start all failures remain visible, and complete HTTP reads have timeouts without automatic command retries.

## 0.7.20

Manual Start/Restart creates user-owned sessions that survive schedule end, and cancels pending schedule power actions. Native Windows power-dialog focus no longer depends on visible WebView timers.

## 0.7.19

Dated manual schedule holds, overnight/overlapping-window handling, a dedicated windowless desktop backend, and regression coverage for those lifecycle policies.

## Earlier releases

Detailed supplied release notes from 0.6.8 onward, build notes and validation reports are preserved in [docs/history](docs/history/). They describe their original release, not the current installation contract. The original accumulated README and Windows setup guide are archived there too.
