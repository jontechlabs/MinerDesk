# MinerDesk 0.7.18 — validation record

## Evidence and scope

The supplied 0.7.17 log reports successful Inspect/Stop/PrepareInstall phases,
then an InstallBackend failure at Register-ScheduledTask with Interval:PT15S.
The diagnosis does not rely on Task Manager process display names. Microsoft
restricts Task Scheduler RestartOnFailure/Interval to PT1M through P31D:
https://learn.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-interval-restarttype-element

The original 0.7.17 code contains the invalid literal in three production paths.
All three now use New-TimeSpan -Minutes 1. Retry count (3), task flags, heartbeat,
normal shutdown and Job Object logic remain otherwise unchanged. A registered
0.7.17 is also eligible for the existing consent-based migration cleanup.

## Checks actually executed in the preparation environment

Source checks against the original 0.7.17 archive:

```text
PASS: Every hook source line has balanced NSIS quoting
PASS: One shared nsExec call: one timeout option and exactly one complete command argument
PASS: Both plugin stack values are consumed immediately
PASS: ProgramData path uses defined NSIS shell variables
PASS: 13 expanded functions have unique names, balanced conditionals and resolved calls
PASS: All explicit jump labels inside expanded functions resolve locally
PASS: Every runtime variable used by expanded hooks is declared or a known NSIS variable
PASS: Literal/comment-aware delimiter checks pass for 12 PowerShell files (not parser execution)
PASS: Package, Tauri and Rust version metadata agree on 0.7.18
PASS: Build contains native exit-code checks, preflight, required current Setup and deferred publishing
PASS: Obsolete separately-maintained legacy PowerShell helper has been removed
PASS: Rust runtime unchanged except version strings and the repair-task restart interval
Source checks completed: 12. Windows runtime/compiler testing remains separate.
```

The task interval was validated using Python/lxml against an isolated XSD fixture
containing the exact Microsoft minInclusive/maxInclusive constraints. This tests
that constraint, not the whole Task Scheduler schema or a running Windows service.

```text
PASS: Windows Interval constraint rejects PT0S
PASS: Windows Interval constraint rejects PT15S
PASS: Windows Interval constraint rejects PT59S
PASS: Windows Interval constraint accepts PT1M
PASS: Windows Interval constraint accepts PT60S
PASS: Windows Interval constraint accepts PT5M
PASS: Windows Interval constraint accepts P31D
PASS: Windows Interval constraint rejects P32D
PASS: src-tauri/windows/maintenance.ps1: 3 retries, 60 seconds, valid schema
REPRODUCED: 0.7.17 src-tauri/windows/maintenance.ps1: 15 seconds is rejected
PASS: scripts/install-privileged-backend.ps1: 3 retries, 60 seconds, valid schema
REPRODUCED: 0.7.17 scripts/install-privileged-backend.ps1: 15 seconds is rejected
PASS: src-tauri/src/lib.rs: 3 retries, 60 seconds, valid schema
REPRODUCED: 0.7.17 src-tauri/src/lib.rs: 15 seconds is rejected
PASS: append-only NSIS logs, stage-specific errors, partial-0.7.17 migration and build preflight
Task policy source/schema checks passed. This is NOT a live Windows registration test.
```

Additional checks executed:
- Parsed all 5 JSON files, the Cargo TOML file and all Python test source files.
- Scanned every production .ps1/.rs/.nsh under scripts/ and src-tauri/: exactly
  three restart-interval definitions, all New-TimeSpan -Minutes 1.
- Compared frontend and entrypoints against 0.7.17: only release labels differ.
- Compared full lib.rs against 0.7.17: only release labels and the single
  repair-task interval literal differ; mining/process-lifecycle logic unchanged.
- Exercised a UTF-16 append model using NSIS's documented initial pointer
  position: explicit seek-to-EOF retains all entries with one initial BOM.
  This is a model, not Windows file API execution.
- Packaging checks: ZIP CRC, relative-path patch dry-run/application and complete
  patched-tree hash equality with the delivered source tree.

## Windows checks included but NOT executed here

This Linux environment has no Windows PowerShell, makensis, Wine or Windows/MSVC
Rust toolchain, and network access did not permit installing missing tools.
No successful Windows application compilation, live Scheduled Task registration,
backend start or interactive Setup/uninstall test is claimed.

The unchanged supported build workflow automatically runs:
1. tests/installer-safety.tests.ps1: actual Windows PowerShell parser plus the
   existing synthetic-process and request-file tests, with added log-append and
   phase-diagnostic source assertions.
2. tests/backend-task-settings.tests.ps1: actual PowerShell CommandAst extraction
   of each production settings statement; .NET XML Schema rejection of PT15S,
   PT59S, PT0S and P32D; acceptance of PT1M, PT60S, PT5M and P31D; consistency of
   all three policies (three retries, 60 seconds, unlimited execution, IgnoreNew).

Neither suite registers a task, starts miners or stops running processes. The
existing request-file test creates/removes only its own temporary fixture.
The PowerShell suites above were not executed in the preparation environment.
A schema-constraint test is not a substitute for end-to-end Windows registration.

## Acceptance check on the user's Windows machine

Build in the new 0.7.18 folder using scripts/build-windows.ps1. Run only
MinerDesk_0.7.18_x64-setup.exe after BUILD SUCCEEDED. Accept maintenance/privileged
backend installation as appropriate. Inspect the registered task:

```powershell
$task = Get-ScheduledTask -TaskName 'MinerDesk Privileged Backend' -ErrorAction Stop
$task.Settings | Select-Object RestartCount, RestartInterval
$task.Actions | Select-Object Execute, Arguments
```

Expect 3 retries and a one-minute duration (normally PT1M), with --desktop-owned.
Check that setup-maintenance.log retains preceding phases and records the policy
before registration. backend-task.xml is an optional post-registration export.
Then test Desktop connectivity and a non-critical mining session separately.

NSIS log semantics reference:
https://nsis.sourceforge.io/Docs/Chapter4.html#fileopen
