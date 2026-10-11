# Changelog

## 0.7.31

- Add fixed GPU memory clocks and signed core/memory frequency offsets to per-GPU tuning and global defaults, with units and engine-specific controls.
- Map the new settings to SRBMiner, lolMiner, BzMiner and Rigel; add NPMiner's CUDA memory-clock lock. Keep lpminer's documented shared core-clock control.
- Use responsive GPU cards and explain fixed clocks, offsets, driver-supported memory states and engine limitations.
- Extend explicit clearing and old-profile compatibility to all six fields without applying automatic overclock presets.
- Reject incomplete SRBMiner/NPMiner tuning lists and invalid fan percentages with actionable messages.
- Cover signed serialization, global/per-GPU arguments, selected device order, skip markers, clearing and process start/stop/restart.

## 0.7.30

- Display effective per-GPU tuning values and keep saved global defaults visible.
- Preserve old-profile inheritance until an explicit GPU edit; explicitly cleared fields no longer revive legacy values. Add Clear all GPU tuning.
- Validate generated and advanced SRBMiner power limits before launch and explain watts versus MHz and hardware limits.
- Include recent miner error output with nonzero exit codes, including late output after exit, in English or French.
- Quote console command arguments containing spaces without changing the direct process launch.
- Add inheritance, clearing, serialized configuration, validation, command display and diagnostic regressions.

## 0.7.29

- Open all desktop GitHub downloads links in the system's default browser on Windows and Linux; preserve ordinary navigation in browser dashboards.
- Restrict the native opening command to MinerDesk release pages and show a copyable address if browser launch fails.
- Report Linux Debian authorization cancellation, unavailable approval and package-manager exit codes with bounded installer diagnostics.
- Run Debian installation outside the interface's async worker and disable unusable terminal authentication fallback in the graphical app.
- Add native-link, browser-dashboard, URL validation and installer error regressions.

## 0.7.28

- Retry an occupied Linux/macOS desktop API port during update handover and retain startup errors in backend diagnostics.
- Make Start backend retry the built-in server without elevation, with one startup worker per desktop and a bounded wait off the UI thread.
- Display Desktop backend on Linux/macOS and reserve scheduled-task repair/restart controls for Windows.
- Report the actual desktop executable in Linux/macOS diagnostics and reconnect using the configured backend port.
- Add occupied-port, deadline, concurrent-start and platform-specific UI regressions.

## 0.7.27

- Extend Linux SRBMiner pseudo-terminal capture to live mining, so initialization errors and mining output are visible when ordinary pipes are silent.
- Strip terminal colors before console display and metric parsing; retain separate stdout/stderr streams and normal manual stop/restart behavior.
- Render nonzero process exit codes as numbers instead of Rust `Some(...)` values and direct users to the miner console.
- Add lifecycle regressions for terminal-only errors, no final newline, live metrics and stopping/restarting a running process.
- Document watts versus MHz and how to clear both per-GPU and legacy tuning for a default-settings test.

## 0.7.26

- Draw dark, readable select controls instead of relying on GTK's light native surface; declare the dark color scheme for native control menus.
- Wait for managed mining-engine downloads to return their installed executable path instead of aborting after five seconds. Bound upstream HTTP requests.
- Explain missing executable paths, launch failures, diagnostic timeouts and unsuccessful/unrecognized device listings in English and French; retain OS/loader errors in diagnostics.
- Separate stdout and stderr before parsing GPU rows and retain the guard against using system indices as mining-engine IDs.
- Capture Linux SRBMiner diagnostics through separate pseudo-terminals, because its output can be silent through ordinary pipes; strip ANSI color sequences before parsing and display.
- Add regressions for slow downloads, empty/missing paths and Linux permission, executable-format and shared-library errors.

## 0.7.25

- Parse SRBMiner's mixed OpenCL/CUDA GPU listings, preserving global device IDs and PCI addresses instead of substituting system/CUDA-local indices.
- Keep unverified fallback inventory informational for indexed engines and preserve saved GPU IDs/tuning.
- Allow slow GPU discovery, bound diagnostics to 60 seconds, run them outside HTTP request workers and discard stale profile results.
- Add regressions for the observed AMD/RTX listing, legacy/sparse IDs, clock overrides, fallback selection and diagnostic timeout/output handling.

## 0.7.24

- Add periodic stable-release checks, update prompts, manual checks and a 24-hour snooze.
- Verify update signatures before installing supported Windows, AppImage and Debian packages; show GitHub download links for other installations.
- Preserve saved profiles and schedules during installation and publish signed update manifests from tested CI builds.

## 0.7.23

- Add Quantus (QTC) detection to the frontend and backend and a public receiving address for optional developer support.
- Preserve `/worker` and `.worker` pool-login suffixes during Quantus support slices; saved user payout addresses and separate worker settings remain unchanged.
- Keep support disabled by default (0%), exclude custom engines, and avoid merging a secondary payout during a Quantus support slice.
- Add frontend/Rust regressions for detection, command arguments, user-wallet preservation and existing Pearl merge support.
- Produce Linux Debian/AppImage packages in CI alongside the Windows installer and executables.

## 0.7.22

- Fix LAN authentication bypass: discard client-supplied local-trust headers and derive local access from the actual connection peer. Add middleware tests for remote IPv4/IPv6, duplicate headers and legitimate loopback access.
- Split shared, Windows and Linux Tauri configuration while preserving the NSIS settings and both Windows backend resources.
- Declare all Cargo executable targets explicitly so Linux packages include the standalone CLI instead of relying on Tauri's directory scan.
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
