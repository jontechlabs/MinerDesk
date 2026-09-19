# MinerDesk 0.7.17 — validation record

## Concrete fault reproduced at source-tokenization level

The 0.7.16 `src-tauri/windows/hooks.nsh` was inspected directly from the supplied
source ZIP. Its inline commands use a single-quoted NSIS argument containing
unescaped single quotes. Under the published NSIS line-tokenization rules,
**27 of 27 inline PowerShell calls split into multiple plugin arguments**.

In particular, the pre-install verification call splits into **seven arguments**
instead of one. Its first command argument stops at:

```text
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$desktop=
```

The source spells the PowerShell dollar as `$$` for NSIS escaping. This command is
incomplete; checking executable paths instead of names could not fix its quoting.
The old hook then treated every nonzero result as “MinerDesk is still running”.
The undefined `$COMMONAPPDATA` string and unchecked native build exit statuses
were separate defects found in the same source review.

The new hook has **one shared plugin call** with exactly one timeout option and
one complete command argument. It uses `-File` and an encoded request file; no
PowerShell program is embedded in an NSIS string.

## Checks actually run in the preparation environment

The dependency-free source verifier was run against the complete 0.7.17 project
and the original 0.7.16 source tree. Output:

```text
PASS: Every hook source line has balanced NSIS quoting
PASS: One shared nsExec call: one timeout option and exactly one complete command argument
PASS: Both plugin stack values are consumed immediately
PASS: ProgramData path uses defined NSIS shell variables
PASS: 13 expanded functions have unique names, balanced conditionals and resolved calls
PASS: All explicit jump labels inside expanded functions resolve locally
PASS: Every runtime variable used by expanded hooks is declared or a known NSIS variable
PASS: Literal/comment-aware delimiter checks pass for 11 PowerShell files (not parser execution)
PASS: Package, Tauri and Rust version metadata agree on 0.7.17
PASS: Build contains native exit-code checks, preflight, required current Setup and deferred publishing
PASS: Obsolete separately-maintained legacy PowerShell helper has been removed
REPRODUCED: 27/27 old inline calls split into multiple NSIS arguments.
Old pre-install verification argument count: 7
Old first command argument: powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$desktop=
PASS: Rust runtime is byte-for-byte unchanged except version strings
Source checks completed: 12. Windows runtime/compiler testing remains separate.
```

Every JSON and TOML file was also parsed successfully. Rust runtime source was
compared byte-for-byte with 0.7.16 after replacing version strings; mining logic
is otherwise unchanged. The final ZIP was checked for CRC/integrity and the
patch was dry-run against a clean 0.7.16 tree (see packaging checks).

## Windows checks supplied, but not executed in this environment

This Linux environment has no Windows PowerShell, `makensis`, Wine, or Windows
Rust/MSVC toolchain. Network access did not allow installing missing tools.
Therefore **no successful full Windows build or installer-runtime test is claimed**.
Literal/delimiter checks are not a substitute for the PowerShell parser or NSIS.

`build-windows.ps1` automatically invokes `tests/installer-safety.tests.ps1` before
compilation. On Windows that test runs the actual PowerShell parser on all `.ps1`
files and tests synthetic snapshots for: no processes, installer-only bootstrap/
worker, real installed runtime, a Desktop that opened Setup, independent standalone
backends, managed-directory boundaries, PID reuse, inaccessible candidate paths,
and Unicode/apostrophe request parameters. It also exercises the actual build
wrapper with a harmless native command returning exit code 7. No real process is
stopped and no scheduled task or registry entry is modified by these tests.

`tests/hooks-smoke.nsi` is an optional compilation-only fixture for standard NSIS
and MUI2; its generated executable exits immediately if accidentally run. The
normal Tauri build also compiles all installer and uninstaller hook paths.

## Installation acceptance checks still required on Windows

Build from a fresh 0.7.17 directory with your usual PowerShell build script. It
must finish with `BUILD SUCCEEDED: MinerDesk 0.7.17` and produce exactly the
current versioned Setup plus `build-info.json` in `publish\windows-x64`.

First test with no Desktop or miners running. Then test active-runtime maintenance
and refusal of the shutdown prompt, followed by a direct uninstall. Legacy 0.7.7
migration should use the new helper rather than its old uninstaller. Confirm that
AppData settings and downloaded miners remain. Failure must identify the actual
phase/code and a log path; only code 20 asserts confirmed remaining processes.

References used for the quoting and process-launch contract:
- NSIS scripting reference: https://nsis.sourceforge.io/Docs/Chapter4.html
- NSIS line parser: https://github.com/kichik/nsis/blob/master/Source/lineparse.cpp
- Microsoft native exit-code semantics: https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_automatic_variables
