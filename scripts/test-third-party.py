#!/usr/bin/env python3
"""Behavioral checks for attribution collection without network or build artifacts."""
import base64
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('third_party', Path(__file__).with_name('third-party.py'))
if SPEC is None or SPEC.loader is None:
    raise RuntimeError('cannot load attribution generator for tests')
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


def archive(entries):
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode='w:gz') as tar:
        for name, data, link in entries:
            entry = tarfile.TarInfo(name)
            if link:
                entry.type = tarfile.SYMTYPE
                entry.linkname = link
                tar.addfile(entry)
            else:
                entry.size = len(data)
                tar.addfile(entry, io.BytesIO(data))
    return out.getvalue()


class AttributionTests(unittest.TestCase):
    def setUp(self):
        Path('/data/temp').mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(prefix='third-party-tests-', dir='/data/temp')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for name in m.INPUTS:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'{}\n')
        (self.root / m.supplemental.SOURCE_FILE).write_text(json.dumps({'schemaVersion': 1, 'parents': {}, 'bindings': {}}))
        (self.root / 'Cargo.toml').write_text('[workspace]\nmembers = ["crates/first"]\n[workspace.package]\nversion = "0.1.0"\n')
        member = self.root / 'crates/first/Cargo.toml'
        member.parent.mkdir(parents=True)
        member.write_text('[package]\nname = "first"\nversion.workspace = true\n')
        (self.root / 'Cargo.lock').write_text('version = 4\n[[package]]\nname = "first"\nversion = "0.1.0"\n[[package]]\nname = "dep"\nversion = "1.2.3"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "' + 'ab' * 32 + '"\n')
        self.integrity = 'sha512-' + base64.b64encode(bytes(64)).decode()
        (self.root / 'pnpm-lock.yaml').write_text("lockfileVersion: '9.0'\n\npackages:\n\n  '@scope/tool@1.2.3':\n    resolution: {integrity: " + self.integrity + "}\n    os: [darwin]\n\nsnapshots:\n  '@scope/tool@1.2.3': {}\n")
        font = self.root / 'ui/src/assets/fonts/jetbrains-mono/source.json'
        font.write_text(json.dumps({'version': '2.304'}))
        self.blob = b'Original upstream license text.\n'
        self.files = {f'third-party/licenses/{m.sha(self.blob)}.txt': self.blob}
        rust = {'name': 'dep', 'version': '1.2.3', 'license': 'MIT'}
        npm = {'name': '@scope/tool', 'version': '1.2.3', 'license': 'MIT'}
        asset = {'name': 'JetBrains Mono', 'version': '2.304', 'license': 'OFL-1.1'}
        self.rows = [m.package_record('cargo', rust, rust, {'LICENSE': self.blob}, {'archiveSha256': 'ab' * 32}),
                     m.package_record('npm', npm, npm, {'LICENSE': self.blob}, {'integrity': self.integrity}),
                     m.package_record('asset', asset, asset, {'OFL.txt': self.blob}, {})]

    def generate(self):
        with patch.object(m, 'collect', return_value=(self.rows, dict(self.files))):
            return m.generate(self.root, self.root)

    def test_workspace_and_optional_package_scope(self):
        rust, npm = m.lock_packages(self.root)
        self.assertEqual([p['name'] for p in rust], ['dep'])
        self.assertEqual([p['name'] for p in npm], ['@scope/tool'])
        self.assertEqual(npm[0]['integrity'], self.integrity)
        (self.root / 'pnpm-lock.yaml').write_text("lockfileVersion: '9.0'\npackages:\n  \"unsupported@1.0\":\n    resolution: {integrity: x}\n")
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            m.lock_packages(self.root)

    def test_download_retries_bounded_without_retrying_integrity_or_permanent_http_errors(self):
        path = self.root / 'network.tgz'
        data = b'original archive'
        error = m.urllib.error.URLError('temporary EOF')
        with patch.object(m.time, 'sleep'), patch.object(m.urllib.request, 'urlopen', side_effect=[error, io.BytesIO(data)]) as fetch:
            self.assertEqual(m.archive_bytes('https://unused.invalid', path, m.sha(data), 'sha256'), data)
            self.assertEqual(fetch.call_count, 2)
        path.unlink()
        with patch.object(m.time, 'sleep'), patch.object(m.urllib.request, 'urlopen', side_effect=error) as fetch:
            with self.assertRaisesRegex(RuntimeError, 'after three attempts'):
                m.archive_bytes('https://unused.invalid', path, m.sha(data), 'sha256')
            self.assertEqual(fetch.call_count, 3)
            self.assertFalse(path.exists())
        permanent = m.urllib.error.HTTPError('https://unused.invalid', 404, 'missing', None, None)
        with patch.object(m.urllib.request, 'urlopen', side_effect=permanent) as fetch:
            with self.assertRaises(m.urllib.error.HTTPError):
                m.archive_bytes('https://unused.invalid', path, m.sha(data), 'sha256')
            self.assertEqual(fetch.call_count, 1)
        with patch.object(m.urllib.request, 'urlopen', return_value=io.BytesIO(data)) as fetch:
            with self.assertRaisesRegex(ValueError, 'integrity mismatch'):
                m.archive_bytes('https://unused.invalid', path, '00' * 32, 'sha256')
            self.assertEqual(fetch.call_count, 1)

    def test_archive_integrity_checked_before_use(self):
        path = self.root / 'archive.tgz'
        path.write_bytes(b'locked archive')
        self.assertEqual(m.archive_bytes('https://unused.invalid', path, m.sha(path.read_bytes()), 'sha256'), path.read_bytes())
        with self.assertRaisesRegex(ValueError, 'integrity mismatch'):
            m.archive_bytes('https://unused.invalid', path, '00' * 32, 'sha256')

    def test_archive_missing_streams_are_rejected(self):
        manifest = b'{"name":"dep","version":"1.2.3","license":"MIT"}'
        data = archive([('package/package.json', manifest, None), ('package/LICENSE', self.blob, None)])
        with patch.object(m.tarfile.TarFile, 'extractfile', return_value=None):
            with self.assertRaisesRegex(ValueError, 'missing package manifest stream'):
                m.read_archive(data, 'package', 'package.json')
        with patch.object(m.tarfile.TarFile, 'extractfile', side_effect=[io.BytesIO(manifest), None]):
            with self.assertRaisesRegex(ValueError, 'missing attribution stream'):
                m.read_archive(data, 'package', 'package.json')

    def test_archive_root_and_verbatim_license(self):
        manifest = b'{"name":"@types/example","version":"1.2.3","license":"MIT"}'
        data = archive([('example/package.json', manifest, None), ('example/LICENSE', self.blob, None)])
        raw, texts = m.read_archive(data, None, 'package.json')
        self.assertEqual(raw, manifest)
        self.assertEqual(texts, {'LICENSE': self.blob})
        with self.assertRaisesRegex(ValueError, 'identity mismatch'):
            m.package_record('npm', {'name': 'different', 'version': '1.2.3'}, json.loads(raw), texts, {})

    def test_archive_traversal_links_duplicates_and_roots_rejected(self):
        manifest = ('package/package.json', b'{}', None)
        for entry in [('package/../LICENSE', self.blob, None), ('/package/LICENSE', self.blob, None),
                      ('package/LICENSE', b'', '../../secret'), ('other/LICENSE', self.blob, None)]:
            with self.subTest(entry=entry[0]), self.assertRaises(ValueError):
                m.read_archive(archive([manifest, entry]), None, 'package.json')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            m.read_archive(archive([manifest, ('package/LICENSE', self.blob, None), ('package/LICENSE', self.blob, None)]), None, 'package.json')

    def test_missing_text_not_silently_synthesized_or_approved(self):
        package = {'name': 'dep', 'version': '1.2.3', 'license': 'MIT'}
        row = m.package_record('cargo', package, package, {}, {})
        self.assertEqual(row['textStatus'], 'missing-from-published-package')
        self.assertEqual(row['licenseTexts'], [])
        text = m.documents({'packages': [row]})['THIRD_PARTY.md']
        self.assertIn(b'Missing from archive', text)
        with self.assertRaisesRegex(ValueError, '1 packages need'):
            m.require_texts({'packages': [row]})
        self.assertIsNone(m.require_texts({'packages': self.rows}))
        self.generate()
        report = json.loads((self.root / 'third-party/inventory.json').read_text())
        report['releaseApproved'] = True
        (self.root / 'third-party/inventory.json').write_bytes(m.encoded(report))
        with self.assertRaisesRegex(ValueError, 'false release approval'):
            m.validate(self.root)

    def test_generation_deterministic_refresh_and_stale_inputs(self):
        first = self.generate()
        original = (self.root / 'third-party/inventory.json').read_bytes()
        self.generate()
        self.assertEqual(original, (self.root / 'third-party/inventory.json').read_bytes())
        (self.root / 'package.json').write_bytes(b'{"changed":true}\n')
        with self.assertRaisesRegex(ValueError, 'inputs changed'):
            m.validate(self.root)
        refreshed = self.generate()
        self.assertNotEqual(first['inputs'], refreshed['inputs'])
        self.assertFalse(refreshed['releaseApproved'])

    def test_modified_and_foreign_files_cannot_be_overwritten(self):
        self.generate()
        license_path = self.root / next(iter(self.files))
        license_path.write_bytes(b'edited by user')
        with self.assertRaisesRegex(ValueError, 'bytes changed'):
            self.generate()
        self.assertEqual(license_path.read_bytes(), b'edited by user')
        license_path.write_bytes(self.blob)
        unknown = self.root / 'third-party/licenses/user.txt'
        unknown.write_bytes(b'user data')
        with self.assertRaisesRegex(ValueError, 'unexpected'):
            self.generate()
        self.assertEqual(unknown.read_bytes(), b'user data')

    def test_lock_identity_integrity_and_document_drift(self):
        self.generate()
        inventory = self.root / 'third-party/inventory.json'
        raw = inventory.read_bytes()
        report = json.loads(raw)
        report['packages'][1]['origin']['integrity'] = 'sha512-wrong'
        inventory.write_bytes(m.encoded(report))
        with self.assertRaisesRegex(ValueError, 'differs from lock'):
            m.validate(self.root)
        inventory.write_bytes(raw)
        (self.root / 'NOTICE').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'generated file changed'):
            m.validate(self.root)


class SupplementalTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='attribution-bindings-', dir='/data/temp')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'third-party').mkdir()
        self.blob = b'Copyright Example\nPermission to distribute this original example.\n'
        self.sha = m.sha(self.blob)
        self.files = {f'third-party/licenses/{self.sha}.txt': self.blob}

    def optional(self):
        manifest = {'name': 'parent', 'version': '1.2.3', 'license': 'MIT',
                    'repository': 'https://github.com/example/project',
                    'optionalDependencies': {'platform': '1.2.3'}}
        raw = m.encoded(manifest)
        parent = m.package_record('npm', {'name': 'parent', 'version': '1.2.3'}, manifest,
                                  {'LICENSE': self.blob}, {'integrity': 'parent-integrity', 'manifestSha256': m.sha(raw)})
        child = m.package_record('npm', {'name': 'platform', 'version': '1.2.3'},
                                 {**manifest, 'name': 'platform'}, {}, {'integrity': 'child-integrity'})
        spec = {'kind': 'npm-optional-parent', 'parent': parent['id'], 'integrity': 'child-integrity'}
        plan = {'schemaVersion': 1, 'parents': {parent['id']: {'integrity': 'parent-integrity',
                'manifestSha256': m.sha(raw), 'manifestBase64': base64.b64encode(raw).decode()}},
                'bindings': {child['id']: spec}}
        return [parent, child], plan

    def workspace_parent(self):
        packages, plan = self.optional()
        parent, child = packages
        parent.update(id='cargo:parent@1.2.3', ecosystem='cargo', repository=None)
        child.update(id='cargo:platform@1.2.3', ecosystem='cargo', repository=None)
        parent_vcs = {'git': {'sha1': 'a' * 40}, 'path_in_vcs': ''}
        child_vcs = {'git': {'sha1': 'a' * 40}, 'path_in_vcs': 'sys'}
        raw = b'[package]\nname = "parent"\nversion = "1.2.3"\nlicense = "MIT"\n[dependencies]\nplatform = { version = "1.2.3", path = "sys/" }\n'
        proof = {'archiveSha256': 'a1' * 32, 'manifestBase64': base64.b64encode(raw).decode(),
                 'manifestSha256': m.sha(raw), 'vcs': parent_vcs, 'vcsSha256': m.sha(m.encoded(parent_vcs))}
        parent['origin'] = {'archiveSha256': proof['archiveSha256'], 'manifestSha256': m.sha(raw),
                            'vcsEvidence': {'sha256': proof['vcsSha256'], 'value': parent_vcs}}
        spec = {'kind': 'cargo-workspace-parent', 'parent': parent['id'], 'archiveSha256': 'b2' * 32,
                'vcs': child_vcs, 'vcsSha256': m.sha(m.encoded(child_vcs))}
        child['origin'] = {'archiveSha256': spec['archiveSha256'],
                           'vcsEvidence': {'sha256': spec['vcsSha256'], 'value': child_vcs}}
        plan['parents'] = {parent['id']: proof}
        plan['bindings'] = {child['id']: spec}
        return packages, plan

    def update_workspace_manifest(self, packages, plan, old, new):
        parent = packages[0]
        proof = plan['parents'][parent['id']]
        raw = base64.b64decode(proof['manifestBase64']).replace(old, new)
        proof.update(manifestBase64=base64.b64encode(raw).decode(), manifestSha256=m.sha(raw))
        parent['origin']['manifestSha256'] = m.sha(raw)

    def test_workspace_parent_reuses_published_root_without_network(self):
        packages, plan = self.workspace_parent()
        with patch.object(m.urllib.request, 'urlopen', side_effect=AssertionError('must reuse archive bytes')):
            self.apply(packages, plan)
        m.supplemental.validate(packages, plan)
        self.assertIsNone(m.require_texts({'packages': packages}))
        self.assertEqual(packages[1]['licenseTexts'][0]['sourcePackage'], packages[0]['id'])
        self.assertIn(b'Locked workspace parent', m.documents({'packages': packages})['THIRD_PARTY.md'])

    def test_workspace_parent_dirty_or_different_commit_rejects(self):
        for target in ('parent-dirty', 'child-dirty', 'different-commit'):
            with self.subTest(target=target):
                packages, plan = self.workspace_parent()
                proof = plan['parents'][packages[0]['id']]
                vcs = proof['vcs'] if target != 'child-dirty' else plan['bindings'][packages[1]['id']]['vcs']
                vcs['git']['dirty'] = True if target != 'different-commit' else False
                if target == 'different-commit':
                    vcs['git']['sha1'] = 'b' * 40
                with self.assertRaises(ValueError):
                    self.apply(packages, plan)

    def test_workspace_parent_archive_and_manifest_tampering_rejects(self):
        for target in ('parent-archive', 'child-archive', 'manifest', 'vcs-proof'):
            with self.subTest(target=target):
                packages, plan = self.workspace_parent()
                if target.endswith('archive'):
                    packages[0 if target == 'parent-archive' else 1]['origin']['archiveSha256'] = '00' * 32
                elif target == 'manifest':
                    plan['parents'][packages[0]['id']]['manifestBase64'] = base64.b64encode(b'[package]').decode()
                else:
                    packages[0]['origin']['vcsEvidence']['sha256'] = '00' * 32
                with self.assertRaises(ValueError):
                    self.apply(packages, plan)

    def test_workspace_parent_identity_license_and_dependency_version_rejects(self):
        cases = [(b'name = "parent"', b'name = "foreign"'),
                 (b'version = "1.2.3"\nlicense', b'version = "9.9.9"\nlicense'),
                 (b'license = "MIT"', b'license = "Apache-2.0"'),
                 (b'platform = { version = "1.2.3"', b'platform = { version = "^1.2"')]
        for old, new in cases:
            with self.subTest(change=new):
                packages, plan = self.workspace_parent()
                self.update_workspace_manifest(packages, plan, old, new)
                with self.assertRaises(ValueError):
                    self.apply(packages, plan)

    def test_workspace_parent_zero_patch_release_constraint_is_explicit(self):
        packages, plan = self.workspace_parent()
        packages[1].update(id='cargo:platform@1.2.0', version='1.2.0')
        spec = plan['bindings'].pop('cargo:platform@1.2.3')
        plan['bindings'][packages[1]['id']] = spec
        self.update_workspace_manifest(packages, plan, b'platform = { version = "1.2.3"', b'platform = { version = "1.2"')
        self.apply(packages, plan)
        m.supplemental.validate(packages, plan)

    def test_workspace_parent_path_and_sibling_license_rejects(self):
        for declared in ('../sys', '/sys', 'sibling', '.', 'sys//', 'sys/../sys'):
            with self.subTest(path=declared):
                packages, plan = self.workspace_parent()
                self.update_workspace_manifest(packages, plan, b'path = "sys/"', f'path = "{declared}"'.encode())
                with self.assertRaises(ValueError):
                    self.apply(packages, plan)
        packages, plan = self.workspace_parent()
        packages[0]['licenseTexts'][0]['sourcePath'] = 'other/LICENSE'
        with self.assertRaises(ValueError):
            self.apply(packages, plan)

    def test_workspace_parent_missing_or_supplemented_parent_rejects(self):
        for target in ('unlocked', 'missing-text', 'supplemented', 'foreign-ecosystem'):
            with self.subTest(target=target):
                packages, plan = self.workspace_parent()
                if target == 'unlocked':
                    packages.pop(0)
                elif target == 'missing-text':
                    packages[0]['licenseTexts'] = []
                elif target == 'supplemented':
                    packages[0]['textStatus'] = 'supplemented'
                else:
                    packages[0]['ecosystem'] = 'npm'
                with self.assertRaises(ValueError):
                    self.apply(packages, plan)

    def test_workspace_parent_full_collection_generation_and_offline_validation(self):
        packages, plan = self.workspace_parent()
        parent, child = packages
        proof = plan['parents'][parent['id']]
        spec = plan['bindings'][child['id']]
        raw_parent = base64.b64decode(proof['manifestBase64'])
        raw_child = b'[package]\nname = "platform"\nversion = "1.2.3"\nlicense = "MIT"\n'
        published = {}
        for package, raw, vcs, texts in ((parent, raw_parent, proof['vcs'], [('LICENSE', self.blob)]),
                                          (child, raw_child, spec['vcs'], [])):
            prefix = f'{package["name"]}-{package["version"]}'
            entries = [('Cargo.toml', raw), ('Cargo.toml.orig', raw), ('.cargo_vcs_info.json', m.encoded(vcs)), *texts]
            published[package['name']] = archive([(f'{prefix}/{path}', blob, None) for path, blob in entries])
        proof['archiveSha256'] = m.sha(published['parent'])
        spec['archiveSha256'] = m.sha(published['platform'])
        for name in m.INPUTS:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'{}\n')
        (self.root / 'Cargo.toml').write_text('[workspace]\nmembers = []\n')
        font_dir = self.root / 'ui/src/assets/fonts/jetbrains-mono'
        (font_dir / 'JetBrainsMono-Regular.woff2').write_bytes(b'font fixture')
        (font_dir / 'OFL.txt').write_bytes(self.blob)
        (font_dir / 'source.json').write_bytes(m.encoded({'version': '2.304', 'project': 'JetBrains Mono',
            'font': {'file': 'JetBrainsMono-Regular.woff2', 'sha256': m.sha(b'font fixture')},
            'notice': {'file': 'OFL.txt', 'sha256': self.sha}}))
        (self.root / m.supplemental.SOURCE_FILE).write_bytes(m.encoded(plan))
        rust = [{'name': name, 'version': '1.2.3', 'source': 'registry+https://github.com/rust-lang/crates.io-index',
                 'checksum': m.sha(blob)} for name, blob in published.items()]

        def fetch(url, *_):
            for name, blob in published.items():
                if url == f'https://static.crates.io/crates/{name}/{name}-1.2.3.crate':
                    return blob
            raise AssertionError(f'unexpected upstream download: {url}')

        with patch.object(m, 'lock_packages', return_value=(rust, [])), patch.object(m, 'archive_bytes', side_effect=fetch):
            report = m.generate(self.root, self.root)
            m.validate(self.root)
        result = next(p for p in report['packages'] if p['id'] == child['id'])
        self.assertEqual(result['textStatus'], 'supplemented')
        self.assertEqual(result['licenseTexts'][0]['sourcePackage'], parent['id'])
        self.assertFalse(report['releaseApproved'])
        self.assertIsNone(m.require_texts(report))

    def cargo(self, coverage='full-text'):
        vcs = {'git': {'sha1': 'a' * 40}, 'path_in_vcs': 'crates/example'}
        raw = m.encoded(vcs)
        data = archive([('example-1.2.3/.cargo_vcs_info.json', raw, None)])
        package = m.package_record('cargo', {'name': 'example', 'version': '1.2.3'},
                                   {'package': {'name': 'example', 'version': '1.2.3', 'license': 'MIT',
                                    'repository': 'https://github.com/example/project'}}, {},
                                   {'archiveSha256': 'b' * 64, 'vcsEvidence': m.supplemental.vcs_evidence(data, 'example-1.2.3')})
        spec = {'kind': 'cargo-vcs', 'archiveSha256': 'b' * 64, 'repository': package['repository'],
                'vcs': vcs, 'vcsSha256': m.sha(raw), 'coverage': coverage,
                'texts': [{'path': 'LICENSE', 'sha256': self.sha,
                           'url': 'https://raw.githubusercontent.com/example/project/' + 'a' * 40 + '/LICENSE'}]}
        plan = {'schemaVersion': 1, 'parents': {}, 'bindings': {package['id']: spec}}
        return [package], plan

    def apply(self, packages, plan, payload=None):
        (self.root / m.supplemental.SOURCE_FILE).write_bytes(m.encoded(plan))
        with patch.object(m.urllib.request, 'urlopen', return_value=io.BytesIO(self.blob if payload is None else payload)):
            m.supplemental.apply(self.root, self.root, packages, self.files, m.archive_bytes)

    def test_suffix_named_licenses_are_original_archive_texts(self):
        data = archive([('pkg/Cargo.toml', b'name="pkg"', None), ('pkg/MIT-LICENSE', self.blob, None),
                        ('pkg/Apache-LICENSE', self.blob, None)])
        _, texts = m.read_archive(data, 'pkg', 'Cargo.toml')
        self.assertEqual(set(texts), {'MIT-LICENSE', 'Apache-LICENSE'})

    def test_optional_parent_exact_binding_without_network(self):
        packages, plan = self.optional()
        with patch.object(m.urllib.request, 'urlopen', side_effect=AssertionError('must reuse archive bytes')):
            self.apply(packages, plan)
        m.supplemental.validate(packages, plan)
        self.assertEqual(packages[1]['licenseTexts'][0]['sourcePackage'], packages[0]['id'])
        self.assertIsNone(m.require_texts({'packages': packages}))

    def test_wrong_optional_version_repository_license_and_integrity_reject(self):
        for field, value in [('version', '9.0.0'), ('repository', 'https://github.com/other/project'),
                             ('declaredLicense', 'Apache-2.0')]:
            with self.subTest(field=field):
                packages, plan = self.optional()
                packages[1][field] = value
                with self.assertRaisesRegex(ValueError, 'mismatch'):
                    self.apply(packages, plan)
        packages, plan = self.optional()
        packages[0]['origin']['integrity'] = 'another archive'
        with self.assertRaisesRegex(ValueError, 'mismatch'):
            self.apply(packages, plan)

    def test_pinned_vcs_text_and_offline_provenance(self):
        packages, plan = self.cargo()
        self.apply(packages, plan)
        m.supplemental.validate(packages, plan)
        self.assertEqual(packages[0]['textStatus'], 'supplemented')
        self.assertEqual(self.files[f'third-party/licenses/{self.sha}.txt'], self.blob)
        packages[0]['licenseTexts'][0]['sourceUrl'] = 'https://example.com/current/LICENSE'
        with self.assertRaisesRegex(ValueError, 'differs'):
            m.supplemental.validate(packages, plan)

    def test_dirty_wrong_commit_and_sibling_paths_reject(self):
        for mutation in ('dirty', 'commit', 'sibling'):
            with self.subTest(mutation=mutation):
                packages, plan = self.cargo()
                spec = plan['bindings'][packages[0]['id']]
                if mutation == 'dirty':
                    spec['vcs']['git']['dirty'] = True
                elif mutation == 'commit':
                    spec['vcs']['git']['sha1'] = 'c' * 40
                else:
                    spec['texts'][0]['path'] = 'other/LICENSE'
                    spec['texts'][0]['url'] = spec['texts'][0]['url'].replace('/LICENSE', '/other/LICENSE')
                with self.assertRaises(ValueError):
                    self.apply(packages, plan)

    def test_changed_upstream_bytes_fail_integrity(self):
        packages, plan = self.cargo()
        with self.assertRaisesRegex(ValueError, 'integrity mismatch'):
            self.apply(packages, plan, b'replaced text')

    def test_notice_only_is_retained_but_release_gate_stays_closed(self):
        packages, plan = self.cargo('notice-only')
        self.apply(packages, plan)
        m.supplemental.validate(packages, plan)
        self.assertTrue(packages[0]['licenseTexts'])
        with self.assertRaisesRegex(ValueError, '1 packages need'):
            m.require_texts({'packages': packages})
        self.assertIn(b'Incomplete: notice only', m.documents({'packages': packages})['THIRD_PARTY.md'])

    def test_vcs_proof_and_unrelated_parent_cannot_be_inserted(self):
        data = archive([('pkg/.cargo_vcs_info.json', b'{}', None), ('pkg/.cargo_vcs_info.json', b'{}', None)])
        with self.assertRaisesRegex(ValueError, 'missing or unsafe'):
            m.supplemental.vcs_evidence(data, 'pkg')
        packages, plan = self.optional()
        self.apply(packages, plan)
        plan['parents']['npm:unrelated@1.0.0'] = {}
        with self.assertRaisesRegex(ValueError, 'unexpected'):
            m.supplemental.validate(packages, plan)


    def npm_commit(self, coverage='full-text'):
        manifest = {'name': '@example/terminal', 'version': '1.2.3', 'license': 'MIT',
                    'repository': 'https://github.com/example/project', 'commit': 'b' * 40}
        raw = m.encoded(manifest)
        package = m.package_record('npm', manifest, manifest, {},
                                  {'integrity': 'locked-integrity', 'manifestSha256': m.sha(raw)})
        spec = {'kind': 'npm-manifest-commit', 'integrity': 'locked-integrity',
                'manifestBase64': base64.b64encode(raw).decode(), 'manifestSha256': m.sha(raw),
                'repository': manifest['repository'], 'commit': manifest['commit'], 'coverage': coverage,
                'texts': [{'path': 'LICENSE', 'sha256': self.sha,
                           'url': f'https://raw.githubusercontent.com/example/project/{manifest["commit"]}/LICENSE'}]}
        plan = {'schemaVersion': 1, 'parents': {}, 'bindings': {package['id']: spec}}
        return [package], plan

    def test_embedded_npm_commit_and_offline_proof(self):
        packages, plan = self.npm_commit()
        self.apply(packages, plan)
        m.supplemental.validate(packages, plan)
        self.assertIsNone(m.require_texts({'packages': packages}))
        self.assertIn(b'Locked npm manifest commit', m.documents({'packages': packages})['THIRD_PARTY.md'])
        packages[0]['origin']['manifestSha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'proof mismatch'):
            m.supplemental.validate(packages, plan)

    def test_npm_embedded_manifest_rejects_identity_commit_and_lock_changes(self):
        for field, value in [('name', 'other'), ('version', '9.0.0'), ('license', 'Apache-2.0'),
                             ('repository', 'https://github.com/other/project'), ('commit', 'main'),
                             ('commit', None), ('gitHead', 'c' * 40)]:
            with self.subTest(field=field, value=value):
                packages, plan = self.npm_commit()
                spec = plan['bindings'][packages[0]['id']]
                manifest = json.loads(base64.b64decode(spec['manifestBase64']))
                manifest[field] = value
                raw = m.encoded(manifest)
                spec['manifestBase64'] = base64.b64encode(raw).decode()
                spec['manifestSha256'] = packages[0]['origin']['manifestSha256'] = m.sha(raw)
                with self.assertRaisesRegex(ValueError, 'proof mismatch'):
                    self.apply(packages, plan)
        packages, plan = self.npm_commit()
        packages[0]['origin']['integrity'] = 'other archive'
        with self.assertRaisesRegex(ValueError, 'proof mismatch'):
            self.apply(packages, plan)

    def test_npm_manifest_proof_bytes_and_root_path_cannot_be_forged(self):
        for mutation in ('bytes', 'branch', 'sibling', 'traversal', 'duplicate', 'extra-field'):
            with self.subTest(mutation=mutation):
                packages, plan = self.npm_commit()
                spec = plan['bindings'][packages[0]['id']]
                if mutation == 'bytes':
                    spec['manifestBase64'] = base64.b64encode(b'{}').decode()
                elif mutation == 'branch':
                    spec['texts'][0]['url'] = 'https://raw.githubusercontent.com/example/project/main/LICENSE'
                elif mutation in ('sibling', 'traversal'):
                    path = 'other/LICENSE' if mutation == 'sibling' else '../LICENSE'
                    spec['texts'][0].update(path=path, url=f'https://raw.githubusercontent.com/example/project/{spec["commit"]}/{path}')
                elif mutation == 'duplicate':
                    spec['texts'].append(dict(spec['texts'][0]))
                else:
                    spec['registryGitHead'] = spec['commit']
                with self.assertRaises(ValueError):
                    self.apply(packages, plan)

    def test_npm_notice_only_still_rejects_release(self):
        packages, plan = self.npm_commit('notice-only')
        self.apply(packages, plan)
        with self.assertRaisesRegex(ValueError, '1 packages need'):
            m.require_texts({'packages': packages})

    def test_npm_commit_changed_text_fails_before_publication(self):
        packages, plan = self.npm_commit()
        with self.assertRaisesRegex(ValueError, 'integrity mismatch'):
            self.apply(packages, plan, b'not the pinned original')



    def published_reference(self):
        blob = m.supplemental.MPL_NOTICE + b'\n\npub fn example() {}\n'
        path = 'src/lib.rs'
        proof = {'path': path, 'sha256': m.sha(blob), 'base64': base64.b64encode(blob).decode()}
        package = m.package_record('cargo', {'name': 'referenced', 'version': '1.0.0'},
                                   {'name': 'referenced', 'version': '1.0.0', 'license': 'MPL-2.0'}, {},
                                   {'url': 'https://static.crates.io/crates/referenced/referenced-1.0.0.crate',
                                    'archiveSha256': 'ab' * 32,
                                    'licenseReference': {'path': path, 'sha256': m.sha(blob)}})
        binding = {'kind': 'cargo-license-reference', 'coverage': 'full-text',
                   'archiveSha256': 'ab' * 32, 'proof': proof,
                   'text': {'path': 'MPL-2.0.txt', 'url': m.supplemental.MPL_TEXT_URL, 'sha256': self.sha}}
        return [package], {'schemaVersion': 1, 'parents': {}, 'bindings': {package['id']: binding}}

    def test_published_reference_preserves_notice_and_official_text(self):
        packages, plan = self.published_reference()
        proof = plan['bindings'][packages[0]['id']]['proof']
        self.apply(packages, plan)
        self.assertEqual(self.files[f'third-party/licenses/{proof["sha256"]}.txt'], base64.b64decode(proof['base64']))
        self.assertEqual(self.files[f'third-party/licenses/{self.sha}.txt'], self.blob)
        self.assertEqual(len(packages[0]['licenseTexts']), 2)
        m.supplemental.validate(packages, plan)
        m.require_texts({'packages': packages})

    def test_published_reference_rejects_unbound_archive_identity_and_license(self):
        for mutation in ('archive', 'origin', 'ecosystem', 'license', 'coverage', 'extra'):
            with self.subTest(mutation=mutation):
                packages, plan = self.published_reference()
                package = packages[0]; binding = plan['bindings'][package['id']]
                if mutation == 'archive': binding['archiveSha256'] = 'cd' * 32
                elif mutation == 'origin': package['origin']['licenseReference']['sha256'] = 'cd' * 32
                elif mutation == 'ecosystem': package['ecosystem'] = 'npm'
                elif mutation == 'license': package['declaredLicense'] = 'MIT'
                elif mutation == 'coverage': binding['coverage'] = 'notice-only'
                else: binding['unchecked'] = True
                with self.assertRaises(ValueError): self.apply(packages, plan)

    def test_published_reference_rejects_source_proof_and_paths(self):
        for mutation in ('base64', 'digest', 'relative', 'absolute', 'extension', 'absent-notice'):
            with self.subTest(mutation=mutation):
                packages, plan = self.published_reference()
                proof = plan['bindings'][packages[0]['id']]['proof']
                if mutation == 'base64': proof['base64'] = 'not-base64!'
                elif mutation == 'digest': proof['sha256'] = 'cd' * 32
                elif mutation in ('relative', 'absolute', 'extension'):
                    proof['path'] = {'relative': '../lib.rs', 'absolute': '/lib.rs', 'extension': 'README.md'}[mutation]
                    packages[0]['origin']['licenseReference']['path'] = proof['path']
                else:
                    blob = b'// SPDX-License-Identifier: MPL-2.0\n'
                    proof.update(base64=base64.b64encode(blob).decode(), sha256=m.sha(blob))
                    packages[0]['origin']['licenseReference']['sha256'] = m.sha(blob)
                with self.assertRaises(ValueError): self.apply(packages, plan)

    def test_published_reference_rejects_other_text_endpoints(self):
        for field, value in [('url', 'http://www.mozilla.org/media/MPL/2.0/index.txt'),
                             ('url', 'https://example.com/MPL-2.0.txt'),
                             ('path', 'LICENSE'), ('sha256', 'not-a-digest')]:
            with self.subTest(field=field, value=value):
                packages, plan = self.published_reference()
                plan['bindings'][packages[0]['id']]['text'][field] = value
                with self.assertRaises(ValueError): self.apply(packages, plan)

    def test_published_reference_changed_official_bytes_reject(self):
        packages, plan = self.published_reference()
        with self.assertRaisesRegex(ValueError, 'integrity mismatch'):
            self.apply(packages, plan, b'different official response')

    def test_reference_archive_evidence_rejects_missing_duplicate_link_and_oversize(self):
        blob = m.supplemental.MPL_NOTICE
        valid = ('pkg/lib.rs', blob, None)
        self.assertEqual(m.supplemental.reference_evidence(archive([valid]), 'pkg', 'lib.rs'),
                         {'path': 'lib.rs', 'sha256': m.sha(blob)})
        for entries in ([], [valid, valid], [('pkg/lib.rs', b'', 'elsewhere')],
                        [('pkg/lib.rs', b'x' * (256 * 1024 + 1), None)]):
            with self.assertRaises(ValueError):
                m.supplemental.reference_evidence(archive(entries), 'pkg', 'lib.rs')
        for path in ('../lib.rs', '/lib.rs', './lib.rs', 'LICENSE'):
            with self.assertRaises(ValueError):
                m.supplemental.reference_evidence(archive([valid]), 'pkg', path)

    def test_reference_offline_record_cannot_hide_proof_or_text_drift(self):
        packages, plan = self.published_reference()
        self.apply(packages, plan)
        packages[0]['licenseTexts'].pop(0)
        with self.assertRaises(ValueError): m.supplemental.validate(packages, plan)


    def test_reference_provenance_renders_without_inventing_vcs(self):
        packages, plan = self.published_reference()
        self.apply(packages, plan)
        documents = m.documents({'packages': packages})
        inventory = documents['THIRD_PARTY.md'].decode()
        self.assertIn('Published source `src/lib.rs`; official MPL 2.0 original', inventory)
        self.assertIn('src/lib.rs', inventory)
        self.assertIn('MPL-2.0.txt', inventory)
        self.assertNotIn('Published VCS', inventory)
        self.assertIn(b'explicit source', documents['NOTICE'])


if __name__ == '__main__':
    unittest.main()
