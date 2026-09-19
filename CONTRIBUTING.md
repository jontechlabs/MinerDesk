# Contributing

Start with a focused issue or pull request. Explain the user-visible problem, expected behavior, platform, MinerDesk version and engine version. Include minimal sanitized logs; never attach personal `config.json`, tokens, seeds, private keys or downloaded engines.

Use Node.js LTS, npm and Rust stable, then `npm ci --include=dev`. The committed npm and Cargo lockfiles are part of the application build. Read [Windows](SETUP-WINDOWS.md) or [Linux](BUILD-LINUX.md) setup instructions before building.

Run `npm test`, `npm run build`, and `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib`. Windows packaging uses `scripts/build-windows.ps1`; Linux uses `scripts/build-linux.sh`. Source checks under `tests/` supplement compilation; they are not live GPU/installer validation.

Preserve windowless desktop backend ownership, independent console CLI behavior, Job Object protection, heartbeat/PID checks, dated scheduler holds, user-owned manual starts, fresh tuning arguments, uptime reset, and the one-minute task restart interval. Changes to these behaviors need focused regression coverage.

Keep Windows installer resources in the Windows Tauri config. Do not commit generated resources, executables, `target`, `node_modules`, `dist`, `publish`, logs or personal configuration. Check new screenshots for identifying data and credentials.

Historical release notes are preserved in `docs/history/` as historical evidence. The current README and source take precedence over obsolete instructions there.
