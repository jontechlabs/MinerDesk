#!/usr/bin/env python3
"""Optional non-Windows regression check. Requires Python 3 + lxml.

Windows builds instead run backend-task-settings.tests.ps1 (.NET schema/parser).
This tests the actual Interval constraint and the policy literals in all three
registration entry points; it does not emulate the Windows Task Scheduler.
"""
import argparse
from pathlib import Path
import re
from lxml import etree

ROOT = Path(__file__).resolve().parents[1]
PATHS = ('src-tauri/windows/maintenance.ps1',
         'scripts/install-privileged-backend.ps1', 'src-tauri/src/lib.rs')
INTERVAL = re.compile(r'-RestartInterval\s+\(New-TimeSpan\s+-(Seconds|Minutes|Hours|Days)\s+(\d+)\)', re.I)
FACTORS = {'seconds': 1, 'minutes': 60, 'hours': 3600, 'days': 86400}


def seconds_from_source(root, relative):
    source = (root / relative).read_text()
    intervals = list(INTERVAL.finditer(source))
    assert len(intervals) == 1, (relative, len(intervals))
    m = intervals[0]
    return int(m[2]) * FACTORS[m[1].lower()]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--old-root', type=Path)
    args = parser.parse_args()
    schema = etree.XMLSchema(etree.parse(str(ROOT / 'tests/task-restart-interval.xsd')))

    def valid(duration):
        element = etree.Element('Interval')
        element.text = duration
        return schema.validate(element)

    cases = {'PT0S': False, 'PT15S': False, 'PT59S': False, 'PT1M': True,
             'PT60S': True, 'PT5M': True, 'P31D': True, 'P32D': False}
    for duration, expected in cases.items():
        assert valid(duration) == expected, duration
        print(f'PASS: Windows Interval constraint {"accepts" if expected else "rejects"} {duration}')

    for relative in PATHS:
        seconds = seconds_from_source(ROOT, relative)
        assert valid(f'PT{seconds}S'), relative
        assert seconds == 60, relative
        source = (ROOT / relative).read_text()
        assert len(re.findall(r'-RestartCount\s+3\b', source)) == 1
        print(f'PASS: {relative}: 3 retries, {seconds} seconds, valid schema')
        if args.old_root:
            previous = seconds_from_source(args.old_root, relative)
            assert previous == 15 and not valid(f'PT{previous}S')
            print(f'REPRODUCED: 0.7.17 {relative}: {previous} seconds is rejected')

    hooks = (ROOT / 'src-tauri/windows/hooks.nsh').read_text()
    assert hooks.count('FileSeek $MdHandle 0 END $MdLogOffset') == 2
    assert hooks.count('FileWriteUTF16LE /BOM $MdHandle "$MdLogMessage') == 1
    assert '$MdCode == 61' in hooks and '$MdCode == 62' in hooks
    assert '"0.7.17" md_legacy_found' in hooks
    helper = (ROOT / 'src-tauri/windows/maintenance.ps1').read_text()
    assert helper.index('Registering backend task:') < helper.index('\n                Register-ScheduledTask ')
    assert 'Throw-MdError 61 "Register-ScheduledTask failed' in helper
    assert 'Throw-MdError 62 ' in helper
    build = (ROOT / 'scripts/build-windows.ps1').read_text()
    assert "'tests\\backend-task-settings.tests.ps1'" in build
    print('PASS: append-only NSIS logs, stage-specific errors, partial-0.7.17 migration and build preflight')
    print('Task policy source/schema checks passed. This is NOT a live Windows registration test.')


if __name__ == '__main__':
    main()
