#!/usr/bin/env python3
"""Run only the three known Linux x64 root fixtures from Cargo JSON artifacts.

Cargo 1.100 moved test outputs below debug/build/<package>/<hash>/out. Test
identity, source and containment are checked instead of assuming debug/deps.
"""
import argparse
import json
from pathlib import Path
import re
import stat
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = {
    'minerdesk-linux-helper': ('bin', 'main.rs'),
    'minerdesk-approved-helper': ('bin', 'approved.rs'),
    'root-fixtures': ('test', 'tests/root-fixtures.rs'),
}


def checked_path(value):
    path = Path(value)
    if not path.is_absolute() or '..' in path.parts:
        raise ValueError('Artifact paths must be absolute and contain no traversal')
    current = Path(path.anchor)
    for part in path.parts[1:]:
        current /= part
        if current.is_symlink():
            raise ValueError(f'Symlinks are not allowed in fixture paths: {current}')
    return path.resolve(strict=True)


def select_artifacts(records, crate_root, target_root):
    crate_root = checked_path(crate_root)
    target_root = checked_path(target_root)
    if not crate_root.is_dir() or not target_root.is_dir() or target_root == Path(target_root.anchor):
        raise ValueError('Expected explicit crate and target directories')
    manifest = checked_path(crate_root / 'Cargo.toml')
    selected, identities = {}, set()
    for item in records:
        if item.get('reason') != 'compiler-artifact' or item.get('profile', {}).get('test') is not True:
            continue
        raw_manifest = item.get('manifest_path')
        if not isinstance(raw_manifest, str):
            continue
        try:
            same_manifest = Path(raw_manifest).resolve(strict=True) == manifest
        except (OSError, ValueError):
            same_manifest = False
        if not same_manifest:
            continue  # Dependency test artifacts are never privileged fixtures.
        if checked_path(raw_manifest) != manifest:
            raise ValueError('Unexpected fixture manifest')
        target = item.get('target', {})
        if target.get('kind') == ['custom-build']:
            continue
        name = target.get('name')
        if name not in EXPECTED:
            raise ValueError(f'Unknown root fixture target: {name}')
        kind, source = EXPECTED[name]
        if target.get('kind') != [kind] or target.get('crate_types') != ['bin'] or target.get('test') is not True:
            raise ValueError(f'Unexpected target kind for {name}')
        if checked_path(target.get('src_path', '')) != checked_path(crate_root / source):
            raise ValueError(f'Unexpected source file for {name}')
        raw_executable = item.get('executable')
        if not isinstance(raw_executable, str) or raw_executable not in item.get('filenames', []):
            raise ValueError(f'Missing Cargo executable receipt for {name}')
        executable = checked_path(raw_executable)
        try:
            executable.relative_to(target_root)
        except ValueError as error:
            raise ValueError(f'Fixture escaped its intended target directory: {name}') from error
        if not re.fullmatch(re.escape(name.replace('-', '_')) + r'-[a-f0-9]{8,64}', executable.name):
            raise ValueError(f'Unexpected fixture executable name: {name}')
        metadata = executable.stat()
        if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
            raise ValueError(f'Fixture is not a regular executable file: {name}')
        with executable.open('rb') as stream:
            header = stream.read(64)
        if len(header) < 64 or header[:6] != b'\x7fELF\x02\x01' or struct.unpack_from('<H', header, 18)[0] != 62:
            raise ValueError(f'Expected a Linux x64 ELF test binary: {name}')
        identity = (metadata.st_dev, metadata.st_ino)
        if name in selected or identity in identities:
            raise ValueError(f'Duplicated or ambiguous root fixture: {name}')
        selected[name] = executable
        identities.add(identity)
    if set(selected) != set(EXPECTED):
        raise ValueError('Cargo must report exactly the three expected root fixture targets')
    return [selected[name] for name in sorted(selected)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('messages', type=Path)
    parser.add_argument('--target-dir', type=Path, default=ROOT / 'src-tauri/linux-helper/target')
    parser.add_argument('--verify-only', action='store_true')
    args = parser.parse_args()
    if args.messages.stat().st_size > 8 * 1024 * 1024:
        raise ValueError('Cargo artifact receipts exceed the expected size')
    records = [json.loads(line) for line in args.messages.read_text().splitlines() if line.strip()]
    executables = select_artifacts(records, ROOT / 'src-tauri/linux-helper', args.target_dir)
    for executable in executables:
        print(f'Verified root fixture: {executable}', flush=True)
    if not args.verify_only:
        for executable in executables:
            subprocess.run(['sudo', '-n', str(executable), '--ignored', '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
