#!/usr/bin/env python3
"""Build/check a locked workspace attribution inventory; never approve licenses."""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import tarfile
import tempfile
import tomllib
import time
import urllib.error
import urllib.request
import third_party_sources as supplemental

ROOT = Path(__file__).resolve().parent.parent
INPUTS = ('Cargo.lock', 'Cargo.toml', 'pnpm-lock.yaml', 'package.json', 'ui/package.json',
          'pnpm-workspace.yaml', 'deny.toml', 'vendor/alacritty_terminal.upstream.json',
          'vendor/alacritty_terminal.patch', 'vendor/README.md',
          'ui/src/assets/fonts/jetbrains-mono/source.json',
          'ui/src/assets/fonts/jetbrains-mono/OFL.txt',
          'ui/src/assets/fonts/jetbrains-mono/JetBrainsMono-Regular.woff2', 'scripts/third-party.py',
          'scripts/third_party_sources.py', supplemental.SOURCE_FILE)
PATTERN = re.compile(r'^(?:(?:MIT|Apache(?:-2\.0)?|BSD(?:-[23]-Clause)?)[._-])?(?:licen[cs]es?|copying|copyright|notices?|authors?|third[-_]?party(?:[-_]licen[cs]es?)?)(?:[._-].*)?$', re.I)
LIMIT = 192 * 1024 * 1024


