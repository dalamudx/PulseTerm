"""Verify curated supplemental attribution against locked publication evidence."""
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
import json
from pathlib import PurePosixPath
import re
import tarfile
import tomllib

SOURCE_FILE = 'third-party/supplemental-sources.json'
COMPLETE = ('collected', 'supplemented')


def digest(blob):
    return hashlib.sha256(blob).hexdigest()


def load(root):
    path = root / SOURCE_FILE
    if path.is_symlink():
        raise ValueError('supplemental source plan is a symlink')
    plan = json.loads(path.read_bytes())
    if set(plan) != {'schemaVersion', 'parents', 'bindings'} or plan['schemaVersion'] != 1:
        raise ValueError('unsupported supplemental source plan')
    if not isinstance(plan['parents'], dict) or not isinstance(plan['bindings'], dict):
        raise ValueError('invalid supplemental source tables')
    return plan


def repository(value):
    if isinstance(value, dict):
        value = value.get('url')
    if not isinstance(value, str):
        raise ValueError('missing supplemental repository')
    value = value.removeprefix('git+').removesuffix('.git').rstrip('/')
    if not re.fullmatch(r'https://github\.com/[\w.-]+/[\w.-]+', value):
        raise ValueError('unsupported supplemental repository')
    return value


def vcs_evidence(data, prefix):
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        entries = [e for e in archive if e.name == f'{prefix}/.cargo_vcs_info.json']
        if len(entries) != 1 or not entries[0].isfile() or entries[0].size > 8192:
            raise ValueError('missing or unsafe published VCS evidence')
        source = archive.extractfile(entries[0])
        if source is None:
            raise ValueError('missing published VCS stream')
        with source:
            blob = source.read()
    return {'sha256': digest(blob), 'value': json.loads(blob)}


def npm_commit_texts(package, spec):
    """Use a commit embedded in a checksum-locked npm manifest, not a registry tag."""
    if (package['ecosystem'] != 'npm'
            or set(spec) != {'kind', 'integrity', 'manifestBase64', 'manifestSha256',
                             'repository', 'commit', 'coverage', 'texts'}):
        raise ValueError('invalid npm manifest commit binding')
    raw = base64.b64decode(spec['manifestBase64'], validate=True)
    if not raw or len(raw) > 65536:
        raise ValueError('invalid npm manifest proof size')
    manifest = json.loads(raw)
    commit = spec['commit']
    if (not isinstance(commit, str) or not re.fullmatch(r'[0-9a-f]{40}', commit)
            or manifest.get('commit') != commit
            or manifest.get('gitHead', commit) != commit
            or manifest.get('name') != package['name'] or manifest.get('version') != package['version']
            or manifest.get('license') != package['declaredLicense']
            or digest(raw) != spec['manifestSha256']
            or package['origin'].get('manifestSha256') != spec['manifestSha256']
            or package['origin']['integrity'] != spec['integrity']
            or repository(manifest.get('repository')) != repository(package['repository'])
            or repository(spec['repository']) != repository(package['repository'])
            or spec['coverage'] not in ('full-text', 'notice-only') or not spec['texts']):
        raise ValueError('locked npm manifest commit proof mismatch')
    repo = repository(spec['repository']).removeprefix('https://github.com/')
    result = []
    for text in spec['texts']:
        path = PurePosixPath(text['path'])
        url = f'https://raw.githubusercontent.com/{repo}/{commit}/{path}'
        # This binding only covers repository-root notices; sibling packages need separate review.
        if (path.is_absolute() or '..' in path.parts or path.parent != PurePosixPath('.')
                or str(path) != text['path'] or text['url'] != url
                or not re.fullmatch(r'[0-9a-f]{64}', text['sha256'])):
            raise ValueError('npm manifest text must be a pinned repository-root file')
        result.append({'sourcePath': text['path'], 'sha256': text['sha256'],
                       'file': f'third-party/licenses/{text["sha256"]}.txt', 'sourceUrl': url})
    if len({t['sourcePath'] for t in result}) != len(result):
        raise ValueError('duplicate npm supplemental text path')
    return result


MPL_NOTICE = (b'/* This Source Code Form is subject to the terms of the Mozilla Public\n'
              b' * License, v. 2.0. If a copy of the MPL was not distributed with this\n'
              b' * file, You can obtain one at https://mozilla.org/MPL/2.0/. */')
