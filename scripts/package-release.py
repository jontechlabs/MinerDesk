#!/usr/bin/env python3
"""Package a tested CI revision, sign update assets, verify, and write latest.json.

Requires Python 3.11+, Node/npm dependencies and minisign on the packaging host.
Private key/password arrive ONLY through Tauri's environment variables.
"""
import argparse
import base64
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def sha(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

def verify_update(path, pubkey):
    signature = base64.b64decode(path.with_name(path.name + '.sig').read_text().strip(), validate=True)
    decoded_key = base64.b64decode(pubkey, validate=True).decode().splitlines()[-1]
    with tempfile.TemporaryDirectory(prefix='minerdesk-signature-') as directory:
        sig = Path(directory) / 'signature.minisig'
        sig.write_bytes(signature)
        run = subprocess.run(['minisign', '-Vm', str(path), '-x', str(sig), '-P', decoded_key], capture_output=True)
        if run.returncode:
            raise RuntimeError(f'Update signature verification failed: {path.name}')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--run-url', required=True)
    parser.add_argument('--notes', type=Path, required=True)
    args = parser.parse_args()
    version = json.loads((ROOT / 'package.json').read_text())['version']
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if head != args.source_commit:
        raise RuntimeError('The checked-out source differs from the compiled revision')
    out = args.output.resolve()
    if out.exists() and any(out.iterdir()):
        raise RuntimeError('Output must be empty; published assets must never be overwritten')
    out.mkdir(parents=True, exist_ok=True)
    windows, linux = args.artifacts / 'windows-x64', args.artifacts / 'linux-x64'
    info = json.loads((windows / 'build-info.json').read_text(encoding='utf-8-sig'))
    if info['version'] != version:
        raise RuntimeError('Wrong Windows artifact version')
    for name, expected in info['sha256'].items():
        if sha(windows / name).lower() != expected.lower():
            raise RuntimeError(f'Windows artifact hash mismatch: {name}')
    for line in (linux / 'SHA256SUMS.txt').read_text().splitlines():
        expected, name = line.split(maxsplit=1)
        if Path(name).name != name or sha(linux / name) != expected:
            raise RuntimeError('Linux artifact hash mismatch')
    records = {}
    for name in ['MinerDesk.exe', 'minerdesk-headless.exe', 'minerdesk-backend.exe']:
        raw = (windows / name).read_bytes()
        offset = struct.unpack_from('<I', raw, 0x3c)[0]
        if raw[:2] != b'MZ' or raw[offset:offset+4] != b'PE\0\0' or struct.unpack_from('<H', raw, offset+4)[0] != 0x8664:
            raise RuntimeError('Wrong Windows executable architecture')
        subsystem = struct.unpack_from('<H', raw, offset+24+68)[0]
        if subsystem != (3 if name == 'minerdesk-headless.exe' else 2):
            raise RuntimeError('Wrong Windows executable subsystem')
        records[name] = {'machine':'x86_64','subsystem':subsystem}
    for name in ['minerdesk','minerdesk-headless']:
        raw = (linux / name).read_bytes()[:64]
        if raw[:4] != b'\x7fELF' or raw[4] != 2 or struct.unpack_from('<H', raw, 18)[0] != 62:
            raise RuntimeError('Wrong Linux executable architecture')
    installer = f'MinerDesk_{version}_x64-setup.exe'
    appimage = f'MinerDesk_{version}_amd64.AppImage'
    deb = f'MinerDesk_{version}_amd64.deb'
    for directory, name in [(windows, installer), (windows, 'minerdesk-headless.exe'), (linux, appimage), (linux, deb)]:
        shutil.copyfile(directory / name, out / name)
    with zipfile.ZipFile(out / f'MinerDesk_{version}_windows-x64.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
        for name in records: archive.write(windows / name, name)
        for name in ['LICENSE','SETUP-WINDOWS.md','SECURITY.md','docs/UPDATES.md','docs/QUANTUS.md']: archive.write(ROOT / name, name)
    with tarfile.open(out / f'MinerDesk_{version}_linux-x64.tar.gz', 'w:gz') as archive:
        for name in ['minerdesk','minerdesk-headless']:
            entry = archive.gettarinfo(str(linux / name), arcname=name)
            entry.mode = 0o755; entry.uid = entry.gid = 0; entry.uname = entry.gname = ''
            with (linux / name).open('rb') as file: archive.addfile(entry, file)
        for name in ['LICENSE','BUILD-LINUX.md','SECURITY.md','docs/UPDATES.md','docs/QUANTUS.md']: archive.add(ROOT / name, arcname=name)
    subprocess.run(['git','archive','--format=zip','-o',str(out / f'MinerDesk_{version}_source.zip'),head],cwd=ROOT,check=True)
    pubkey = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())['plugins']['updater']['pubkey']
    for name in [installer, appimage, deb]:
        result = subprocess.run(['node',str(ROOT / 'node_modules/@tauri-apps/cli/tauri.js'),'signer','sign',str(out / name)],capture_output=True)
        if result.returncode:
            raise RuntimeError('Update signing failed (private diagnostic output withheld)')
        verify_update(out / name, pubkey)
    now = datetime.now(timezone.utc).isoformat().replace('+00:00','Z')
    manifest = {'version':version,'notes':args.notes.read_text(encoding='utf-8'),'pub_date':now,'platforms':{}}
    for target,name in [('windows-x86_64',installer),('linux-x86_64-appimage',appimage),('linux-x86_64-deb',deb)]:
        manifest['platforms'][target] = {'url':f'https://github.com/jontechlabs/MinerDesk/releases/download/v{version}/{name}','signature':(out / (name+'.sig')).read_text().strip()}
    (out / 'latest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
    receipt = {'version':version,'source_commit':head,'run_url':args.run_url,'packaged_utc':now,'windows_authenticode':info.get('signing','unsigned'),'update_signatures':'verified against embedded public key using minisign','windows_executables':records,'live_gpu_mining_tested':False}
    (out / 'BUILD-INFO.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    (out / 'SHA256SUMS.txt').write_text(''.join(f'{sha(path)}  {path.name}\n' for path in sorted(out.iterdir()) if path.name != 'SHA256SUMS.txt'),encoding='utf-8')
    print(json.dumps({'version':version,'commit':head,'verified_update_assets':[installer,appimage,deb],'assets':[p.name for p in sorted(out.iterdir())]},indent=2))

if __name__ == '__main__': main()
