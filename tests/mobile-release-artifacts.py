"""Exercise the real release packager with synthetic APKs and SDK responses.

The fixtures are not executable apps. Actual signatures are independently
verified with the Android SDK in the protected production-build workflow.
"""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SOURCE = Path(__file__).resolve().parents[1] / 'scripts/mobile-release-artifacts.py'
COMMIT = 'b' * 40
VERSION = '1.0.0'
VERSION_CODE = 1000000
IDENTIFIER = 'com.minerdesk.mobile'
CERTIFICATE = 'a' * 64


class ReleasePackaging(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='minerdesk-mobile-release-test-')
        self.root = Path(self.directory.name).resolve()
        self.assertEqual(self.root.parent, Path(tempfile.gettempdir()).resolve())
        self.addCleanup(self.directory.cleanup)
        self.mobile = self.root / 'apps/mobile'
        self.apk_root = self.mobile / 'src-tauri/gen/android/app/build/outputs/apk'
        self.apk_root.mkdir(parents=True)
        (self.mobile / 'package.json').write_text(json.dumps({'version': VERSION}))
        self.config_path = self.mobile / 'src-tauri/tauri.conf.json'
        self.config_path.write_text(json.dumps({'version': VERSION, 'identifier': IDENTIFIER,
                                               'bundle': {'android': {'versionCode': VERSION_CODE}}}))
        self.certificate_path = self.mobile / 'signing-certificate.json'
        self.certificate_path.write_text(json.dumps({'identifier': IDENTIFIER,
                                                     'certificate_sha256': CERTIFICATE}))
        (self.root / 'LICENSE').write_text('TEST DATA license')
        brand = self.root / 'docs/brand/mobile'
        brand.mkdir(parents=True)
        (brand / 'mark.svg').write_text('<svg><!-- TEST DATA --></svg>')
        self.apks = {}
        for abi, machine in [('arm64-v8a', 183), ('x86_64', 62)]:
            self.write_apk(abi, machine)
        spec = importlib.util.spec_from_file_location('release_packager_fixture', SOURCE)
        self.packager = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.packager)
        self.packager.ROOT, self.packager.MOBILE = self.root, self.mobile
        self.output = self.root / 'publish/mobile-android-release'
        self.calls = []
        self.debuggable = False
        self.test_only = False
        self.debug_certificate = False
        self.unsigned = False
        self.no_v2 = False
        self.signer = CERTIFICATE
        self.signer_count = 1
        self.version_code = VERSION_CODE
        self.internet = True

    def write_apk(self, abi, machine, suffix='release', include_dex=True):
        header = bytearray(64)
        header[:4] = b'\x7fELF'
        header[4:6] = b'\x02\x01'
        struct.pack_into('<H', header, 18, machine)
        apk = self.apk_root / f'fixture-{abi}-{suffix}.apk'
        with zipfile.ZipFile(apk, 'w') as archive:
            archive.writestr('AndroidManifest.xml', 'TEST DATA: fake SDK supplies parsed manifest')
            if include_dex:
                archive.writestr('classes.dex', 'TEST DATA: not executable')
            archive.writestr(f'lib/{abi}/libminerdesk_mobile_lib.so', header)
        self.apks[abi] = apk
        return apk

    def sdk_response(self, arguments):
        args = [str(argument) for argument in arguments]
        self.calls.append(args)
        tool = Path(args[0]).name
        if tool == 'git':
            self.assertEqual(args[1:], ['-C', str(self.root), 'rev-parse', 'HEAD'])
            return COMMIT + '\n'
        if tool == 'aapt':
            self.assertEqual(args[1], 'dump')
            if args[2] == 'badging':
                self.assertTrue(Path(args[-1]).is_file())
                result = f"package: name='{IDENTIFIER}' versionCode='{self.version_code}' versionName='{VERSION}'\n"
                if self.internet:
                    result += "uses-permission: name='android.permission.INTERNET'\n"
                result += "application: label='MinerDesk Mobile' icon='res/mipmap-mdpi-v4/ic_launcher.png'\n"
                if self.debuggable:
                    result += 'application-debuggable\n'
                return result
            self.assertEqual(args[2], 'xmltree')
            self.assertEqual(args[-1], 'AndroidManifest.xml')
            return ('E: manifest\n  E: application\n'
                    f'    A: android:debuggable(0x0101000f)=(type 0x12)0x{0xffffffff if self.debuggable else 0:x}\n'
                    f'    A: android:testOnly(0x01010272)=(type 0x12)0x{0xffffffff if self.test_only else 0:x}\n')
        if tool == 'apksigner':
            self.assertEqual(args[1:4], ['verify', '--verbose', '--print-certs'])
            if self.unsigned:
                raise subprocess.CalledProcessError(1, args, output='TEST DATA unsigned APK rejected')
            dn = 'Android Debug' if self.debug_certificate else 'MinerDesk Mobile'
            result = f'Signer #1 certificate DN: CN={dn}\n'
            result += f'Signer #1 certificate SHA-256 digest: {self.signer}\n'
            result += 'Verified using v2 scheme (APK Signature Scheme v2): ' + ('false' if self.no_v2 else 'true') + '\n'
            if self.signer_count != 1:
                result += f'Signer #2 certificate SHA-256 digest: {CERTIFICATE}\n'
            return result
        if tool == 'zipalign':
            self.assertEqual(args[1:6], ['-c', '-P', '16', '-v', '4'])
            return 'Verification successful\n'
        raise AssertionError(f'Unexpected external command: {tool}')

    def invoke(self, commit=COMMIT, expected_certificate=CERTIFICATE):
        with patch.object(sys, 'argv', ['mobile-release-artifacts.py', '--source-commit', commit,
                                       '--expected-certificate-sha256', expected_certificate]), \
                patch.object(self.packager, 'run', side_effect=self.sdk_response), \
                patch.dict(os.environ, {'ANDROID_HOME': str(self.root / 'fake-sdk'), 'GITHUB_RUN_ID': '12345'}), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.packager.main()
            return json.loads(output.getvalue())

    def assert_not_staged(self):
        self.assertFalse(self.output.exists())

    def test_release_has_two_persistent_signatures_versions_brand_and_exact_checksums(self):
        details = self.invoke()
        self.assertFalse(details['preview_only'])
        self.assertEqual(details['version_code'], VERSION_CODE)
        self.assertEqual(details['certificate_sha256'], CERTIFICATE)
        self.assertEqual(details['source_commit'], COMMIT)
        self.assertEqual(details['run_url'], 'https://github.com/jontechlabs/MinerDesk/actions/runs/12345')
        self.assertEqual(set(details['architectures']), {'arm64-v8a', 'x86_64'})
        for abi, record in details['architectures'].items():
            file = self.output / record['file']
            self.assertEqual(file.read_bytes(), self.apks[abi].read_bytes())
            self.assertEqual(record['certificate_sha256'], CERTIFICATE)
            self.assertEqual(record['sha256'], hashlib.sha256(file.read_bytes()).hexdigest())
        checksums = {}
        for line in (self.output / 'SHA256SUMS-android.txt').read_text().splitlines():
            checksum, name = line.split('  ', 1)
            checksums[name] = checksum
            self.assertEqual(checksum, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        self.assertEqual(set(checksums), {path.name for path in self.output.iterdir()} - {'SHA256SUMS-android.txt'})
        with zipfile.ZipFile(self.output / f'MinerDesk-Mobile_{VERSION}_brand.zip') as archive:
            self.assertEqual(set(archive.namelist()), {'MinerDesk-Mobile-brand/mark.svg', 'MinerDesk-Mobile-brand/LICENSE'})
        self.assertIn('Uninstall only MinerDesk Mobile preview', (self.output / 'INSTALL-ANDROID.txt').read_text())
        self.assertEqual(json.loads((self.output / 'SIGNING-CERTIFICATE-android.json').read_text()),
                         json.loads(self.certificate_path.read_text()))
        self.assertEqual(sum(Path(call[0]).name == 'zipalign' for call in self.calls), 2)

    def test_wrong_native_architecture_rejected_before_staging(self):
        self.write_apk('arm64-v8a', 62)
        with self.assertRaisesRegex(RuntimeError, 'architecture'):
            self.invoke()
        self.assert_not_staged()

    def test_missing_executable_content_rejected_before_staging(self):
        self.write_apk('arm64-v8a', 183, include_dex=False)
        with self.assertRaisesRegex(RuntimeError, 'application contents'):
            self.invoke()
        self.assert_not_staged()

    def test_debuggable_release_rejected_before_staging(self):
        self.debuggable = True
        with self.assertRaisesRegex(RuntimeError, 'debuggable or testOnly'):
            self.invoke()
        self.assert_not_staged()

    def test_test_only_release_rejected_before_staging(self):
        self.test_only = True
        with self.assertRaisesRegex(RuntimeError, 'debuggable or testOnly'):
            self.invoke()
        self.assert_not_staged()

    def test_wrong_persistent_certificate_rejected_before_staging(self):
        self.signer = 'c' * 64
        with self.assertRaisesRegex(RuntimeError, 'persistent production certificate'):
            self.invoke()
        self.assert_not_staged()

    def test_debug_certificate_rejected_even_if_fingerprint_was_configured(self):
        self.debug_certificate = True
        with self.assertRaisesRegex(RuntimeError, 'persistent production certificate'):
            self.invoke()
        self.assert_not_staged()

    def test_unsigned_apk_does_not_get_a_success_receipt(self):
        self.unsigned = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.invoke()
        self.assert_not_staged()

    def test_v1_only_apk_rejected(self):
        self.no_v2 = True
        with self.assertRaisesRegex(RuntimeError, 'Signature Scheme v2'):
            self.invoke()
        self.assert_not_staged()

    def test_multiple_signers_rejected(self):
        self.signer_count = 2
        with self.assertRaisesRegex(RuntimeError, 'exactly one verified'):
            self.invoke()
        self.assert_not_staged()

    def test_wrong_version_code_rejected(self):
        self.version_code = 1000
        with self.assertRaisesRegex(RuntimeError, 'versionCode'):
            self.invoke()
        self.assert_not_staged()

    def test_missing_internet_permission_rejected(self):
        self.internet = False
        with self.assertRaisesRegex(RuntimeError, 'Internet permission'):
            self.invoke()
        self.assert_not_staged()

    def test_duplicate_abi_rejected(self):
        self.write_apk('arm64-v8a', 183, suffix='duplicate')
        with self.assertRaisesRegex(RuntimeError, 'Duplicated'):
            self.invoke()
        self.assert_not_staged()

    def test_incomplete_abi_set_rejected(self):
        self.apks['x86_64'].unlink()
        with self.assertRaisesRegex(RuntimeError, 'Both arm64 and x86_64'):
            self.invoke()
        self.assert_not_staged()

    def test_missing_expected_fingerprint_is_not_optional(self):
        with self.assertRaisesRegex(RuntimeError, 'complete expected production certificate'):
            self.invoke(expected_certificate='')
        self.assert_not_staged()

    def test_changed_secret_and_matching_new_apks_cannot_silently_rotate_the_pinned_certificate(self):
        self.signer = 'c' * 64
        with self.assertRaisesRegex(RuntimeError, 'committed production identity'):
            self.invoke(expected_certificate='c' * 64)
        self.assert_not_staged()

    def test_public_certificate_for_another_package_is_rejected(self):
        self.certificate_path.write_text(json.dumps({'identifier': 'com.example.other',
                                                     'certificate_sha256': CERTIFICATE}))
        with self.assertRaisesRegex(RuntimeError, 'another application identifier'):
            self.invoke()
        self.assert_not_staged()

    def test_missing_public_certificate_pin_is_not_optional(self):
        self.certificate_path.unlink()
        with self.assertRaises(FileNotFoundError):
            self.invoke()
        self.assert_not_staged()

    def test_wrong_source_commit_rejected_before_staging(self):
        with self.assertRaisesRegex(RuntimeError, 'provenance'):
            self.invoke(commit='c' * 40)
        self.assert_not_staged()

    def test_existing_release_is_never_silently_overwritten(self):
        self.invoke()
        original = (self.output / 'BUILD-INFO-android.json').read_bytes()
        with self.assertRaisesRegex(RuntimeError, 'must be empty'):
            self.invoke()
        self.assertEqual(original, (self.output / 'BUILD-INFO-android.json').read_bytes())


if __name__ == '__main__':
    unittest.main()
