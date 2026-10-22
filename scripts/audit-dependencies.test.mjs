import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { auditDependencies } from './audit-dependencies.mjs';

const inputs = ['Cargo.lock', 'pnpm-lock.yaml', 'deny.toml', 'Cargo.toml',
  'package.json', 'ui/package.json', 'pnpm-workspace.yaml'];
const counts = (overrides = {}) => ({ info: 0, low: 0, moderate: 0, high: 0, critical: 0, ...overrides });

function fixture(t) {
  mkdirSync('/data/temp', { recursive: true });
  const cwd = mkdtempSync('/data/temp/audit-gate-test.');
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  mkdirSync(join(cwd, 'ui'));
  for (const name of inputs) writeFileSync(join(cwd, name), `fixture ${name}\n`);
  const executable = join(cwd, 'audit-fixture.mjs');
  writeFileSync(executable, `
const row = JSON.parse(process.argv[2]);
if (row.signal) process.kill(process.pid, row.signal);
process.stdout.write(row.stdout ?? '');
process.stderr.write(row.stderr ?? '');
process.exitCode = row.code;
`);
  const outputDir = join(cwd, 'reports');
  const calls = [];
  function run(rows, before) {
    return auditDependencies({ cwd, outputDir, log() {}, runner(command, args) {
      calls.push([command, ...args]);
      const inFlight = JSON.parse(readFileSync(join(outputDir, 'summary.json'), 'utf8'));
      assert.equal(inFlight.completed, false);
      assert.equal(inFlight.passed, false);
      assert.deepEqual(JSON.parse(readFileSync(join(outputDir, inFlight.runDirectory, 'summary.json'), 'utf8')), inFlight);
      before?.(command);
      const row = rows[command];
      if (row.launchError) throw Object.assign(new Error('fixture executable missing'), { code: 'ENOENT' });
      return spawnSync(process.execPath, [executable, JSON.stringify(row)], { encoding: 'utf8', cwd });
    } });
  }
  return { cwd, outputDir, calls, run };
}

const pnpm = (code = 0, overrides = {}) => ({ code, stdout: JSON.stringify({ metadata: { vulnerabilities: counts(overrides) } }) });

test('Rust policy failure still runs JavaScript and preserves both actual exit receipts', (t) => {
  const f = fixture(t);
  const report = f.run({ cargo: { code: 5, stderr: 'error[rejected]: fixture license\n' }, pnpm: pnpm() });
  assert.equal(report.exitCode, 5);
  assert.equal(report.completed, true);
  assert.equal(report.passed, false);
  assert.deepEqual(f.calls, [
    ['cargo', 'deny', '--all-features', '--locked', 'check'],
    ['pnpm', 'audit', '--audit-level', 'moderate', '--json'],
  ]);
  assert.equal(report.checks[0].findings.rejected, 1);
  assert.equal(report.checks[1].passed, true);
  assert.equal(readFileSync(join(f.outputDir, report.checks[0].stderr), 'utf8'), 'error[rejected]: fixture license\n');
  assert.deepEqual(JSON.parse(readFileSync(join(f.outputDir, 'summary.json'), 'utf8')), report);
});

test('JavaScript security failure cannot be hidden by a successful Rust audit', (t) => {
  const report = fixture(t).run({ cargo: { code: 0 }, pnpm: pnpm(1, { high: 2 }) });
  assert.equal(report.checks[0].passed, true);
  assert.equal(report.checks[1].vulnerabilities.high, 2);
  assert.equal(report.exitCode, 1);
  assert.equal(report.passed, false);
});

test('missing executable replaces stale success and cannot skip the other audit', (t) => {
  const f = fixture(t);
  assert.equal(f.run({ cargo: { code: 0 }, pnpm: pnpm() }).passed, true);
  const report = f.run({ cargo: { launchError: true }, pnpm: pnpm() });
  assert.equal(report.checks[0].exitCode, null);
  assert.match(report.checks[0].executionError, /missing/);
  assert.equal(report.checks[1].passed, true);
  assert.equal(report.passed, false);
  assert.equal(readFileSync(join(f.outputDir, report.checks[0].stdout), 'utf8'), '');
});

