# Detached signed update helper

This independent crate is the privilege boundary for an explicitly enabled
MinerDesk Fleet update. It accepts no Fleet URL, public key, command, user,
process identifier, executable or output path. The application downloads while
mining continues, validates the signature, and starts the packaged helper.

The helper reads one newline-terminated JSON request (at most 8192 bytes):

```json
{"schema":1,"job_id":"ad243da8-6f11-4c19-b7b4-165aec9b11d8","expected_version":"0.8.3","package_path":"/tmp/minerdesk-fleet-update-example/package"}
```

The helper independently identifies the installed agent, original OS user and
actual agent configuration. It copies itself away from the installation,
fetches the official latest stable release and its version-specific manifest
from `jontechlabs/MinerDesk`, and verifies the package with the embedded Tauri
publisher key. Its authenticated signature comment must bind the exact selected
version, which must be newer than the installed version. Installer bytes are
copied into protected storage before the helper emits a newline JSON event:

```json
{"schema":1,"job_id":"ad243da8-6f11-4c19-b7b4-165aec9b11d8","stage":"ready","error":null}
```

Only then may the application stop its miners and send:

```json
{"schema":1,"job_id":"ad243da8-6f11-4c19-b7b4-165aec9b11d8","apply":true}
```

The worker waits for the identified original process to exit, installs, and
relaunches the original agent. A missing, mismatched or timed-out acknowledgment
never installs. No administrator password is stored.

## Linux

The Debian package installs the root-owned executable at
`/usr/lib/minerdesk/minerdesk-update-helper`. A local `pkexec` invocation with
exactly `--enroll` creates the calling UID's protected grant and a checked
`/etc/sudoers.d/minerdesk-updates-UID` rule. Its only passwordless command is the
literal installed helper with the literal `--apply` argument. It permits no
shell, argument wildcard, enrollment or environment override. `--revoke` is also
a local `pkexec` operation. The grant survives package upgrades.

The unprivileged application uses `/usr/bin/sudo -n` and that fixed command.
The root worker derives the UID from sudo and walks the actual sudo monitor
chain to the user's installed `/usr/bin/minerdesk` or `minerdesk-headless`.
It opens the user's private download directory and package by descriptor,
rejecting links, hardlinks, nonregular files and unexpected ownership.

Private staging and diagnostics live below
`/var/lib/minerdesk/updates/private`; private UID grants live in `grants`.
Public status lives at `public/UID-JOB.json`, and the enrollment marker at
`public/UID-enrolled.json`. No private token or argv appears in public errors.
The worker runs only `/usr/bin/dpkg --install` with its protected package and
then restores the original user's groups, GID and UID before restarting.

Writable user-owned AppImages use the same handshake without elevation, stage
an executable copy outside the mounted image, and replace the image atomically.
State lives in the original user's `~/.local/state/minerdesk/updates` directory.
The bundle must install the helper beside its actual MinerDesk executable.

## Windows

Only the already authorized installed `minerdesk-backend.exe` is eligible.
The helper validates its HKLM NSIS registration and protected installation,
captures the matching original user's interactive highest-privilege backend
task, and starts a detached protected worker under the OS ProgramData folder.
The worker waits for the backend to exit, uses the installed maintenance
`PrepareInstall` action to close the known installed UI and runtime, invokes
only the signed NSIS package, and restores the validated original backend task.
It never invokes Defender, firewall or general cleanup actions.

The package must install the helper at `resources/minerdesk-update-helper.exe`,
plus the existing maintenance scripts at `resources/maintenance.ps1` and
`resources/maintenance-common.ps1`. Public status is
`ProgramData/MinerDesk/updates/public/JOB.json`; private worker directories are
restricted to Administrators and SYSTEM.

## Fixture verification

`cargo test --manifest-path src-tauri/update-helper/Cargo.toml` exercises real
Tauri signatures made with a throwaway test key, tampering, wrong publishers,
missing/mismatched signed versions, frozen official metadata, strict URLs,
bounded messages, denied privileged overrides, and Linux descriptor ownership
and link checks. Tests never run an installer, write a root policy, or launch a
miner. The test key's private half is not retained.
