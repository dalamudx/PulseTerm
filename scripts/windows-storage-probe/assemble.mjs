// Build-time packaging only. Locked archive/license failures block the ZIP.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const sha = bytes => createHash('sha256').update(bytes).digest('hex');

export function copyLockedPackage(artifact, lock, cargoHome, destination) {
  const match = artifact.package_id.match(/#([^#@]+)@([^#@]+)$/);
  if (!match) throw new Error('Unrecognized package identity');
  const [, name, version] = match;
  const entries = lock.split(/\n\[\[package\]\]\n/).filter(s => s.split('\n').includes(`name = "${name}"`) && s.split('\n').includes(`version = "${version}"`));
  if (entries.length !== 1) throw new Error(`Locked package ambiguous: ${name}`);
  const checksum = entries[0].match(/^checksum = "([a-f0-9]{64})"$/m)?.[1];
  const registry = path.basename(path.dirname(path.dirname(artifact.manifest_path)));
  const archive = path.join(cargoHome, 'registry/cache', registry, `${name}-${version}.crate`);
  if (!checksum || sha(fs.readFileSync(archive)) !== checksum) throw new Error(`Locked archive digest mismatch: ${name}`);
  const prefix = `${name}-${version}/`;
  const members = execFileSync('tar', ['-tzf', archive], { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 }).split('\n');
  const licenses = members.filter(s => s.startsWith(prefix) && !s.slice(prefix.length).includes('/') && /^(LICENSE|LICENCE|COPYING|NOTICE|COPYRIGHT)([.\-_]|$)/i.test(s.slice(prefix.length)));
  if (!licenses.some(s => /^(LICENSE|LICENCE|COPYING)([.\-_]|$)/i.test(s.slice(prefix.length)))) throw new Error(`No standalone license in locked archive: ${name}`);
  const target = path.join(destination, `${name}-${version}`);
  fs.mkdirSync(target, { recursive: true });
  for (const member of licenses) {
    const bytes = execFileSync('tar', ['-xOzf', archive, member], { maxBuffer: 2 * 1024 * 1024 });
    if (!bytes.length) throw new Error(`Empty license or notice: ${member}`);
    fs.writeFileSync(path.join(target, member.slice(prefix.length)), bytes);
  }
  return { name, version, checksum, source: artifact.package_id };
}

export function assemble(root, artifactsFile, kit, compilerDocs) {
  const artifacts = fs.readFileSync(artifactsFile, 'utf8').split('\n').filter(Boolean).map(JSON.parse).filter(a => a.reason === 'compiler-artifact');
  const tests = artifacts.filter(a => a.target.name === 'pulse_platform' && a.profile.test && a.executable?.endsWith('.exe'));
  if (tests.length !== 1) throw new Error('Expected one Windows platform test executable');
  fs.mkdirSync(kit, { recursive: true });
  fs.copyFileSync(tests[0].executable, path.join(kit, 'platform-tests.exe'));
  const nativeTests = [];
  for (const [file, prefix] of [
    ['tests.rs', 'windows::tests::'],
    ['private_file_tests.rs', 'windows::private_file_tests::'],
    ['atomic_file_tests.rs', 'windows::private_file::atomic_file::tests::'],
  ]) {
    const text = fs.readFileSync(path.join(root, 'crates/pulse-platform/src/windows', file), 'utf8');
    for (const match of text.matchAll(/#\[test\]\s*fn\s+(\w+)\s*\(/g)) nativeTests.push(prefix + match[1]);
  }
  if (nativeTests.length !== 22 || new Set(nativeTests).size !== nativeTests.length) throw new Error('Review changes to native test coverage before packaging');
  fs.writeFileSync(path.join(kit, 'native-tests.json'), JSON.stringify({ schemaVersion: 1, target: 'x86_64-pc-windows-gnu', nativeTests: nativeTests.sort() }, null, 2) + '\n');
  const legal = path.join(kit, 'third-party');
  fs.mkdirSync(legal, { recursive: true });
  const cargoHome = process.env.CARGO_HOME || path.join(process.env.HOME, '.cargo');
  const packages = new Map(artifacts.filter(a => a.package_id.startsWith('registry+')).map(a => [a.package_id, a]));
  const lock = fs.readFileSync(path.join(root, 'Cargo.lock'), 'utf8');
  const scope = [...packages.values()].map(a => copyLockedPackage(a, lock, cargoHome, legal));
  const rustRoot = execFileSync('rustc', ['--print', 'sysroot'], { encoding: 'utf8' }).trim();
  const rustDocs = path.join(rustRoot, 'share/doc/rust');
  for (const file of ['COPYRIGHT.html', 'COPYRIGHT-library.html']) fs.copyFileSync(path.join(rustDocs, file), path.join(legal, `Rust-${file}`));
  fs.cpSync(path.join(rustDocs, 'licenses'), path.join(legal, 'rust-licenses'), { recursive: true });
  for (const [source, target] of [['gcc-mingw-w64-base/copyright', 'NOTICE-GCC-Runtime.txt'], ['mingw-w64-common/copyright', 'NOTICE-MinGW.txt']]) fs.copyFileSync(path.join(compilerDocs, source), path.join(legal, target));
  fs.copyFileSync('/usr/share/common-licenses/GPL-3', path.join(legal, 'GPL-3.txt'));
  fs.copyFileSync(path.join(root, 'LICENSE-MIT'), path.join(kit, 'LICENSE-PulseTerm-MIT.txt'));
  const script = path.join(root, 'scripts/windows-storage-probe');
  for (const file of ['run.ps1', 'README.md']) fs.copyFileSync(path.join(script, file), path.join(kit, file));
  const sourceHashes = {};
  for (const relative of ['Cargo.lock', '.mise.toml', 'crates/pulse-platform/Cargo.toml', ...fs.readdirSync(path.join(root, 'crates/pulse-platform/src'), { recursive: true }).filter(p => p.endsWith('.rs')).map(p => `crates/pulse-platform/src/${p}`)]) sourceHashes[relative] = sha(fs.readFileSync(path.join(root, relative)));
  const build = { schemaVersion: 1, runnerVersion: 3, target: 'x86_64-pc-windows-gnu', rustc: execFileSync('rustc', ['--version'], { encoding: 'utf8' }).trim(), revision: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(), productionStartupValidated: false, nativeWindowsExecution: 'pending', sourceHashes, thirdPartyScope: 'Conservative compilation scope including build-time crates; not a claim of exact shipped dependencies', packages: scope };
  fs.writeFileSync(path.join(kit, 'build-info.json'), JSON.stringify(build, null, 2) + '\n');
  const files = fs.readdirSync(kit, { recursive: true }).filter(p => p !== 'SHA256SUMS' && fs.statSync(path.join(kit, p)).isFile()).sort();
  fs.writeFileSync(path.join(kit, 'SHA256SUMS'), files.map(file => `${sha(fs.readFileSync(path.join(kit, file)))}  ${file}`).join('\n') + '\n');
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) assemble(...process.argv.slice(2));
