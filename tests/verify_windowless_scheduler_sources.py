#!/usr/bin/env python3
"""Read-only source invariants; NOT a Rust/PowerShell/Windows runtime test."""
import argparse
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--baseline', type=Path)
args = parser.parse_args()
checks = 0

def read(path):
    return (root / path).read_text(encoding='utf-8-sig')

def check(value, description):
    global checks
    if not value:
        raise AssertionError(description)
    checks += 1
    print('PASS:', description)

lib = read('src-tauri/src/lib.rs')
policy = read('src-tauri/src/schedule_control.rs')
cli = read('src-tauri/src/bin/minerdesk-headless.rs')
backend = read('src-tauri/src/bin/minerdesk-backend.rs')
check('windows_subsystem = "windows"' in backend and 'run_desktop_backend()' in backend,
      'Separate windowless binary calls shared runtime directly')
check('windows_subsystem' not in cli and 'run_headless()' in cli,
      'CLI source retains console entry point')
check('let desktop_owned = windowless || args.desktop_owned;' in lib,
      'Windowless binary always Desktop-owned, CLI optional')
check('if windowless { std::future::pending::<()>().await; }' in lib,
      'No dependence on Ctrl+C registration without a console')
for path in ('src-tauri/windows/maintenance.ps1', 'scripts/install-privileged-backend.ps1'):
    check('minerdesk-backend.exe' in read(path), 'Windowless task target: ' + path)
locator = lib.split('fn locate_desktop_backend_executable()', 1)[1].split('\n#[cfg', 1)[0]
check('"minerdesk-backend.exe"' in locator and '"minerdesk-headless.exe"' not in locator,
      'Desktop locator does not silently fall back to visible CLI on Windows')
resources = json.loads(read('src-tauri/tauri.windows.conf.json'))['bundle']['resources']
check({'resources/minerdesk-backend.exe', 'resources/minerdesk-headless.exe'} <= set(resources),
      'Both executable resources packaged')
build = read('scripts/build-windows.ps1')
check('Assert-MdExeSubsystem -Path $backend -Expected 2' in build and
      'Assert-MdExeSubsystem -Path $headless -Expected 3' in build,
      'Actual PE subsystem validated at Windows build time')
check('scheduler-control.tests.ps1' in build and 'windowless-backend.tests.ps1' in build,
      'Windows preflight includes production policy tests and packaging checks')
for command in ('nvidia-smi', 'powercfg.exe', 'powershell.exe'):
    check(f'background_command("{command}")' in lib, 'No extra console for ' + command)
check('scheduler-manual-stops.json' in lib and 'fs::rename(&temporary, &path)' in lib,
      'Dated manual holds persisted with atomic replacement')
check('format!("{}@{}", window.id, start_day)' in policy and 'now.day.checked_sub(1)?' in policy,
      'Occurrences include local start date and overnight prior day')
check('!stopped.is_disjoint(current)' in policy,
      'Active overlap with any held occurrence prevents restart')
for route, action in (('api_stop(', 'stop'), ('api_restart(', 'restart')):
    part = lib.split('async fn ' + route, 1)[1].split('\nasync fn ', 1)[0]
    check(f'miner_command_response(s.core, id, "{action}").await' in part,
          'Manual API uses shared command adapter: ' + action)
adapter = lib.split('async fn miner_command_response(', 1)[1].split('\nasync fn ', 1)[0]
check('core.stop_miner_manually(&id)' in adapter and 'core.restart_miner_manually(&id)' in adapter,
      'Adapter preserves manual schedule-hold policy')
part = lib.split('async fn api_stop_all(', 1)[1].split('\nasync fn ', 1)[0]
check('stop_all_manually()' in part, 'Stop all preserves manual schedule-hold policy')
scheduler = lib.split('fn start_scheduler(', 1)[1].split('// ---------- Web API', 1)[0]
check('state.lifecycle.lock()' in scheduler and '!control.is_paused(id, &occurrences)' in scheduler,
      'Scheduler serializes lifecycle and honors manual holds before start')
check('self.shutting_down.store(true, Ordering::SeqCst)' in lib and
      'if self.shutting_down.load(Ordering::SeqCst)' in lib,
      'Shutdown gate prevents new starts')
check(policy.count('#[test]') >= 15,
      'At least 15 production-policy test cases present (not executed by this checker)')
check('scheduler_paused' in read('src/App.tsx') and 'schedulerPausedHelp' in read('src/i18n.ts'),
      'Pause state exposed by UI with localized help')
if args.baseline:
    old = (args.baseline / 'src-tauri/src/lib.rs').read_text()
    start = 'mod windows_miner_job {'
    end = '#[derive(Debug, Clone, Serialize, Deserialize)]'
    check(old.split(start, 1)[1].split(end, 1)[0] == lib.split(start, 1)[1].split(end, 1)[0],
          'Existing Job Object / ManagedChild implementation byte-identical to supplied baseline')
    check((args.baseline / 'src-tauri/src/bin/minerdesk-headless.rs').read_text() == cli,
          'CLI entry file byte-identical to supplied baseline')
print(f'Source invariants passed: {checks}. No compiler or live Windows execution is implied.')
