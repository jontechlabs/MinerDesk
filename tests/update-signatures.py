"""Exercise real Tauri signing and production release verification with throwaway keys."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
root = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release', root/'scripts/package-release.py')
release = importlib.util.module_from_spec(spec); spec.loader.exec_module(release)
cli = ['node',str(root/'node_modules/@tauri-apps/cli/tauri.js'),'signer']

class Signatures(unittest.TestCase):
    def test_valid_signature_tamper_and_wrong_publisher(self):
        with tempfile.TemporaryDirectory(prefix='minerdesk-test-key-') as directory:
            base = Path(directory)
            key = base/'test.key'
            environment = {**os.environ,'TAURI_SIGNING_PRIVATE_KEY_PATH':str(key),'TAURI_SIGNING_PRIVATE_KEY_PASSWORD':'throwaway-test-only'}
            result = subprocess.run(cli+['generate','--ci','-p','throwaway-test-only','-w',str(key)],capture_output=True)
            self.assertEqual(result.returncode,0,'Test key generation failed')
            package = base/'test.AppImage'; package.write_bytes(b'Test bytes only; never executed.\n')
            result = subprocess.run(cli+['sign',str(package)],env=environment,capture_output=True)
            self.assertEqual(result.returncode,0,'Test signing failed')
            pubkey = (base/'test.key.pub').read_text().strip()
            release.verify_update(package,pubkey)
            package.write_bytes(package.read_bytes()+b'tampered')
            with self.assertRaises(RuntimeError): release.verify_update(package,pubkey)
            package.write_bytes(b'Test bytes only; never executed.\n')
            other = base/'other.key'
            result = subprocess.run(cli+['generate','--ci','-p','throwaway-test-only','-w',str(other)],capture_output=True)
            self.assertEqual(result.returncode,0)
            with self.assertRaises(RuntimeError): release.verify_update(package,(base/'other.key.pub').read_text().strip())
            package.with_name(package.name+'.sig').write_text('invalid-base64')
            with self.assertRaises(Exception): release.verify_update(package,pubkey)

if __name__ == '__main__': unittest.main()