MPL_TEXT_URL = 'https://www.mozilla.org/media/MPL/2.0/index.txt'


def reference_evidence(data, prefix, source_path):
    path = PurePosixPath(source_path)
    if (path.is_absolute() or '..' in path.parts or str(path) != source_path
            or path.suffix != '.rs'):
        raise ValueError('unsafe published reference path')
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        entries = [e for e in archive if e.name == f'{prefix}/{source_path}']
        if len(entries) != 1 or not entries[0].isfile() or entries[0].size > 256 * 1024:
            raise ValueError('missing, duplicate or unsafe published reference')
        source = archive.extractfile(entries[0])
        if source is None:
            raise ValueError('missing published reference stream')
        with source:
            blob = source.read()
        blob.decode('utf-8')
    return {'path': source_path, 'sha256': digest(blob)}


def reference_texts(package, spec):
    if (set(spec) != {'kind', 'archiveSha256', 'proof', 'coverage', 'text'}
            or package['ecosystem'] != 'cargo' or package['declaredLicense'] != 'MPL-2.0'
            or spec['coverage'] != 'full-text'
            or package['origin'].get('archiveSha256') != spec['archiveSha256']):
        raise ValueError('invalid published license reference binding')
    proof, text = spec['proof'], spec['text']
    if (not isinstance(proof, dict) or set(proof) != {'path', 'sha256', 'base64'}
            or not isinstance(text, dict) or set(text) != {'path', 'url', 'sha256'}):
        raise ValueError('invalid reference proof or text')
    path = PurePosixPath(proof['path'])
    blob = base64.b64decode(proof['base64'], validate=True)
    if (len(blob) > 256 * 1024 or not blob.startswith(MPL_NOTICE)
            or path.is_absolute() or '..' in path.parts or str(path) != proof['path']
            or path.suffix != '.rs' or digest(blob) != proof['sha256']
            or package['origin'].get('licenseReference') != {'path': proof['path'], 'sha256': proof['sha256']}
            or text['path'] != 'MPL-2.0.txt' or text['url'] != MPL_TEXT_URL
            or not re.fullmatch(r'[0-9a-f]{64}', text['sha256'])):
        raise ValueError('published reference or official MPL text mismatch')
    blob.decode('utf-8')
    return [{'sourcePath': proof['path'], 'sha256': proof['sha256'],
             'file': f'third-party/licenses/{proof["sha256"]}.txt',
             'sourceArchive': package['origin']['url']},
            {'sourcePath': text['path'], 'sha256': text['sha256'],
             'file': f'third-party/licenses/{text["sha256"]}.txt', 'sourceUrl': text['url']}]


