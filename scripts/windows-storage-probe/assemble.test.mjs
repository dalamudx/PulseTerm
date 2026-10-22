import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { copyLockedPackage } from './assemble.mjs';

test('locked archives: original license/notice copied; digest and standalone text failures rejected', () => {
  if (!process.env.TMPDIR?.startsWith('/data/temp/')) throw new Error('Set TMPDIR below /data/temp');
  const root = fs.mkdtempSync(path.join(process.env.TMPDIR, 'legal-test-'));
  try {
    const source = path.join(root, 'fixture-1.0.0');
    const cache = path.join(root, 'registry/cache/fixture');
    fs.mkdirSync(source); fs.mkdirSync(cache, { recursive: true });
    fs.writeFileSync(path.join(source, 'LICENSE-MIT'), 'standalone test fixture license\n');
    fs.writeFileSync(path.join(source, 'NOTICE'), 'original test fixture notice\n');
    const archive = path.join(cache, 'fixture-1.0.0.crate');
    execFileSync('tar', ['-czf', archive, '-C', root, 'fixture-1.0.0']);
    const digest = createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
    const lock = `version = 4\n\n[[package]]\nname = "fixture"\nversion = "1.0.0"\nchecksum = "${digest}"\n`;
    const artifact = { package_id: 'registry+https://example.invalid#fixture@1.0.0', manifest_path: path.join(root, 'registry/src/fixture/fixture-1.0.0/Cargo.toml') };
    const output = path.join(root, 'output');
    copyLockedPackage(artifact, lock, root, output);
    assert.equal(fs.readFileSync(path.join(output, 'fixture-1.0.0/NOTICE'), 'utf8'), 'original test fixture notice\n');
    assert.throws(() => copyLockedPackage(artifact, lock.replace(digest, '0'.repeat(64)), root, output), /digest mismatch/);
    fs.appendFileSync(archive, 'corruption');
    assert.throws(() => copyLockedPackage(artifact, lock, root, output), /digest mismatch/);
    fs.rmSync(path.join(source, 'LICENSE-MIT'));
    execFileSync('tar', ['-czf', archive, '-C', root, 'fixture-1.0.0']);
    const noLicenseDigest = createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
    assert.throws(() => copyLockedPackage(artifact, lock.replace(digest, noLicenseDigest), root, output), /No standalone license/);
    assert.throws(() => copyLockedPackage(artifact, lock + lock, root, output), /ambiguous/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
