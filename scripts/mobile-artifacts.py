#!/usr/bin/env python3
"""Verify and package CI mobile previews, never an App Store or production release."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import struct
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MOBILE = ROOT / 'apps/mobile'


def run(args):
    return subprocess.check_output([str(arg) for arg in args], text=True, stderr=subprocess.STDOUT)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def android(out, version, identifier):
    sdk = Path(os.environ['ANDROID_HOME']) / 'build-tools/36.0.0'
    source = MOBILE / 'src-tauri/gen/android/app/build/outputs/apk'
    architectures = {'arm64-v8a': ('arm64', 183), 'x86_64': ('x86_64', 62)}
    found = {}
    for apk in sorted(source.rglob('*.apk')):
        with zipfile.ZipFile(apk) as archive:
            payloads = [name for name in archive.namelist() if re.fullmatch(r'lib/[^/]+/libminerdesk_mobile_lib\.so', name)]
            if len(payloads) != 1:
                raise RuntimeError(f'Expected one ABI per preview APK: {apk.name}')
            abi = payloads[0].split('/')[1]
            if abi not in architectures or abi in found:
                raise RuntimeError(f'Unexpected or duplicated Android ABI: {abi}')
            with archive.open(payloads[0]) as stream:
                header = stream.read(64)
            if header[:4] != b'\x7fELF' or header[4] != 2 or struct.unpack_from('<H', header, 18)[0] != architectures[abi][1]:
                raise RuntimeError('Android native payload architecture differs from its ABI')
        badging = run([sdk / 'aapt', 'dump', 'badging', apk])
        package = re.search(r"package: name='([^']+)'[^\n]*versionName='([^']+)'", badging)
        if not package or package.groups() != (identifier, version) or 'application-debuggable' not in badging:
            raise RuntimeError('APK identity, version or debug configuration differs from the preview contract')
        if "uses-permission: name='android.permission.INTERNET'" not in badging:
            raise RuntimeError('Android preview is missing Internet permission')
        certificate = run([sdk / 'apksigner', 'verify', '--verbose', '--print-certs', apk])
        if 'CN=Android Debug' not in certificate:
            raise RuntimeError('Expected the ephemeral Android debug certificate, not a production key')
        public_digest = re.search(r'Signer #1 certificate SHA-256 digest: ([a-fA-F0-9]+)', certificate)
        if not public_digest:
            raise RuntimeError('Missing verified Android signer fingerprint')
        name = f'MinerDesk-Mobile_{version}_android-{architectures[abi][0]}-preview.apk'
        shutil.copyfile(apk, out / name)
        found[abi] = {'file': name, 'certificate_sha256': public_digest[1].lower()}
    if set(found) != set(architectures):
        raise RuntimeError('Both arm64 and x86_64 Android previews are required')
    return {'kind': 'Android debug APK previews', 'architectures': found,
            'signing': 'Ephemeral Android debug key; not a production release'}


def ios(out, version, identifier):
    if sys.platform != 'darwin':
        raise RuntimeError('iOS Simulator packaging requires macOS')
    apps = sorted((MOBILE / 'src-tauri/gen/apple/build').rglob('*.app'))
    if len(apps) != 1:
        raise RuntimeError(f'Expected one exported simulator app, found {len(apps)}')
    app = apps[0]
    with (app / 'Info.plist').open('rb') as stream:
        info = plistlib.load(stream)
    if info.get('CFBundleIdentifier') != identifier or info.get('CFBundleShortVersionString') != version:
        raise RuntimeError('iOS Simulator bundle identity or version mismatch')
    if 'iPhoneSimulator' not in info.get('CFBundleSupportedPlatforms', []):
        raise RuntimeError('Refusing to label a physical-device build as a simulator preview')
    executable = info.get('CFBundleExecutable', '')
    if not executable or Path(executable).name != executable or not (app / executable).is_file():
        raise RuntimeError('Missing iOS Simulator executable')
    architectures = run(['lipo', '-archs', app / executable]).strip().split()
    if not architectures or any(arch not in ['arm64', 'x86_64'] for arch in architectures):
        raise RuntimeError('Unexpected iOS Simulator executable architecture')
    name = f'MinerDesk-Mobile_{version}_ios-simulator-preview.zip'
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(out / name)], check=True)
    return {'kind': 'iOS Simulator preview', 'architectures': architectures, 'file': name,
            'signing': 'No Apple signing credentials; simulator only, not an iPhone IPA'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=['android', 'ios'])
    parser.add_argument('--source-commit', required=True)
    args = parser.parse_args()
    commit = run(['git', '-C', ROOT, 'rev-parse', 'HEAD']).strip()
    if commit != args.source_commit:
        raise RuntimeError('Build provenance does not match this checkout')
    package = json.loads((MOBILE / 'package.json').read_text())
    config = json.loads((MOBILE / 'src-tauri/tauri.conf.json').read_text())
    version = package['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+', version) or config['version'] != version:
        raise RuntimeError('Mobile version mismatch')
    out = ROOT / f'publish/mobile-{args.platform}-preview'
    out.mkdir(parents=True, exist_ok=True)
    if any(out.iterdir()):
        raise RuntimeError('Preview output must be empty; refusing stale or overwritten artifacts')
    details = (android if args.platform == 'android' else ios)(out, version, config['identifier'])
    details.update({'version': version, 'source_commit': commit, 'preview_only': True,
                    'run_url': f'https://github.com/jontechlabs/MinerDesk/actions/runs/{os.environ.get("GITHUB_RUN_ID", "local")}'})
    (out / 'BUILD-INFO.json').write_text(json.dumps(details, indent=2) + '\n')
    (out / 'README-PREVIEW.txt').write_text(
        'MinerDesk Mobile preview\n\n'
        'Android APKs use a temporary debug certificate and are for testing only.\n'
        'A later preview may have a different certificate and require uninstalling the older preview.\n'
        'The iOS archive is for the iOS Simulator on a Mac; it is not installable on an iPhone.\n'
        'No store submission, production mobile signing key or Apple credentials are included.\n'
        'Connect using the MinerDesk web API address and access key, not a Fleet invitation.\n'
        'Read docs/MOBILE.md in the source repository for setup and network requirements.\n')
    (out / 'SHA256SUMS.txt').write_text(''.join(f'{digest(path)}  {path.name}\n' for path in sorted(out.iterdir())))
    print(json.dumps(details, indent=2))


if __name__ == '__main__':
    main()