test('successful pnpm exit with empty, malformed, incomplete or inconsistent data fails closed', (t) => {
  const f = fixture(t);
  for (const stdout of ['', 'not json', '{}', JSON.stringify({ metadata: { vulnerabilities: counts({ high: 1 }) } }),
    JSON.stringify({ metadata: { vulnerabilities: counts({ low: -1 }) } })]) {
    const report = f.run({ cargo: { code: 0 }, pnpm: { code: 0, stdout } });
    assert.equal(report.checks[1].exitCode, 0);
    assert.equal(typeof report.checks[1].validationError, 'string');
    assert.equal(report.exitCode, 1);
    assert.equal(report.passed, false);
  }
});

test('below-threshold findings retain the existing moderate policy', (t) => {
  const report = fixture(t).run({ cargo: { code: 0 }, pnpm: pnpm(0, { low: 3 }) });
  assert.equal(report.passed, true);
  assert.equal(report.checks[1].vulnerabilities.low, 3);
});

test('changed policy or lock inputs invalidate success from both commands', (t) => {
  const f = fixture(t);
  const report = f.run({ cargo: { code: 0 }, pnpm: pnpm() }, (command) => {
    if (command === 'pnpm') writeFileSync(join(f.cwd, 'deny.toml'), 'changed during audit\n');
  });
  assert.equal(report.checks.every((check) => check.passed), true);
  assert.equal(report.inputsChanged, true);
  assert.equal(report.passed, false);
  assert.equal(report.exitCode, 1);
});

test('child termination cannot be represented as a successful audit', { skip: process.platform === 'win32' }, (t) => {
  const report = fixture(t).run({ cargo: { code: 0, signal: 'SIGTERM' }, pnpm: pnpm() });
  assert.equal(report.checks[0].exitCode, null);
  assert.equal(report.checks[0].signal, 'SIGTERM');
  assert.equal(report.checks[1].passed, true);
  assert.equal(report.passed, false);
});


test('later success keeps the previous failed receipt and all raw outputs byte-for-byte', (t) => {
  const f = fixture(t);
  const first = f.run({ cargo: { code: 1, stdout: 'first stdout\n', stderr: 'error[unmaintained]: first\n' }, pnpm: pnpm() });
  const paths = [join(first.runDirectory, 'summary.json'), ...first.checks.flatMap(check => [check.stdout, check.stderr])];
  const saved = paths.map(name => readFileSync(join(f.outputDir, name)));
  const second = f.run({ cargo: { code: 0, stdout: 'second stdout\n' }, pnpm: pnpm() });
  assert.equal(second.passed, true);
  assert.notEqual(second.runId, first.runId);
  paths.forEach((name, index) => assert.deepEqual(readFileSync(join(f.outputDir, name)), saved[index]));
  assert.deepEqual(JSON.parse(readFileSync(join(f.outputDir, second.runDirectory, 'summary.json'), 'utf8')), second);
  assert.deepEqual(JSON.parse(readFileSync(join(f.outputDir, 'summary.json'), 'utf8')), second);
});

test('missing inputs leave an incomplete current run and preserve the previous success archive', (t) => {
  const f = fixture(t);
  const first = f.run({ cargo: { code: 0 }, pnpm: pnpm() });
  rmSync(join(f.cwd, 'Cargo.lock'));
  assert.throws(() => f.run({}), /ENOENT/);
  const latest = JSON.parse(readFileSync(join(f.outputDir, 'summary.json'), 'utf8'));
  assert.notEqual(latest.runId, first.runId);
  assert.equal(latest.completed, false);
  assert.equal(latest.passed, false);
  assert.equal(latest.checks.length, 0);
  assert.deepEqual(JSON.parse(readFileSync(join(f.outputDir, latest.runDirectory, 'summary.json'), 'utf8')), latest);
  assert.deepEqual(JSON.parse(readFileSync(join(f.outputDir, first.runDirectory, 'summary.json'), 'utf8')), first);
});

test('run directory creation failure cannot leave a current passing receipt', (t) => {
  const f = fixture(t);
  mkdirSync(f.outputDir);
  writeFileSync(join(f.outputDir, 'summary.json'), JSON.stringify({ passed: true, completed: true }));
  writeFileSync(join(f.outputDir, 'runs'), 'fixture blocks directory creation');
  assert.throws(() => f.run({}), /EEXIST/);
  const latest = JSON.parse(readFileSync(join(f.outputDir, 'summary.json'), 'utf8'));
  assert.equal(latest.passed, false);
  assert.equal(latest.completed, false);
  assert.equal(f.calls.length, 0);
});
