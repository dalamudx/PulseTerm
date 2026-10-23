#!/usr/bin/env python3
"""Build an isolated SSH test server that transmits handler partial success.

russh 0.63.3 overwrites this bit for passwords and signed public keys. Correct
only the selected fixture branch; product, registry cache and repository lockfile
remain untouched. The archive must match the repository locked digest.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

root = Path(__file__).resolve().parents[1]
output = Path(sys.argv[1])
temp_root = Path(os.environ['TMPDIR']).resolve(strict=True)
assert temp_root.is_relative_to(Path('/data/temp')) and temp_root != Path('/data/temp')
assert output.parent.resolve(strict=True).is_relative_to(temp_root) and not output.is_symlink()
lock_bytes = (root / 'Cargo.lock').read_bytes()
locked = next(p for p in tomllib.loads(lock_bytes.decode())['package']
              if p['name'] == 'russh' and p['version'] == '0.63.3')
cache = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo'))) / 'registry' / 'cache'
archives = list(cache.glob('*/russh-0.63.3.crate'))
archive = next((p for p in archives if hashlib.sha256(p.read_bytes()).hexdigest() == locked['checksum']), None)
assert archive is not None, 'locked russh archive unavailable or digest mismatch'
old = ('                                    auth_request.partial_success = partial_success;\n'
       '                                }\n'
       '                                auth_request.partial_success = false;\n'
       '                                auth_user.clear();')
new = ('                                    auth_request.partial_success = partial_success;\n'
       '                                } else {\n'
       '                                    auth_request.partial_success = false;\n'
       '                                }\n'
       '                                auth_user.clear();')
assert len(sys.argv) in (2, 3, 4), 'expected binary, optional provenance, optional method'
mode = sys.argv[3] if len(sys.argv) == 4 else 'public-key'
assert mode in ('public-key', 'password'), 'unsupported partial-success method'
correction = 'Preserve handler partial_success for a verified signed public key; reset on ordinary rejection.'
if mode == 'password':
    old = ('                    } else {\n'
           '                        auth_request.methods.remove(MethodKind::Password);\n'
           '                    }\n'
           '                    auth_request.partial_success = false;\n')
    new = ('                    } else {\n'
           '                        auth_request.methods.remove(MethodKind::Password);\n'
           '                        auth_request.partial_success = false;\n'
           '                    }\n')
    correction = 'Preserve handler partial_success for password; reset on ordinary rejection.'
with tempfile.TemporaryDirectory(prefix='joint-server.', dir=temp_root) as directory:
    work = Path(directory)
    with tarfile.open(archive) as package:
        package.extractall(work, filter='data')
    sdk = work / 'russh-0.63.3'
    server = sdk / 'src' / 'server' / 'encrypted.rs'
    original = server.read_text()
    assert original.count(old) == 1, 'locked partial-success server branch changed'
    corrected = original.replace(old, new, 1)
    server.write_text(corrected)
    manifest = (root / 'Cargo.toml').read_text()
    for key in ('members', 'default-members'):
        manifest, count = re.subn(r'^' + key + r' = .*$', key + ' = ["xtask"]', manifest, flags=re.M)
        assert count == 1
    manifest = manifest.replace('path = "crates/', 'path = "' + str(root / 'crates') + '/')
    manifest = manifest.replace('path = "vendor/', 'path = "' + str(root / 'vendor') + '/')
    marker = '[patch.crates-io]\n'
    assert manifest.count(marker) == 1
    manifest = manifest.replace(marker, marker + 'russh = { path = ' + json.dumps(str(sdk)) + ' }\n', 1)
    (work / 'Cargo.toml').write_text(manifest)
    (work / 'Cargo.lock').write_bytes(lock_bytes)
    task = work / 'xtask'
    task.mkdir()
    task_manifest = re.sub(
        r'(\bpath\s*=\s*)"([^"]+)"',
        lambda match: match[1] + json.dumps(str((root / 'xtask' / match[2]).resolve(strict=True))),
        (root / 'xtask' / 'Cargo.toml').read_text(),
    )
    (task / 'Cargo.toml').write_text(task_manifest +
        '\n[[bin]]\nname = "xtask"\npath = ' + json.dumps(str(root / 'xtask' / 'src' / 'main.rs')) + '\n')
    subprocess.run(['cargo', 'build', '--offline', '--manifest-path', str(task / 'Cargo.toml'),
                    '--features', 'ssh-target-probe', '--target-dir', str(work / 'target')],
                   cwd=root, check=True, timeout=900)
    shutil.copy2(work / 'target' / 'debug' / 'xtask', output)
    provenance = {
        'fixtureOnly': True,
        'productionDependencyModified': False,
        'crate': 'russh', 'version': locked['version'],
        'archiveSha256': locked['checksum'],
        'originalServerSha256': hashlib.sha256(original.encode()).hexdigest(),
        'fixtureServerSha256': hashlib.sha256(corrected.encode()).hexdigest(),
        'correction': correction,
        'partialSuccessMethod': mode,
        'binarySha256': hashlib.sha256(output.read_bytes()).hexdigest(),
    }
    evidence = Path(sys.argv[2]) if len(sys.argv) >= 3 else root / 'target' / 'probes' / 'key-interactive-test-server.json'
    assert evidence.resolve().is_relative_to(root / "target" / "probes") and not evidence.is_symlink()
    evidence.parent.mkdir(parents=True, exist_ok=True)
    evidence.write_text(json.dumps(provenance, indent=2) + '\n')
assert (root / 'Cargo.lock').read_bytes() == lock_bytes, 'repository lockfile changed during fixture build'
print('fixture.isolated_partial_server_ready')
