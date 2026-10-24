"""Check source integrity, patch refusal and independent failure receipts without network."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('distribution_sources', Path(__file__).with_name('distribution-sources.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class SourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='distribution-sources-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'Cargo.toml').write_text('[workspace]\n')
        (self.root / 'Cargo.lock').write_text('locked source fixture\n')
        self.data = b'opaque original archive bytes; never extracted'
        self.license = b'original license fixture'
        license_hash = hashlib.sha256(self.license).hexdigest()
        (self.root / 'license.txt').write_bytes(self.license)
        self.package = {'id': 'cargo:covered@1.2.3', 'name': 'covered', 'version': '1.2.3',
                        'ecosystem': 'cargo', 'declaredLicense': 'MPL-2.0', 'textStatus': 'collected',
                        'origin': {'url': 'https://static.crates.io/crates/covered/covered-1.2.3.crate',
                                   'archiveSha256': hashlib.sha256(self.data).hexdigest()},
                        'licenseTexts': [{'file': 'license.txt', 'sha256': license_hash, 'sourcePath': 'LICENSE'}]}

    def build(self, download=None):
        return m.source_files(self.root, {'packages': [self.package]}, download or (lambda *_: self.data))

    def test_original_bytes_license_and_source_urls_are_retained(self):
        def download(url, name, digest):
            self.assertEqual(url, self.package['origin']['url'])
            self.assertEqual(name, 'covered-1.2.3.crate')
            self.assertEqual(digest, hashlib.sha256(self.data).hexdigest())
            return self.data
        files, report = self.build(download)
        self.assertEqual(files['sources/covered-1.2.3.crate'], self.data)
        self.assertEqual(files[report['sources'][0]['licenseTexts'][0]['path']], self.license)
        self.assertFalse(report['releaseApproved'])
        self.assertEqual(json.loads(files['manifest.json']), report)
        self.assertIn(self.package['origin']['url'].encode(), files['README.md'])
        npm = {**self.package, 'ecosystem': 'npm', 'id': 'npm:covered@1.2.3'}
        _, scoped = m.source_files(self.root, {'packages': [self.package, npm]}, download)
        self.assertEqual(len(scoped['sources']), 1)
        self.assertIn('npm', scoped['scope'])

    def test_changed_archive_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'archive checksum'):
            self.build(lambda *_: b'corrupted')

    def test_changed_license_is_rejected(self):
        (self.root / 'license.txt').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'license checksum'):
            self.build()

    def test_patched_alias_or_replacement_cannot_claim_original_sources(self):
        for text in ('[patch.crates-io]\nalias={package="covered",path="modified"}\n',
                     '[replace]\n"covered:1.2.3"={path="modified"}\n'):
            with self.subTest(text=text):
                (self.root / 'Cargo.toml').write_text(text)
                with self.assertRaisesRegex(ValueError, 'modified MPL'):
                    self.build(lambda *_: self.fail('patched sources must not be downloaded'))

    def test_incomplete_or_complex_license_does_not_silently_pass(self):
        self.package['textStatus'] = 'supplemented-notice-only'
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            self.build()
        self.package['textStatus'] = 'collected'
        self.package['declaredLicense'] = 'MPL-2.0 OR MIT'
        with self.assertRaisesRegex(ValueError, 'unsupported'):
            self.build()

    def test_foreign_source_url_and_duplicate_packages_are_rejected(self):
        self.package['origin']['url'] = 'https://example.invalid/archive'
        with self.assertRaisesRegex(ValueError, 'URL'):
            self.build()
        self.package['origin']['url'] = 'https://static.crates.io/crates/covered/covered-1.2.3.crate'
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            m.source_files(self.root, {'packages': [self.package, self.package]}, lambda *_: self.data)

    def test_failures_keep_independent_receipts_without_ready_archives(self):
        scripts = self.root / 'scripts'
        scripts.mkdir()
        (scripts / 'third-party.py').write_text('def validate(root):\n    raise ValueError("fixture failure")\n')
        with patch.object(m, 'ROOT', self.root):
            for _ in range(2):
                with self.assertRaisesRegex(ValueError, 'fixture failure'):
                    m.main()
        receipts = list((self.root / 'target/probes/distribution-sources').glob('*/summary.json'))
        self.assertEqual(len(receipts), 2)
        for path in receipts:
            value = json.loads(path.read_bytes())
            self.assertFalse(value['completed'])
            self.assertFalse(value['releaseApproved'])
            self.assertIn('fixture failure', value['failure'])
            self.assertFalse((path.parent / 'mpl-sources.zip').exists())


if __name__ == '__main__':
    unittest.main()
