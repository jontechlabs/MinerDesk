"""Exercise package removal only inside disposable root-owned fixture paths."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(hasattr(os, 'geteuid') and os.geteuid() == 0, 'requires disposable root-owned fixtures')
class Removal(unittest.TestCase):
    def fixture(self, base):
        script = (ROOT / 'src-tauri/linux-helper/postrm.sh').read_text()
        for original in ['/var/lib/minerdesk', '/usr/lib/minerdesk', '/etc/sudoers.d', '/usr/share/polkit-1/actions']:
            mapped = base / original.removeprefix('/')
            mapped.mkdir(parents=True, exist_ok=True)
            script = script.replace(original, str(mapped))
        runner = base / 'postrm.sh'
        runner.write_text(script)
        policies = base / 'etc/sudoers.d'
        for name in ['minerdesk-updates-1000', 'minerdesk-updates-1001', 'minerdesk-updates-1000-backup', 'another-app-1000']:
            (policies / name).write_text('disposable fixture only\n')
        updates = base / 'var/lib/minerdesk/updates'
        (updates / 'private').mkdir(parents=True)
        (updates / 'private/job.json').write_text('private fixture only')
        return runner, policies, updates

    def test_upgrade_preserves_grants_and_removal_cleans_only_owned_paths(self):
        for action in ['remove', 'purge']:
            with self.subTest(action=action), tempfile.TemporaryDirectory(prefix='minerdesk-postrm-fixture-') as directory:
                runner, policies, updates = self.fixture(Path(directory))
                subprocess.run(['sh', str(runner), 'upgrade', '0.8.3'], check=True)
                self.assertTrue((policies / 'minerdesk-updates-1000').exists())
                self.assertTrue((updates / 'private/job.json').exists())
                subprocess.run(['sh', str(runner), action], check=True)
                self.assertFalse((policies / 'minerdesk-updates-1000').exists())
                self.assertFalse((policies / 'minerdesk-updates-1001').exists())
                self.assertFalse(updates.exists())
                self.assertTrue((policies / 'minerdesk-updates-1000-backup').exists())
                self.assertTrue((policies / 'another-app-1000').exists())

    def test_redirected_storage_and_policy_are_never_followed(self):
        with tempfile.TemporaryDirectory(prefix='minerdesk-postrm-fixture-') as directory:
            base = Path(directory)
            runner, policies, updates = self.fixture(base)
            target = base / 'other-data'
            updates.rename(target)
            updates.symlink_to(target, target_is_directory=True)
            policy = policies / 'minerdesk-updates-1000'
            policy.unlink()
            policy.symlink_to(target / 'private/job.json')
            subprocess.run(['sh', str(runner), 'remove'], check=True)
            self.assertTrue((target / 'private/job.json').exists())
            self.assertTrue(policy.is_symlink())
            self.assertTrue(updates.is_symlink())


if __name__ == '__main__':
    unittest.main()
