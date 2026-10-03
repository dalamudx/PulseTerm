// All original published files must remain byte-identical except the single getter.
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, cpSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
const root = fileURLToPath(new URL('../vendor/alacritty_terminal/', import.meta.url));
const baseline = JSON.parse(readFileSync(new URL('../vendor/alacritty_terminal.upstream.json', import.meta.url), 'utf8'));
const patch = readFileSync(new URL('../vendor/alacritty_terminal.patch', import.meta.url), 'utf8');
const additions = patch.split('\n').filter(line => line.startsWith('+') && !line.startsWith('+++')).map(line => line.slice(1)).join('\n') + '\n';
assert.ok(additions.includes('pub fn inactive_grid(&self) -> &Grid<Cell>'));
const actual = [];
function scan(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) scan(full);
    else actual.push(path.relative(root, full).split(path.sep).join('/'));
  }
}
scan(root);
assert.deepEqual(actual.sort(), Object.keys(baseline.files).sort(), 'vendor file set changed');
for (const [name, digest] of Object.entries(baseline.files)) {
  let bytes = readFileSync(path.join(root, name));
  if (name === 'src/term/mod.rs') {
    const text = bytes.toString('utf8');
    assert.equal(text.split(additions).length, 2, 'expected exactly one getter patch');
    bytes = Buffer.from(text.replace(additions, ''));
  }
  assert.equal(createHash('sha256').update(bytes).digest('hex'), digest, `unexpected upstream change: ${name}`);
}
console.log(`Verified ${baseline.crate} ${baseline.version}: ${actual.length} published files, only inactive_grid getter differs`);
if (process.argv.includes('--test')) {
  // Excluded dependencies cannot be tested with cargo -p. Keep the package
  // outside the workspace; its own test lock never alters the product lock.
  const scratch = mkdtempSync(path.join(tmpdir(), 'pulseterm-alacritty-tests-'));
  try {
    cpSync(root, scratch, { recursive: true });
    writeFileSync(path.join(scratch, 'Cargo.lock'), readFileSync(new URL('../vendor/alacritty_terminal.test.lock', import.meta.url)));
    const result = spawnSync('cargo', ['test', '--manifest-path', path.join(scratch, 'Cargo.toml'), '--locked'], {
      stdio: 'inherit', timeout: 180_000,
      env: { ...process.env, CARGO_TARGET_DIR: fileURLToPath(new URL('../target/vendor-test-build', import.meta.url)) },
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`Upstream tests failed: ${result.status ?? result.signal}`);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}
