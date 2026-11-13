"""Exercise the production preview packager with disposable APKs and SDK responses.

These files are synthetic, non-executable TEST DATA. Real APK signatures are
verified by the Android CI job; this suite checks orchestration and rejection.
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

SOURCE = Path(__file__).resolve().parents[1] / 'scripts/mobile-artifacts.py'
COMMIT = 'b' * 40
VERSION = '0.1.0'
IDENTIFIER = 'com.minerdesk.mobile'


class PreviewPackaging(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='minerdesk-mobile-package-test-')
        self.root = Path(self.directory.name).resolve()
        self.assertEqual(self.root.parent, Path(tempfile.gettempdir()).resolve())
        self.assertTrue(self.root.name.startswith('minerdesk-mobile-package-test-'))
        self.addCleanup(self.directory.cleanup)
        self.mobile = self.root / 'apps/mobile'
        self.apk_root = self.mobile / 'src-tauri/gen/android/app/build/outputs/apk'
        self.apk_root.mkdir(parents=True)
        (self.mobile / 'package.json').write_text(json.dumps({'version': VERSION}))
        (self.mobile / 'src-tauri/tauri.conf.json').write_text(json.dumps({'version': VERSION, 'identifier': IDENTIFIER}))
        self.apks = {}
        for abi, machine in [('arm64-v8a', 183), ('x86_64', 62)]:
            self.write_apk(abi, machine)
        spec = importlib.util.spec_from_file_location('mobile_packager_fixture', SOURCE)
        self.packager = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.packager)
        self.packager.ROOT, self.packager.MOBILE = self.root, self.mobile
        self.unsigned = False
        self.bad_version = False
        self.calls = []
        self.output = self.root / 'publish/mobile-android-preview'

    def write_apk(self, abi, machine):
        header = bytearray(64)
        header[:4] = b'\x7fELF'
        header[4:6] = b'\x02\x01'
        struct.pack_into('<H', header, 18, machine)
        apk = self.apk_root / f'fixture-{abi}-debug.apk'
        with zipfile.ZipFile(apk, 'w') as archive:
            archive.writestr('AndroidManifest.xml', 'TEST DATA: manifest is supplied by the fake SDK parser')
            archive.writestr(f'lib/{abi}/libminerdesk_mobile_lib.so', header)
        self.apks[abi] = apk

    def sdk_response(self, arguments):
        args = [str(argument) for argument in arguments]
        self.calls.append(args)
        tool = Path(args[0]).name
        if tool == 'git':
            self.assertEqual(args[1:], ['-C', str(self.root), 'rev-parse', 'HEAD'])
            return COMMIT + '\n'
        if tool == 'aapt':
            self.assertEqual(args[1:3], ['dump', 'badging'])
            self.assertIn(Path(args[-1]), self.apks.values())
            version = '9.9.9' if self.bad_version else VERSION
            return (f"package: name='{IDENTIFIER}' versionCode='1000' versionName='{version}'\n"
                    "uses-permission: name='android.permission.INTERNET'\napplication-debuggable\n")
        if tool == 'apksigner':
            self.assertEqual(args[1:4], ['verify', '--verbose', '--print-certs'])
            self.assertIn(Path(args[-1]), self.apks.values())
            if self.unsigned:
                raise subprocess.CalledProcessError(1, args, output='TEST DATA: unsigned APK rejected')
            return ('Signer #1 certificate DN: C=US, O=Android, CN=Android Debug\n'
                    'Signer #1 certificate SHA-256 digest: ' + 'a' * 64 + '\n')
        raise AssertionError(f'Unexpected external command: {tool}')

    def invoke_main(self, commit=COMMIT):
        with patch.object(sys, 'argv', ['mobile-artifacts.py', 'android', '--source-commit', commit]), \
                patch.object(self.packager, 'run', side_effect=self.sdk_response), \
                patch.dict(os.environ, {'ANDROID_HOME': str(self.root / 'fake-sdk'), 'GITHUB_RUN_ID': '12345'}), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            self.packager.main()
            return json.loads(output.getvalue())

    def test_main_packages_both_abis_and_executes_metadata_and_checksum_path(self):
        details = self.invoke_main()
        self.assertEqual(details['source_commit'], COMMIT)
        self.assertEqual(details['version'], VERSION)
        self.assertTrue(details['preview_only'])
        self.assertEqual(details['run_url'], 'https://github.com/jontechlabs/MinerDesk/actions/runs/12345')
        self.assertEqual(set(details['architectures']), {'arm64-v8a', 'x86_64'})
        info = json.loads((self.output / 'BUILD-INFO.json').read_text())
        self.assertEqual(info, details)
        expected = {'BUILD-INFO.json', 'README-PREVIEW.txt',
                    f'MinerDesk-Mobile_{VERSION}_android-arm64-preview.apk',
                    f'MinerDesk-Mobile_{VERSION}_android-x86_64-preview.apk'}
        sums = {}
        for line in (self.output / 'SHA256SUMS.txt').read_text().splitlines():
            checksum, name = line.split('  ', 1)
            sums[name] = checksum
            self.assertEqual(checksum, hashlib.sha256((self.output / name).read_bytes()).hexdigest())
        self.assertEqual(set(sums), expected)
        self.assertEqual({path.name for path in self.output.iterdir()}, expected | {'SHA256SUMS.txt'})
        for abi, record in details['architectures'].items():
            self.assertEqual((self.output / record['file']).read_bytes(), self.apks[abi].read_bytes())
        self.assertEqual(sum(Path(call[0]).name == 'apksigner' for call in self.calls), 2)

    def test_wrong_elf_machine_is_rejected_even_when_abi_directory_looks_correct(self):
        self.write_apk('arm64-v8a', 62)
        with self.assertRaisesRegex(RuntimeError, 'architecture'):
            self.invoke_main()
        self.assertFalse((self.output / 'BUILD-INFO.json').exists())

    def test_unsigned_apk_rejection_propagates_without_a_success_receipt(self):
        self.unsigned = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.invoke_main()
        self.assertFalse((self.output / 'BUILD-INFO.json').exists())
        self.assertFalse((self.output / 'SHA256SUMS.txt').exists())

    def test_wrong_apk_version_is_rejected(self):
        self.bad_version = True
        with self.assertRaisesRegex(RuntimeError, 'identity, version'):
            self.invoke_main()
        self.assertFalse((self.output / 'BUILD-INFO.json').exists())

    def test_wrong_source_commit_is_rejected_before_staging(self):
        with self.assertRaisesRegex(RuntimeError, 'provenance'):
            self.invoke_main('c' * 40)
        self.assertFalse(self.output.exists())

    def test_existing_preview_is_never_silently_overwritten(self):
        self.invoke_main()
        original = (self.output / 'BUILD-INFO.json').read_bytes()
        with self.assertRaisesRegex(RuntimeError, 'must be empty'):
            self.invoke_main()
        self.assertEqual((self.output / 'BUILD-INFO.json').read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
