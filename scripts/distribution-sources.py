"""Prepare verified MPL registry sources for review; never approve or publish a release."""
import hashlib
import importlib.util
import json
import shutil
from pathlib import Path
import re
import tempfile
import tomllib
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def source_files(root, inventory, download):
    packages = [p for p in inventory['packages']
                if p['ecosystem'] == 'cargo' and 'MPL-' in p.get('declaredLicense', '')]
    if not packages:
        raise ValueError('no covered MPL sources in inventory')
    manifest = tomllib.loads((root / 'Cargo.toml').read_text())
    patched = {value.get('package', name) for table in manifest.get('patch', {}).values()
               for name, value in table.items()}
    if manifest.get('replace') or patched & {p['name'] for p in packages}:
        raise ValueError('modified MPL sources require a separate source review')
    files, sources = {}, []
    for package in sorted(packages, key=lambda p: p['id']):
        name, version = package['name'], package['version']
        if (package['ecosystem'] != 'cargo' or package['declaredLicense'] != 'MPL-2.0'
                or package['textStatus'] not in ('collected', 'supplemented')
                or not re.fullmatch(r'[A-Za-z0-9_-]+', name)
                or not re.fullmatch(r'[0-9][A-Za-z0-9.+-]*', version)
                or not package['licenseTexts']):
            raise ValueError('unsupported or incomplete MPL source')
        archive = f'{name}-{version}.crate'
        url = f'https://static.crates.io/crates/{name}/{archive}'
        expected = package['origin']['archiveSha256']
        if package['origin']['url'] != url:
            raise ValueError('MPL source URL differs from locked registry archive')
        data = download(url, archive, expected)
        if sha(data) != expected:
            raise ValueError('MPL source archive checksum mismatch')
        path = f'sources/{archive}'
        if path in files:
            raise ValueError('duplicate MPL source')
        files[path] = data
        licenses = []
        for text in package['licenseTexts']:
            original = (root / text['file']).read_bytes()
            digest = sha(original)
            if digest != text['sha256']:
                raise ValueError('MPL license checksum mismatch')
            member = f'licenses/{digest}.txt'
            files[member] = original
            licenses.append({'path': member, 'sourcePath': text['sourcePath'], 'sha256': digest})
        sources.append({'id': package['id'], 'path': path, 'url': url,
                        'sha256': expected, 'licenseTexts': licenses})
    report = {'schemaVersion': 1, 'releaseApproved': False,
              'scope': 'Locked original Rust MPL registry archives; npm and binary content remain separate',
              'cargoLockSha256': sha((root / 'Cargo.lock').read_bytes()), 'sources': sources}
    files['manifest.json'] = (json.dumps(report, sort_keys=True, indent=2) + '\n').encode()
    lines = ['# Covered source code', '',
             'This companion contains the original Rust MPL-2.0 registry source archives and license texts.',
             'manifest.json records exact versions, original download URLs and SHA-256 checksums.',
             'The .crate files are gzip-compressed tar archives; no source archive was executed or extracted.',
             'MPL source patches/replacements require separate review and are rejected by this preparer.',
             'Distribute this source companion alongside the matching installer and retain it at the same download location.',
             'It covers Rust registry packages only; npm native tools and system libraries require separate source review.',
             'Preparing this file does not publish a source channel or approve the installer, signing or other licenses.', '',
             '| Component | Original source | SHA-256 |', '|---|---|---|']
    lines.extend(f'| {s["id"]} | [{s["path"]}]({s["url"]}) | {s["sha256"]} |' for s in sources)
    files['README.md'] = ('\n'.join(lines) + '\n').encode()
    return files, report


def main():
    spec = importlib.util.spec_from_file_location('source_attribution', ROOT / 'scripts/third-party.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    output = ROOT / 'target/probes/distribution-sources' / uuid.uuid4().hex
    output.mkdir(parents=True, exist_ok=False)
    receipt = output / 'summary.json'
    result = {'completed': False, 'releaseApproved': False}
    receipt.write_text(json.dumps(result) + '\n')
    try:
        inventory = module.validate(ROOT)
        before = (module.fingerprints(ROOT), sha((ROOT / 'third-party/inventory.json').read_bytes()))
        Path('/data/temp').mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='distribution-sources-', dir='/data/temp') as scratch:
            def download(url, name, digest):
                return module.archive_bytes(url, Path(scratch) / name, digest, 'sha256')
            files, report = source_files(ROOT, inventory, download)
            staged = Path(scratch) / 'mpl-sources.zip'
            with zipfile.ZipFile(staged, 'w', compression=zipfile.ZIP_DEFLATED) as bundle:
                for name, data in sorted(files.items()):
                    bundle.writestr(name, data)
            module.validate(ROOT)
            if before != (module.fingerprints(ROOT), sha((ROOT / 'third-party/inventory.json').read_bytes())):
                raise ValueError('source inputs changed during preparation')
            destination = output / 'mpl-sources.zip'
            shutil.copyfile(staged, destination)
            (output / 'manifest.json').write_bytes(files['manifest.json'])
        result.update(completed=True, components=len(report['sources']),
                      archive=destination.relative_to(ROOT).as_posix(),
                      archiveSha256=sha(destination.read_bytes()))
        receipt.write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
        print(json.dumps(result))
    except Exception as error:
        result.update(completed=False, failure=str(error))
        receipt.write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
        raise


if __name__ == '__main__':
    main()