def sha(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()


def fingerprints(root):
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']
    paths = (*INPUTS, *(f'{member}/Cargo.toml' for member in workspace['members']))
    return {p: sha((root / p).read_bytes()) for p in paths}


def lock_packages(root):
    cargo = tomllib.loads((root / 'Cargo.lock').read_text())
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']
    first_party = set()
    for member in workspace['members']:
        package = tomllib.loads((root / member / 'Cargo.toml').read_text())['package']
        version = package['version']
        if isinstance(version, dict) and version.get('workspace') is True:
            version = workspace['package']['version']
        first_party.add((package['name'], version))
    rust = [p for p in cargo['package'] if (p['name'], p['version']) not in first_party]
    if cargo.get('version') != 4 or len(rust) + len(first_party) != len(cargo['package']):
        raise ValueError('unsupported Cargo lock/workspace membership')
    for p in rust:
        if p.get('source') and (p['source'] != 'registry+https://github.com/rust-lang/crates.io-index'
                                or not re.fullmatch(r'[0-9a-f]{64}', p.get('checksum', ''))):
            raise ValueError(f'unsupported Rust source: {p["name"]}')
        if not p.get('source') and p['name'] != 'alacritty_terminal':
            raise ValueError(f'unreviewed path dependency: {p["name"]}')
    text = (root / 'pnpm-lock.yaml').read_text()
    if not text.startswith("lockfileVersion: '9.0'\n") or text.count('\npackages:\n') != 1:
        raise ValueError('unsupported pnpm lock format')
    section = text.split('\npackages:\n', 1)[1].split('\nsnapshots:', 1)[0]
    starts = list(re.finditer(r"^  (?:'([^']+)'|([^\s'\"][^:]*)):[ \t]*$", section, re.M))
    npm = []
    for i, match in enumerate(starts):
        identity = match[1] or match[2]
        name, version = identity.rsplit('@', 1)
        if not re.fullmatch(r'(?:@[\w.-]+/)?[\w.-]+', name) or not re.fullmatch(r'[0-9][\w.+-]*', version):
            raise ValueError(f'unsupported npm identity: {identity}')
        block = section[match.end(): starts[i + 1].start() if i + 1 < len(starts) else len(section)]
        integrity = re.findall(r'^    resolution: \{integrity: (sha512-[A-Za-z0-9+/=]+)\}\s*$', block, re.M)
        if len(integrity) != 1:
            raise ValueError(f'unsupported or missing npm integrity: {identity}')
        npm.append({'name': name, 'version': version, 'integrity': integrity[0]})
    # Refuse a changed header syntax instead of silently omitting packages.
    headers = re.findall(r'^  \S.*:[ \t]*$', section, re.M)
    if not npm or len(headers) != len(starts):
        raise ValueError('incomplete pnpm package table')
    return rust, npm


def archive_bytes(url, destination, expected, algorithm):
    if not destination.exists():
        request = urllib.request.Request(url, headers={'User-Agent': 'PulseTerm-attribution/1'})
        for attempt in range(3):
            try:
                with urllib.request.urlopen(request, timeout=45) as response, destination.open('xb') as out:
                    size = 0
                    while chunk := response.read(1024 * 1024):
                        size += len(chunk)
                        if size > LIMIT:
                            raise ValueError(f'archive exceeds size limit: {url}')
                        out.write(chunk)
                break
            except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
                destination.unlink(missing_ok=True)
                if isinstance(error, urllib.error.HTTPError) and error.code not in (429, 500, 502, 503, 504):
                    raise
                if attempt == 2:
                    raise RuntimeError(f'archive download failed after three attempts: {url}') from error
                time.sleep(0.25 * (attempt + 1))
    data = destination.read_bytes()
    if len(data) > LIMIT:
        raise ValueError('cached archive exceeds size limit')
    actual = hashlib.new(algorithm, data).digest()
    wanted = bytes.fromhex(expected) if algorithm == 'sha256' else base64.b64decode(expected, validate=True)
    if actual != wanted:
        raise ValueError(f'archive integrity mismatch: {url}')
    return data


def read_archive(data, prefix, manifest_name):
    texts = {}
    manifest = None
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        for entry in archive:
            path = PurePosixPath(entry.name)
            if path.is_absolute() or '..' in path.parts or not path.parts:
                raise ValueError('unsafe archive member path')
            if prefix is None:
                prefix = path.parts[0]
            if path.parts[0] != prefix:
                raise ValueError('mixed archive roots')
            short = '/'.join(path.parts[1:])
            if short == manifest_name and entry.isfile():
                if manifest is not None or entry.size > 2 * 1024 * 1024:
                    raise ValueError('duplicate or oversized package manifest')
                source = archive.extractfile(entry)
                if source is None:
                    raise ValueError('missing package manifest stream')
                with source:
                    manifest = source.read()
            candidate = any(PATTERN.fullmatch(part) for part in path.parts[1:])
            if candidate and entry.isfile():
                if entry.size > 4 * 1024 * 1024:
                    raise ValueError('oversized attribution text')
                if short in texts:
                    raise ValueError('duplicate attribution archive member')
                source = archive.extractfile(entry)
                if source is None:
                    raise ValueError('missing attribution stream')
                with source:
                    blob = source.read()
                blob.decode('utf-8')
                texts[short] = blob
            elif candidate and (entry.issym() or entry.islnk()):
                raise ValueError('attribution symlink requires explicit review')
    if manifest is None:
        raise ValueError('archive has no package manifest')
    return manifest, texts


def package_record(kind, item, manifest, texts, origin):
    package = manifest.get('package', manifest)
    if package.get('name') != item['name'] or package.get('version') != item['version']:
        raise ValueError(f'archive package identity mismatch: {item["name"]}')
    license_value = package.get('license')
    if isinstance(license_value, dict):
        license_value = license_value.get('type')
    if not isinstance(license_value, str) or not license_value.strip():
        raise ValueError(f'missing declared license: {item["name"]}')
    return {'id': f'{kind}:{item["name"]}@{item["version"]}', 'ecosystem': kind,
            'name': item['name'], 'version': item['version'], 'declaredLicense': license_value,
            'authors': package.get('authors', package.get('author', [])),
            'repository': package.get('repository'), 'origin': origin,
            'licenseTexts': [{'sourcePath': name, 'sha256': sha(blob),
                              'file': f'third-party/licenses/{sha(blob)}.txt'}
                             for name, blob in sorted(texts.items())],
            'textStatus': 'collected' if texts else 'missing-from-published-package'}


def collect(root, scratch, rust, npm):
    cargo_home = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')))
    source_plan = supplemental.load(root)

    def rust_package(item):
        name, version = item['name'], item['version']
        if not item.get('source'):
            base = root / 'vendor/alacritty_terminal'
            provenance = json.loads((root / 'vendor/alacritty_terminal.upstream.json').read_text())
            texts = {n: (base / n).read_bytes() for n in provenance['files']
                     if PATTERN.fullmatch(PurePosixPath(n).name)}
            for n, blob in texts.items():
                if sha(blob) != provenance['files'][n]:
                    raise ValueError('vendored attribution changed')
            record = package_record('cargo', item, tomllib.loads((base / 'Cargo.toml').read_text()), texts,
                                    {'source': 'vendor/alacritty_terminal', 'upstreamArchiveSha256': provenance['crateSha256'],
                                     'modificationNotice': 'vendor/README.md', 'patch': 'vendor/alacritty_terminal.patch'})
            return record, texts
        url = f'https://static.crates.io/crates/{name}/{name}-{version}.crate'
        caches = sorted((cargo_home / 'registry/cache').glob(f'*/{name}-{version}.crate'))
        path = caches[0] if caches else scratch / f'{sha(url.encode())}.crate'
        data = archive_bytes(url, path, item['checksum'], 'sha256')
        raw, texts = read_archive(data, f'{name}-{version}', 'Cargo.toml')
        record = package_record('cargo', item, tomllib.loads(raw.decode()), texts,
                                {'url': url, 'archiveSha256': item['checksum']})
        binding = source_plan['bindings'].get(record['id'])
        is_parent = record['id'] in source_plan['parents']
        if is_parent:
            original, _ = read_archive(data, f'{name}-{version}', 'Cargo.toml.orig')
            record['origin']['manifestSha256'] = sha(original)
        if binding is not None or is_parent:
            if binding is not None and binding['kind'] == 'cargo-license-reference':
                record['origin']['licenseReference'] = supplemental.reference_evidence(
                    data, f'{name}-{version}', binding['proof']['path'])
            else:
                record['origin']['vcsEvidence'] = supplemental.vcs_evidence(data, f'{name}-{version}')
        return record, texts

    def npm_package(item):
        name, version = item['name'], item['version']
        url = f'https://registry.npmjs.org/{name}/-/{name.rsplit("/", 1)[-1]}-{version}.tgz'
        data = archive_bytes(url, scratch / f'{sha(url.encode())}.tgz', item['integrity'][7:], 'sha512')
        raw, texts = read_archive(data, None, 'package.json')
        record = package_record('npm', item, json.loads(raw), texts,
                                {'url': url, 'integrity': item['integrity']})
        if (record['id'] in source_plan['parents']
                or source_plan['bindings'].get(record['id'], {}).get('kind') == 'npm-manifest-commit'):
            record['origin']['manifestSha256'] = sha(raw)
        return record, texts

    results = []
    # Archives can contain platform binaries; they are never extracted or executed.
    with ThreadPoolExecutor(max_workers=4) as pool:
        results.extend(pool.map(rust_package, rust))
        results.extend(pool.map(npm_package, npm))
    blobs = {}
    records = []
    for record, texts in results:
        records.append(record)
        for blob in texts.values():
            blobs[f'third-party/licenses/{sha(blob)}.txt'] = blob
    font_dir = root / 'ui/src/assets/fonts/jetbrains-mono'
    font = json.loads((font_dir / 'source.json').read_text())
    for key in ('font', 'notice'):
        if sha((font_dir / font[key]['file']).read_bytes()) != font[key]['sha256']:
            raise ValueError(f'font {key} provenance mismatch')
    blob = (font_dir / font['notice']['file']).read_bytes()
    blobs[f'third-party/licenses/{sha(blob)}.txt'] = blob
    records.append({'id': 'asset:JetBrains Mono@' + font['version'], 'ecosystem': 'asset',
                    'name': font['project'], 'version': font['version'], 'declaredLicense': 'OFL-1.1',
                    'authors': 'The JetBrains Mono Project Authors', 'repository': 'https://github.com/JetBrains/JetBrainsMono',
                    'origin': font, 'licenseTexts': [{'sourcePath': 'OFL.txt', 'sha256': sha(blob),
                                                     'file': f'third-party/licenses/{sha(blob)}.txt'}],
                    'textStatus': 'collected'})
    records.sort(key=lambda p: p['id'])
    supplemental.apply(root, scratch, records, blobs, archive_bytes)
    return records, blobs


def escaped(value):
    text = str(value).replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;').replace('|', '&#124;').replace('`', '&#96;').replace('\n', ' ')
    return re.sub(r'([\\\[\]*_])', r'\\\1', text)


def documents(report):
    packages = report['packages']
    missing = [p for p in packages if p['textStatus'] not in supplemental.COMPLETE]
    notice = '''PulseTerm third-party attribution materials

PulseTerm incorporates third-party software and the unmodified JetBrains Mono font.
See THIRD_PARTY.md and third-party/inventory.json for exact locked versions, declared
licenses, archive integrity, and links to collected original license/notice texts.
Redistributions must retain the applicable original texts in third-party/licenses/.

The alacritty_terminal 0.26.0 source contains the approved read-only inactive_grid
accessor patch. Upstream copyright and Apache-2.0 license are retained. Modification
and provenance details: vendor/README.md, vendor/alacritty_terminal.patch, and
vendor/alacritty_terminal.upstream.json. No upstream acceptance is claimed.

JetBrains Mono 2.304: Copyright 2020 The JetBrains Mono Project Authors
(https://github.com/JetBrains/JetBrainsMono). Licensed under SIL Open Font License 1.1.
Original font bytes and complete OFL are retained; see ui/src/assets/fonts/jetbrains-mono/.

This is a workspace-wide review inventory, including development/build tools and
optional platform packages. It is not a per-binary SBOM or release authorization.
Supplemental texts retain their locked-parent, published-commit or explicit source
reference provenance in THIRD_PARTY.md. Link-only notices do not satisfy the text gate.
Native subcomponents and operating-system libraries still require distribution review.
Declared licenses have not been accepted by this inventory. The independent audit,
source-distribution obligations, and platform release gates continue to apply.
'''
    lines = ['# Third-party dependency inventory', '',
             'Generated by `mise run third-party-write`; verify with `mise run third-party-check`.', '',
             f'{len(packages)} components; {sum(p["textStatus"] != "collected" for p in packages)} packages omit separate texts in their archive; {len(missing)} still lack complete attribution texts after verified supplements.', '',
             'Scope: every non-workspace Cargo.lock package, every pnpm v9 `packages` entry (including platform optional packages), and the bundled font. This is a workspace review inventory, not proof of the contents or licensing completeness of a specific binary.', '',
             'Original bytes are retained in `third-party/licenses/`; names, SPDX expressions, and authors are upstream declarations, not approval. Archive SHA-256/SHA-512 is checked against the locks. Missing texts and bundled native/system components require separate review. Do not substitute a generic SPDX template for an absent upstream copyright notice.', '',
             'Supplemental texts bind a published clean Cargo VCS commit, a locked npm parent or embedded commit, or an explicit published-source license reference to its official original. Their provenance is explicit below. Link-only notices remain incomplete and do not pass the text gate.', '',
             '## Components still missing complete texts', '']
    lines.extend(f'- `{escaped(p["id"])}` — {escaped(p["declaredLicense"])} ({p["textStatus"]})' for p in missing)
    lines.extend(['', '## Locked components', '', '| Component | Declared license | Original texts | Provenance |', '| --- | --- | --- | --- |'])
    for p in packages:
        links = ', '.join(f'[{escaped(t["sourcePath"])}]({t["file"]})' for t in p['licenseTexts']) or '**Missing from archive**'
        source = p.get('supplementalSource')
        provenance = 'Published archive'
        if source:
            if source['kind'] in ('npm-optional-parent', 'cargo-workspace-parent'):
                role = 'workspace parent' if source['kind'] == 'cargo-workspace-parent' else 'parent'
                provenance = f'Locked {role} `{source["parent"]}`'
            elif source['kind'] == 'npm-manifest-commit':
                provenance = f'Locked npm manifest commit `{source["commit"]}`; {source["coverage"]}'
            elif source['kind'] == 'cargo-license-reference':
                provenance = f'Published source `{source["proof"]["path"]}`; official MPL 2.0 original; {source["coverage"]}'
            else:
                provenance = f'Published VCS `{source["vcs"]["git"]["sha1"]}`; {source["coverage"]}'
        if p['textStatus'] == 'supplemented-notice-only':
            links += ' **Incomplete: notice only**'
        lines.append(f'| {escaped(p["id"])} | {escaped(p["declaredLicense"])} | {links} | {provenance} |')
    lines.extend(['', 'Inputs and per-file SHA-256 values are in [the machine-readable inventory](third-party/inventory.json).', ''])
    return {'NOTICE': notice.encode(), 'THIRD_PARTY.md': '\n'.join(lines).encode()}


def validate(root, check_inputs=True):
    report = json.loads((root / 'third-party/inventory.json').read_bytes())
    if (report.get('schemaVersion') != 1 or report.get('generator') != 'scripts/third-party.py'
            or report.get('releaseApproved') is not False):
        raise ValueError('unsupported attribution inventory or false release approval')
    if check_inputs:
        if report['inputs'] != fingerprints(root):
            raise ValueError('attribution inputs changed; regenerate and review')
        rust, npm = lock_packages(root)
        expected = {f'cargo:{p["name"]}@{p["version"]}': p for p in rust}
        expected.update({f'npm:{p["name"]}@{p["version"]}': p for p in npm})
        font = json.loads((root / 'ui/src/assets/fonts/jetbrains-mono/source.json').read_text())
        expected['asset:JetBrains Mono@' + font['version']] = {}
        ids = [p['id'] for p in report['packages']]
        if set(ids) != set(expected) or len(ids) != len(expected):
            raise ValueError('attribution package set differs from locks')
        for p in report['packages']:
            source = expected[p['id']]
            if p['ecosystem'] == 'npm' and p['origin']['integrity'] != source['integrity']:
                raise ValueError('npm attribution integrity differs from lock')
            if p['ecosystem'] == 'cargo' and source.get('checksum') and p['origin']['archiveSha256'] != source['checksum']:
                raise ValueError('Cargo attribution checksum differs from lock')
    wanted = set()
    for p in report['packages']:
        if p['id'] != f'{p["ecosystem"]}:{p["name"]}@{p["version"]}':
            raise ValueError('inconsistent component identity')
        if p['textStatus'] not in (*supplemental.COMPLETE, 'supplemented-notice-only', 'missing-from-published-package') or ((p['textStatus'] != 'missing-from-published-package') != bool(p['licenseTexts'])):
            raise ValueError('inconsistent license text status')
        for text in p['licenseTexts']:
            name = f'third-party/licenses/{text["sha256"]}.txt'
            if not re.fullmatch(r'[0-9a-f]{64}', text['sha256']) or text['file'] != name:
                raise ValueError('invalid license text path')
            if (root / name).is_symlink() or sha((root / name).read_bytes()) != text['sha256']:
                raise ValueError('license text bytes changed')
            wanted.add(name)
    if (root / 'third-party/licenses').is_symlink():
        raise ValueError('attribution text directory is a symlink')
    actual = {p.relative_to(root).as_posix() for p in (root / 'third-party/licenses').iterdir()}
    if actual != wanted:
        raise ValueError('unexpected or missing attribution files')
    wanted.update(('NOTICE', 'THIRD_PARTY.md'))
    if set(report['generatedFiles']) != wanted:
        raise ValueError('generated file set differs from collected texts')
    for name, digest in report['generatedFiles'].items():
        if (root / name).is_symlink() or sha((root / name).read_bytes()) != digest:
            raise ValueError(f'generated file changed: {name}')
    if check_inputs:
        supplemental.validate(report['packages'], supplemental.load(root))
        for name, data in documents(report).items():
            if (root / name).read_bytes() != data:
                raise ValueError(f'{name} changed or stale')
    return report


def generate(root, scratch):
    previous = validate(root, check_inputs=False) if (root / 'third-party/inventory.json').exists() else None
    before = fingerprints(root)
    rust, npm = lock_packages(root)
    packages, files = collect(root, scratch, rust, npm)
    if fingerprints(root) != before:
        raise ValueError('inputs changed during attribution collection')
    report = {'schemaVersion': 1, 'generator': 'scripts/third-party.py', 'inputs': before,
              'scope': 'workspace lock superset; not a per-binary SBOM', 'releaseApproved': False,
              'packages': packages}
    files.update(documents(report))
    report['generatedFiles'] = {name: sha(data) for name, data in sorted(files.items())}
    files['third-party/inventory.json'] = encoded(report)
    owned = set(previous['generatedFiles']) | {'third-party/inventory.json'} if previous else set()
    for name in files:
        if (root / name).exists() and name not in owned:
            raise ValueError(f'refuse to overwrite unowned attribution: {name}')
    if previous:
        validate(root, check_inputs=False)
    # Publish the inventory last; an interrupted update must fail validation.
    for name in sorted(files, key=lambda n: (n == 'third-party/inventory.json', n)):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(files[name])
    for name in owned - set(files):
        (root / name).unlink()
    return validate(root)


def require_texts(report):
    missing = [p['id'] for p in report['packages'] if p['textStatus'] not in supplemental.COMPLETE]
    if missing:
        raise ValueError(f'{len(missing)} packages need original attribution texts; see THIRD_PARTY.md')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='generate or refresh verified owned output')
    parser.add_argument('--require-texts', action='store_true', help='fail if any component still lacks complete attribution text')
    args = parser.parse_args()
    if args.write:
        Path('/data/temp').mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='third-party-', dir='/data/temp') as scratch:
            report = generate(ROOT, Path(scratch))
    else:
        report = validate(ROOT)
    print(json.dumps({'checked': True, 'components': len(report['packages']),
                      'missingPublishedTexts': sum(p['textStatus'] != 'collected' for p in report['packages']),
                      'missingCompleteTexts': sum(p['textStatus'] not in supplemental.COMPLETE for p in report['packages']),
                      'supplementedComponents': sum(p['textStatus'].startswith('supplemented') for p in report['packages']),
                      'noticeOnlyComponents': sum(p['textStatus'] == 'supplemented-notice-only' for p in report['packages']),
                      'releaseApproved': False}, sort_keys=True))
    if args.require_texts:
        require_texts(report)


if __name__ == '__main__':
    main()
