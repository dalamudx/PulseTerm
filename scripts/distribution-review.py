"""Resolve candidate desktop dependency scopes; never approve a release."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PROFILES = (
    ('linux-x64', 'x86_64-unknown-linux-gnu', 'workspace-cli,workspace-ssh', 'candidate'),
    ('windows-msvc-x64', 'x86_64-pc-windows-msvc', 'workspace-ssh', 'candidate'),
    ('macos-arm64', 'aarch64-apple-darwin', 'workspace-ssh', 'candidate'),
    ('macos-x64', 'x86_64-apple-darwin', 'workspace-ssh', 'candidate'),
    ('windows-gnu-comparison', 'x86_64-pc-windows-gnu', 'workspace-ssh', 'comparison-only'),
)
EDGES = {'normal': 'normal,no-proc-macro', 'combined': 'normal,build'}
PATTERN = re.compile(r'([A-Za-z0-9_-]+) v([^\s()]+)(?: \([^\r\n]*\))?(?: \(\*\))?')
COMPLETE = ('collected', 'supplemented')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def manifest_inputs(root):
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())
    paths = ['Cargo.toml', 'Cargo.lock', 'pnpm-lock.yaml', 'ui/package.json',
             'apps/desktop/src-tauri/workspace/tauri.conf.json', 'third-party/inventory.json',
             'scripts/distribution-review.py', '.mise.toml']
    project = set()
    for member in workspace['workspace']['members']:
        path = (root / member / 'Cargo.toml').resolve()
        relative = path.relative_to(root).as_posix()
        package = tomllib.loads(path.read_text())['package']
        version = package['version']
        if isinstance(version, dict):
            if version != {'workspace': True}:
                raise ValueError('unsupported inherited package version')
            version = workspace['workspace']['package']['version']
        project.add(f'cargo:{package["name"]}@{version}')
        paths.append(relative)
    paths.extend(p.relative_to(root).as_posix() for p in (root / 'vendor').glob('**/Cargo.toml'))
    return {name: sha((root / name).read_bytes()) for name in sorted(set(paths))}, project


def parse_tree(text, known, project):
    result = set()
    for line in text.splitlines():
        if line == '[build-dependencies]':
            continue
        match = PATTERN.fullmatch(line)
        if not match:
            raise ValueError('unrecognized Cargo tree output')
        component = f'cargo:{match[1]}@{match[2]}'
        if component not in known and component not in project:
            raise ValueError(f'component absent from validated inventory: {component}')
        result.add(component)
    desktop = {p for p in project if p.startswith('cargo:pulseterm-desktop@')}
    if len(desktop) != 1 or not desktop <= result:
        raise ValueError('Cargo tree is empty or lacks the desktop root')
    return result


def checked_inventory(root):
    spec = importlib.util.spec_from_file_location('distribution_attribution', root / 'scripts/third-party.py')
    if spec is None or spec.loader is None:
        raise ValueError('attribution validator unavailable')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.validate(root)


def run(command, root):
    deadline = 300 if command[:2] == ['cargo', 'fetch'] else 60
    try:
        result = subprocess.run(command, cwd=root, text=True, capture_output=True, timeout=deadline, check=True)
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f'{error}\n{error.stderr}') from error
    if len(result.stdout.encode()) > 8 * 1024 * 1024:
        raise ValueError('dependency graph output exceeds budget')
    return result.stdout


def review(root, runner=run, validate=checked_inventory):
    inventory = validate(root)
    before, project = manifest_inputs(root)
    known = {p['id']: p for p in inventory['packages']}
    if len(known) != len(inventory['packages']) or inventory.get('releaseApproved') is not False:
        raise ValueError('invalid or falsely approved attribution inventory')
    ui = [p for p in inventory['packages'] if p['ecosystem'] == 'npm']
    assets = [p for p in inventory['packages'] if p['ecosystem'] not in ('npm', 'cargo')]
    if any(p['ecosystem'] != 'asset' for p in assets):
        raise ValueError('unrecognized attribution ecosystem')
    retained = {p['id'] for p in [*ui, *assets]}
    raw = {}
    profiles = []
    toolchain = runner(['cargo', '--version'], root).strip()

    rust = runner(['rustc', '-vV'], root)
    hosts = re.findall(r'^host: (\S+)$', rust, flags=re.MULTILINE)
    if len(hosts) != 1:
        raise ValueError('resolver host not reported by rustc')
    runner(['cargo', 'fetch', '--locked'], root)
    for name, target, features, purpose in PROFILES:
        graphs = {}
        for mode, edges in EDGES.items():
            command = ['cargo', 'tree', '--frozen', '-p', 'pulseterm-desktop', '--no-default-features',
                       '--features', features, '--target', target, '--edges', edges,
                       '--prefix', 'none', '--color', 'never', '--format', '{p}']
            text = runner(command, root)
            graphs[mode] = parse_tree(text, known, project)
            raw[f'{name}-{mode}.txt'] = text
        if not graphs['normal'] <= graphs['combined']:
            raise ValueError('normal-edge graph is not contained in the combined graph')
        selected = (graphs['combined'] - project) | retained
        profiles.append({'name': name, 'target': target, 'features': features.split(','), 'purpose': purpose,
                         'normalNonProcMacroCargo': sorted(graphs['normal'] - project),
                         'additionalBuildOrProcMacroCargo': sorted(graphs['combined'] - graphs['normal'] - project),
                         'projectPackages': sorted(graphs['combined'] & project),
                         'allSelectedComponents': sorted(selected),
                         'incompleteTextComponents': sorted(p for p in selected if known[p]['textStatus'] not in COMPLETE)})
    validate(root)
    bundle = json.loads((root / 'apps/desktop/src-tauri/workspace/tauri.conf.json').read_bytes())
    after, _ = manifest_inputs(root)
    if before != after:
        raise ValueError('review inputs changed during graph resolution')
    all_missing = sorted(p['id'] for p in inventory['packages'] if p['textStatus'] not in COMPLETE)
    return {'schemaVersion': 1, 'completed': True, 'releaseApproved': False,
            'scope': 'Cargo target-filtered candidate graphs on the current resolver host; not a binary SBOM',
            'cargoVersion': toolchain, 'resolverHost': hosts[0], 'inputs': before, 'profiles': profiles,
            'frontendScope': 'all locked npm components retained; package.json devDependencies do not prove absence from UI assets',
            'retainedNpm': sorted(p['id'] for p in ui), 'retainedAssets': sorted(p['id'] for p in assets),
            'workspaceMissingTexts': all_missing, 'workspaceBundleActive': bundle['bundle']['active'],
            'unresolvedDistributionReview': ['actual artifact contents and links', 'system and dynamic libraries',
                                             'native target build and resolver-host differences',
                                             'license policy and maintenance audit', 'signing and packaging'],
            'rawGraphSha256': {name: sha(text.encode()) for name, text in sorted(raw.items())}}, raw


def markdown(report):
    lines = ['# Candidate distribution dependency review', '',
             'Resolution completed; releaseApproved=false. This is not a binary SBOM or a release permit.', '',
             '| Profile | Normal, excluding proc macros | Additional build/proc macro | Conservatively retained npm/assets | Incomplete texts in selected scope |',
             '|---|---:|---:|---:|---:|']
    retained = len(report['retainedNpm']) + len(report['retainedAssets'])
    for p in report['profiles']:
        lines.append(f'| {p["name"]} | {len(p["normalNonProcMacroCargo"])} | {len(p["additionalBuildOrProcMacroCargo"])} | {retained} | {len(p["incompleteTextComponents"])} |')
    lines.extend(['', f'Workspace text gate remains unchanged: {len(report["workspaceMissingTexts"])} incomplete components.',
                  'Windows GNU is a comparison, not an adopted distribution toolchain. Non-Linux CLI is not selected.',
                  'Foreign target resolution is not foreign native build evidence. Cargo graphs also contain compile-time code.',
                  'All npm packages and assets are retained. Bundler output has not been pruned or approved.',
                  'Actual binary links, system libraries, license acceptance, maintenance, packaging and signing remain unreviewed.', ''])
    return '\n'.join(lines)


def main():
    output = ROOT / 'target/probes/distribution-review.json'
    summary = output.with_suffix('.md')
    output.parent.mkdir(parents=True, exist_ok=True)
    incomplete = {'schemaVersion': 1, 'completed': False, 'releaseApproved': False}
    output.write_text(json.dumps(incomplete, indent=2) + '\n')
    summary.write_text('Dependency review incomplete; no release approval.\n')
    temp_root = Path(tempfile.gettempdir())
    temp_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='distribution-review-', dir=temp_root) as scratch:
        prior = {key: os.environ.get(key) for key in ('TMPDIR', 'TMP', 'TEMP')}
        os.environ.update({key: scratch for key in prior})
        try:
            report, raw = review(ROOT)
            graphs = output.parent / 'distribution-review-graphs'
            graphs.mkdir(exist_ok=True)
            for name, text in raw.items():
                (graphs / name).write_text(text)
            summary.write_text(markdown(report))
            output.write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
            print(json.dumps({'completed': True, 'releaseApproved': False,
                              'profiles': len(report['profiles']), 'workspaceMissingTexts': len(report['workspaceMissingTexts'])}))
        except Exception as error:
            output.write_text(json.dumps({**incomplete, 'failure': str(error)}, indent=2) + '\n')
            summary.write_text('Dependency review failed; see JSON failure. No release approval.\n')
            raise
        finally:
            for key, value in prior.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value


if __name__ == '__main__':
    main()