def cargo_workspace_texts(package, spec, plan, index):
    """Reuse root texts only with clean same-commit, published path evidence."""
    if (package['ecosystem'] != 'cargo'
            or set(spec) != {'kind', 'parent', 'archiveSha256', 'vcs', 'vcsSha256'}):
        raise ValueError('invalid cargo workspace parent binding')
    parent = index.get(spec['parent'])
    proof = plan['parents'].get(spec['parent'])
    if (parent is None or proof is None or parent['ecosystem'] != 'cargo'
            or parent['id'] == package['id'] or parent['textStatus'] != 'collected'
            or not parent['licenseTexts']
            or set(proof) != {'archiveSha256', 'manifestBase64', 'manifestSha256', 'vcs', 'vcsSha256'}):
        raise ValueError('workspace parent must be locked with original archive texts')
    raw = base64.b64decode(proof['manifestBase64'], validate=True)
    if len(raw) > 2 * 1024 * 1024:
        raise ValueError('oversized workspace parent manifest')
    manifest = tomllib.loads(raw.decode('utf-8'))
    published = manifest.get('package', {})
    dependency = manifest.get('dependencies', {}).get(package['name'])
    vcs, parent_vcs = spec['vcs'], proof['vcs']
    commit = vcs.get('git', {}).get('sha1', '')
    if (digest(raw) != proof['manifestSha256']
            or parent['origin'].get('manifestSha256') != proof['manifestSha256']
            or parent['origin']['archiveSha256'] != proof['archiveSha256']
            or package['origin']['archiveSha256'] != spec['archiveSha256']
            or package['origin'].get('vcsEvidence') != {'sha256': spec['vcsSha256'], 'value': vcs}
            or parent['origin'].get('vcsEvidence') != {'sha256': proof['vcsSha256'], 'value': parent_vcs}
            or not re.fullmatch(r'[0-9a-f]{40}', commit)
            or parent_vcs.get('git', {}).get('sha1') != commit
            or vcs['git'].get('dirty', False) or parent_vcs['git'].get('dirty', False)
            or published.get('name') != parent['name'] or published.get('version') != parent['version']
            or published.get('license') != parent['declaredLicense']
            or parent['declaredLicense'] != package['declaredLicense']
            or not isinstance(dependency, dict)):
        raise ValueError('workspace parent publication, commit or license mismatch')
    requirement = dependency.get('version', '')
    if (dependency.get('package', package['name']) != package['name']
            or not isinstance(requirement, str)
            or not (requirement == package['version']
                    or re.fullmatch(r'[0-9]+\.[0-9]+', requirement)
                    and requirement + '.0' == package['version'])):
        raise ValueError('workspace dependency version must bind the locked release')
    parent_path = PurePosixPath(parent_vcs.get('path_in_vcs', ''))
    child_path = PurePosixPath(vcs.get('path_in_vcs', ''))
    declared = dependency.get('path')
    if not isinstance(declared, str) or not declared:
        raise ValueError('workspace dependency must declare a package path')
    dependency_path = PurePosixPath(declared)
    for path in (parent_path, child_path, dependency_path):
        if path.is_absolute() or '..' in path.parts:
            raise ValueError('unsafe workspace package path')
    if (str(dependency_path) != declared.removesuffix('/')
            or child_path == parent_path or parent_path / dependency_path != child_path):
        raise ValueError('workspace dependency path differs from published VCS')
    for text in parent['licenseTexts']:
        if PurePosixPath(text['sourcePath']).name != text['sourcePath']:
            raise ValueError('workspace texts must belong to the published parent root')
    return [{**text, 'sourcePackage': parent['id']} for text in parent['licenseTexts']]


def expected_texts(package, spec, plan, index):
    """Validate the binding, returning exact text records without fetching anything."""
    if spec.get('kind') == 'cargo-workspace-parent':
        return cargo_workspace_texts(package, spec, plan, index)
    if spec.get('kind') == 'cargo-license-reference':
        return reference_texts(package, spec)
    if spec.get('kind') == 'npm-manifest-commit':
        return npm_commit_texts(package, spec)
    if spec.get('kind') == 'npm-optional-parent':
        if set(spec) != {'kind', 'parent', 'integrity'} or package['ecosystem'] != 'npm':
            raise ValueError('invalid optional-parent binding')
        parent = index.get(spec['parent'])
        proof = plan['parents'].get(spec['parent'])
        if parent is None or proof is None or parent['textStatus'] != 'collected':
            raise ValueError('optional parent must be locked and have archive texts')
        raw = base64.b64decode(proof['manifestBase64'], validate=True)
        manifest = json.loads(raw)
        if (digest(raw) != proof['manifestSha256']
                or parent['origin'].get('manifestSha256') != proof['manifestSha256']
                or parent['origin']['integrity'] != proof['integrity']
                or package['origin']['integrity'] != spec['integrity']
                or f'npm:{manifest["name"]}@{manifest["version"]}' != parent['id']
                or manifest.get('optionalDependencies', {}).get(package['name']) != package['version']
                or manifest.get('license') != package['declaredLicense']
                or parent['declaredLicense'] != package['declaredLicense']
                or repository(manifest.get('repository')) != repository(package['repository'])
                or repository(parent['repository']) != repository(package['repository'])):
            raise ValueError('optional parent publication or exact dependency mismatch')
        return [{**text, 'sourcePackage': parent['id']} for text in parent['licenseTexts']]
    if spec.get('kind') != 'cargo-vcs' or package['ecosystem'] != 'cargo':
        raise ValueError('unsupported supplemental binding')
    if set(spec) != {'kind', 'archiveSha256', 'repository', 'vcs', 'vcsSha256', 'coverage', 'texts'}:
        raise ValueError('invalid cargo-vcs binding')
    vcs = spec['vcs']
    commit = vcs.get('git', {}).get('sha1', '')
    if (not re.fullmatch(r'[0-9a-f]{40}', commit) or vcs['git'].get('dirty', False)
            or package['origin']['archiveSha256'] != spec['archiveSha256']
            or package['origin'].get('vcsEvidence') != {'sha256': spec['vcsSha256'], 'value': vcs}
            or repository(package['repository']) != repository(spec['repository'])
            or spec['coverage'] not in ('full-text', 'notice-only') or not spec['texts']):
        raise ValueError('published VCS source is dirty, missing or mismatched')
    base = PurePosixPath(vcs.get('path_in_vcs', ''))
    if base.is_absolute() or '..' in base.parts:
        raise ValueError('unsafe VCS package path')
    parents = {base, *base.parents}
    repo = repository(spec['repository']).removeprefix('https://github.com/')
    result = []
    for text in spec['texts']:
        path = PurePosixPath(text['path'])
        url = f'https://raw.githubusercontent.com/{repo}/{commit}/{path}'
        if (path.is_absolute() or '..' in path.parts or str(path) != text['path']
                or path.parent not in parents or text['url'] != url
                or not re.fullmatch(r'[0-9a-f]{64}', text['sha256'])):
            raise ValueError('supplemental path must be a pinned package ancestor')
        result.append({'sourcePath': text['path'], 'sha256': text['sha256'],
                       'file': f'third-party/licenses/{text["sha256"]}.txt', 'sourceUrl': url})
    if len({t['sourcePath'] for t in result}) != len(result):
        raise ValueError('duplicate supplemental text path')
    return result


