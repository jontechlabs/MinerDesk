"""Validate the production privileged fixture selector; never execute a fixture."""
import copy
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / 'scripts/run-linux-helper-tests.py'
spec = importlib.util.spec_from_file_location('fixture_selector', SOURCE)
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


class SelectorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='minerdesk-helper-selector-')
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.crate = self.root / 'crate'
        self.target = self.root / 'target'
        self.crate.mkdir()
        self.target.mkdir()
        (self.crate / 'Cargo.toml').write_text('TEST DATA: fixture manifest')
        self.records = []
        for index, (name, (kind, source)) in enumerate(selector.EXPECTED.items()):
            source_path = self.crate / source
            source_path.parent.mkdir(parents=True, exist_ok=True)
            source_path.write_text('TEST DATA: fixture source')
            executable = self.target / 'debug/build/minerdesk-linux-helper' / (str(index) * 16) / 'out' / (name.replace('-', '_') + '-' + str(index) * 16)
            self.write_elf(executable)
            self.records.append({'reason': 'compiler-artifact', 'manifest_path': str(self.crate / 'Cargo.toml'),
                                 'target': {'name': name, 'kind': [kind], 'crate_types': ['bin'], 'src_path': str(source_path), 'test': True},
                                 'profile': {'test': True}, 'executable': str(executable), 'filenames': [str(executable)]})

    def write_elf(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        header = bytearray(64)
        header[:6] = b'\x7fELF\x02\x01'
        struct.pack_into('<H', header, 18, 62)
        path.write_bytes(header)
        path.chmod(0o755)

    def select(self, records=None):
        return selector.select_artifacts(self.records if records is None else records, self.crate, self.target)

    def test_new_layout_accepts_only_the_three_explicit_identities(self):
        selected = self.select()
        self.assertEqual(len(selected), 3)
        self.assertTrue(all('/debug/build/' in str(path) for path in selected))

    def test_old_layout_is_also_accepted(self):
        for record in self.records:
            path = self.target / 'debug/deps' / Path(record['executable']).name
            self.write_elf(path)
            record['executable'], record['filenames'] = str(path), [str(path)]
        self.assertEqual(len(self.select()), 3)

    def test_build_scripts_and_normal_bins_are_not_test_fixtures(self):
        normal = copy.deepcopy(self.records[0])
        normal['profile']['test'] = False
        build = copy.deepcopy(self.records[0])
        build['target']['kind'] = ['custom-build']
        build['target']['name'] = 'build-script-build'
        self.assertEqual(len(self.select(self.records + [normal, build])), 3)

    def test_dependency_lookalike_cannot_replace_an_expected_fixture(self):
        foreign = self.root / 'dependency/Cargo.toml'
        foreign.parent.mkdir()
        foreign.write_text('TEST DATA: dependency')
        self.records[0]['manifest_path'] = str(foreign)
        with self.assertRaisesRegex(ValueError, 'exactly the three'):
            self.select()

    def test_wrong_kind_or_source_is_rejected(self):
        original = copy.deepcopy(self.records)
        for field, value in [('kind', ['test']), ('src_path', str(self.crate / 'Cargo.toml'))]:
            self.records = copy.deepcopy(original)
            self.records[0]['target'][field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'kind|source'):
                self.select()

    def test_unknown_or_duplicate_targets_are_rejected(self):
        with self.assertRaisesRegex(ValueError, 'Duplicated'):
            self.select(self.records + [copy.deepcopy(self.records[0])])
        self.records[0]['target']['name'] = 'unreviewed-helper-test'
        with self.assertRaisesRegex(ValueError, 'Unknown'):
            self.select()

    def test_external_executable_is_rejected(self):
        path = self.root / Path(self.records[0]['executable']).name
        self.write_elf(path)
        self.records[0]['executable'], self.records[0]['filenames'] = str(path), [str(path)]
        with self.assertRaisesRegex(ValueError, 'escaped'):
            self.select()

    def test_symlink_file_and_parent_are_rejected(self):
        original = Path(self.records[0]['executable'])
        link = self.target / original.name
        link.symlink_to(original)
        self.records[0]['executable'], self.records[0]['filenames'] = str(link), [str(link)]
        with self.assertRaisesRegex(ValueError, 'Symlinks'):
            self.select()
        link.unlink()
        parent = self.target / 'alias'
        parent.symlink_to(original.parent, target_is_directory=True)
        path = parent / original.name
        self.records[0]['executable'], self.records[0]['filenames'] = str(path), [str(path)]
        with self.assertRaisesRegex(ValueError, 'Symlinks'):
            self.select()

    def test_missing_receipt_or_non_elf_payload_is_rejected(self):
        self.records[0]['filenames'] = []
        with self.assertRaisesRegex(ValueError, 'receipt'):
            self.select()
        self.records[0]['filenames'] = [self.records[0]['executable']]
        Path(self.records[0]['executable']).write_text('#!/bin/sh\necho not a Rust fixture\n')
        with self.assertRaisesRegex(ValueError, 'ELF'):
            self.select()


if __name__ == '__main__':
    unittest.main()
