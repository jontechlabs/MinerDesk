#!/usr/bin/env python3
"""Verify production Android APKs before staging a manually reviewed release.

Only public certificate fingerprints and artifact metadata enter this output.
The private Android keystore and its passwords are never read by this script.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MOBILE = ROOT / 'apps/mobile'
ARCHITECTURES = {'arm64-v8a': ('arm64', 183), 'x86_64': ('x86_64', 62)}


def run(args):
    return subprocess.check_output([str(arg) for arg in args], text=True, stderr=subprocess.STDOUT)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def certificate_fingerprint(value):
    normalized = value.strip().replace(':', '').lower()
    if not re.fullmatch('[a-f0-9]{64}', normalized):
        raise RuntimeError('A complete expected production certificate SHA-256 is required')
    return normalized


def reject_test_attributes(tree):
    for line in tree.splitlines():
        if any(re.search(r'android:' + name + r'\(', line) for name in ('debuggable', 'testOnly')):
            boolean = re.search(r'=\(type\s+0x12\)(0x[0-9a-fA-F]+|[0-9]+)\b', line)
            if not boolean or int(boolean[1], 0) != 0:
                raise RuntimeError('Production APK must not be debuggable or testOnly')


def verify_apk(apk, sdk, version, identifier, version_code, expected_certificate):
    with zipfile.ZipFile(apk) as archive:
        payloads = [name for name in archive.namelist()
                    if re.fullmatch(r'lib/[^/]+/libminerdesk_mobile_lib\.so', name)]
        if len(payloads) != 1:
            raise RuntimeError('Expected exactly one native ABI in each production APK')
        abi = payloads[0].split('/')[1]
        if abi not in ARCHITECTURES:
            raise RuntimeError('Unexpected Android native ABI')
        with archive.open(payloads[0]) as stream:
            header = stream.read(64)
        if (len(header) != 64 or header[:4] != b'\x7fELF' or header[4:6] != b'\x02\x01'
                or struct.unpack_from('<H', header, 18)[0] != ARCHITECTURES[abi][1]):
            raise RuntimeError('Android native payload architecture differs from its ABI')
        if 'AndroidManifest.xml' not in archive.namelist() or 'classes.dex' not in archive.namelist():
            raise RuntimeError('Missing executable Android application contents')
    badging = run([sdk / 'aapt', 'dump', 'badging', apk])
    package = re.search(r"package: name='([^']+)' versionCode='([^']+)' versionName='([^']+)'", badging)
    if not package or package.groups() != (identifier, str(version_code), version):
        raise RuntimeError('APK identity, versionName or versionCode differs from the release contract')
    if 'application-debuggable' in badging or 'application-testOnly' in badging:
        raise RuntimeError('Production APK must not be debuggable or testOnly')
    if "uses-permission: name='android.permission.INTERNET'" not in badging:
        raise RuntimeError('Android release is missing Internet permission')
    if not re.search(r"^application(?:-label(?:-[^:]*)?)?:.*(?:icon=|label=)", badging, re.MULTILINE):
        raise RuntimeError('Android release is missing its application identity')
    tree = run([sdk / 'aapt', 'dump', 'xmltree', apk, 'AndroidManifest.xml'])
    reject_test_attributes(tree)
    certificate = run([sdk / 'apksigner', 'verify', '--verbose', '--print-certs', apk])
    fingerprints = re.findall(r'Signer #([0-9]+) certificate SHA-256 digest: ([a-fA-F0-9]+)', certificate)
    if len(fingerprints) != 1 or fingerprints[0][0] != '1':
        raise RuntimeError('Expected exactly one verified Android production signer')
    signer = certificate_fingerprint(fingerprints[0][1])
    if signer != expected_certificate or 'CN=Android Debug' in certificate:
        raise RuntimeError('Android signer does not match the persistent production certificate')
    if 'Verified using v2 scheme (APK Signature Scheme v2): true' not in certificate:
        raise RuntimeError('Android release requires a verified APK Signature Scheme v2 signature')
    run([sdk / 'zipalign', '-c', '-P', '16', '-v', '4', apk])
    return abi, signer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-commit', required=True)
    parser.add_argument('--expected-certificate-sha256',
                        default=os.environ.get('ANDROID_SIGNING_CERT_SHA256', ''))
    args = parser.parse_args()
    commit = run(['git', '-C', ROOT, 'rev-parse', 'HEAD']).strip()
    if not re.fullmatch('[0-9a-f]{40}', args.source_commit) or commit != args.source_commit:
        raise RuntimeError('Build provenance does not match this checkout')
    package = json.loads((MOBILE / 'package.json').read_text())
    config = json.loads((MOBILE / 'src-tauri/tauri.conf.json').read_text())
    version = package['version']
    if not re.fullmatch(r'[1-9]\d*\.\d+\.\d+', version) or config['version'] != version:
        raise RuntimeError('A matching stable mobile version is required')
    major, minor, patch = map(int, version.split('.'))
    version_code = config['bundle']['android'].get('versionCode', major * 1000000 + minor * 1000 + patch)
    if not isinstance(version_code, int) or not 1 <= version_code <= 2100000000:
        raise RuntimeError('Android versionCode must be a valid positive release integer')
    expected_certificate = certificate_fingerprint(args.expected_certificate_sha256)
    certificate_record = json.loads((MOBILE / 'signing-certificate.json').read_text())
    if certificate_record.get('identifier') != config['identifier']:
        raise RuntimeError('Pinned production certificate belongs to another application identifier')
    pinned_certificate = certificate_fingerprint(certificate_record.get('certificate_sha256', ''))
    if expected_certificate != pinned_certificate:
        raise RuntimeError('Expected signing certificate differs from the committed production identity')
    sdk = Path(os.environ['ANDROID_HOME']) / 'build-tools/36.0.0'
    source = MOBILE / 'src-tauri/gen/android/app/build/outputs/apk'
    out = ROOT / 'publish/mobile-android-release'
    if out.exists() and any(out.iterdir()):
        raise RuntimeError('Release output must be empty; refusing stale or overwritten artifacts')
    # Verify every candidate before any executable is staged for publication.
    verified = {}
    for apk in sorted(source.rglob('*.apk')):
        abi, signer = verify_apk(apk, sdk, version, config['identifier'], version_code, expected_certificate)
        if abi in verified:
            raise RuntimeError('Duplicated production Android ABI')
        verified[abi] = (apk, signer)
    if set(verified) != set(ARCHITECTURES):
        raise RuntimeError('Both arm64 and x86_64 production APKs are required')
    out.mkdir(parents=True, exist_ok=True)
    architectures = {}
    for abi, (apk, signer) in verified.items():
        name = f'MinerDesk-Mobile_{version}_android-{ARCHITECTURES[abi][0]}.apk'
        shutil.copyfile(apk, out / name)
        architectures[abi] = {'file': name, 'certificate_sha256': signer,
                              'sha256': digest(out / name), 'size_bytes': (out / name).stat().st_size}
    details = {'kind': 'Android production APK release', 'version': version,
               'version_code': version_code, 'identifier': config['identifier'],
               'source_commit': commit, 'preview_only': False,
               'signing': 'Persistent MinerDesk Mobile production certificate; APK v2 verified',
               'certificate_sha256': pinned_certificate,
               'architectures': architectures,
               'run_url': f'https://github.com/jontechlabs/MinerDesk/actions/runs/{os.environ.get("GITHUB_RUN_ID", "local")}',
               'distribution': 'Direct Android APK distribution; no app-store submission',
               'preview_migration': 'Uninstall the 0.1.0 debug preview once before installing this production-signed app.'}
    (out / 'BUILD-INFO-android.json').write_text(json.dumps(details, indent=2) + '\n')
    # Publish the reviewed public identity, never the keystore or its passwords.
    (out / 'SIGNING-CERTIFICATE-android.json').write_text(json.dumps(certificate_record, indent=2) + '\n')
    (out / 'INSTALL-ANDROID.txt').write_text(
        'MinerDesk Mobile — Android release\n\n'
        'Choose android-arm64 for ordinary modern Android phones. android-x86_64 is for Intel devices/emulators.\n'
        'Android 7.0 (API 24) or newer and a current Android System WebView are required.\n'
        'Download the APK from the official jontechlabs/MinerDesk release and open it to install.\n'
        'If Android asks, permit this installation for the app used to open the APK.\n\n'
        'ONE-TIME PREVIEW MIGRATION: the 0.1.0 test APK used an ephemeral debug certificate.\n'
        'Android cannot upgrade that certificate in place. Uninstall only MinerDesk Mobile preview,\n'
        'then install this release. Your rigs, miners and schedules remain on their computers.\n'
        'Re-enter your server URL and Web/LAN access key. The key remains session-only.\n'
        'Later releases use this persistent signing identity and can replace this release in place.\n\n'
        'Connect using your MinerDesk web/API URL, not a Fleet pairing invitation.\n'
        'For Internet access use valid HTTPS or a private VPN; the access key is required.\n'
        'This download does not install or run mining software on your phone.\n'
        'No Play Store or Apple App Store publication is included. See docs/MOBILE.md for setup.\n')
    brand = ROOT / 'docs/brand/mobile'
    if brand.is_dir():
        brand_archive = out / f'MinerDesk-Mobile_{version}_brand.zip'
        with zipfile.ZipFile(brand_archive, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(brand.rglob('*')):
                if path.is_file():
                    archive.write(path, Path('MinerDesk-Mobile-brand') / path.relative_to(brand))
            archive.write(ROOT / 'LICENSE', 'MinerDesk-Mobile-brand/LICENSE')
    (out / 'SHA256SUMS-android.txt').write_text(
        ''.join(f'{digest(path)}  {path.name}\n' for path in sorted(out.iterdir()) if path.is_file()))
    print(json.dumps(details, indent=2))


if __name__ == '__main__':
    main()