def apply(root, scratch, packages, files, download):
    plan = load(root)
    index = {p['id']: p for p in packages}
    if not set(plan['bindings']) <= set(index):
        raise ValueError('supplemental targets differ from locked packages')
    wanted = {}
    for identity, spec in plan['bindings'].items():
        package = index[identity]
        if package['textStatus'] != 'missing-from-published-package' or package['licenseTexts']:
            raise ValueError('supplemental binding cannot replace archive texts')
        texts = expected_texts(package, spec, plan, index)
        if spec.get('kind') == 'cargo-license-reference':
            proof = spec['proof']
            files[f'third-party/licenses/{proof["sha256"]}.txt'] = base64.b64decode(proof['base64'], validate=True)
        for text in texts:
            if 'sourceUrl' in text:
                wanted[text['sourceUrl']] = text['sha256']
        package['supplementalSource'] = spec
        package['licenseTexts'] = texts
        package['textStatus'] = ('supplemented-notice-only' if spec.get('coverage') == 'notice-only'
                                 else 'supplemented')

    def fetch(item):
        url, sha256 = item
        blob = download(url, scratch / (digest(url.encode()) + '.upstream'), sha256, 'sha256')
        if not blob or len(blob) > 4 * 1024 * 1024:
            raise ValueError('empty or oversized supplemental text')
        blob.decode('utf-8')
        return f'third-party/licenses/{sha256}.txt', blob

    with ThreadPoolExecutor(max_workers=4) as pool:
        files.update(pool.map(fetch, sorted(wanted.items())))
    validate(packages, plan)


def validate(packages, plan):
    index = {p['id']: p for p in packages}
    supplemented = {p['id'] for p in packages if p['textStatus'].startswith('supplemented')}
    if supplemented != set(plan['bindings']):
        raise ValueError('supplemental component set differs from source plan')
    used_parents = set()
    for identity in sorted(supplemented):
        package, spec = index[identity], plan['bindings'][identity]
        status = 'supplemented-notice-only' if spec.get('coverage') == 'notice-only' else 'supplemented'
        if (package.get('supplementalSource') != spec or package['textStatus'] != status
                or package['licenseTexts'] != expected_texts(package, spec, plan, index)):
            raise ValueError('supplemental attribution differs from pinned source plan')
        if spec['kind'] in ('npm-optional-parent', 'cargo-workspace-parent'):
            used_parents.add(spec['parent'])
    if used_parents != set(plan['parents']):
        raise ValueError('unexpected supplemental parent proof')
