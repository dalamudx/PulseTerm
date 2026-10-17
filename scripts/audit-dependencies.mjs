#!/usr/bin/env node
// Run both dependency gates even when one fails; a report never grants release approval.
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const INPUTS = ['Cargo.lock', 'pnpm-lock.yaml', 'deny.toml', 'Cargo.toml',
  'package.json', 'ui/package.json', 'pnpm-workspace.yaml'];
const CHECKS = [
  { id: 'rust', command: 'cargo', args: ['deny', '--all-features', '--locked', 'check'] },
  { id: 'javascript', command: 'pnpm', args: ['audit', '--audit-level', 'moderate', '--json'] },
];
const LEVELS = ['info', 'low', 'moderate', 'high', 'critical'];

function fingerprints(cwd) {
  return Object.fromEntries(INPUTS.map((name) => [name,
    createHash('sha256').update(readFileSync(join(cwd, name))).digest('hex')]));
}

function vulnerabilities(stdout) {
  const counts = JSON.parse(stdout)?.metadata?.vulnerabilities;
  if (!counts || !LEVELS.every((level) => Number.isSafeInteger(counts[level]) && counts[level] >= 0)) {
    throw new Error('pnpm audit did not return complete, nonnegative severity counts');
  }
  return Object.fromEntries(LEVELS.map((level) => [level, counts[level]]));
}

export function auditDependencies({ cwd = ROOT, outputDir = join(cwd, 'target/probes/dependency-audit'),
  runner = spawnSync, log = console.log } = {}) {
  mkdirSync(outputDir, { recursive: true });
  const summaryPath = join(outputDir, 'summary.json');
  const report = { schemaVersion: 1, completed: false, passed: false, exitCode: 1,
    nodeVersion: process.version, checks: [] };
  const persist = () => writeFileSync(summaryPath, `${JSON.stringify(report, null, 2)}\n`);
  // Invalidate an older passing report before starting a command or reading inputs.
  persist();
  report.inputs = fingerprints(cwd);
  persist();
  for (const check of CHECKS) {
    let result;
    try {
      result = runner(check.command, check.args, { cwd, encoding: 'utf8',
        maxBuffer: 64 * 1024 * 1024,
        // Windows package-manager shims need cmd.exe. All command/args above are constants.
        shell: process.platform === 'win32',
        env: { ...process.env, CARGO_TERM_COLOR: 'never' } });
    } catch (error) {
      result = { status: null, error, stdout: '', stderr: '' };
    }
    const stdout = result.stdout ?? '';
    const stderr = result.stderr ?? '';
    writeFileSync(join(outputDir, `${check.id}.stdout.log`), stdout);
    writeFileSync(join(outputDir, `${check.id}.stderr.log`), stderr);
    const outcome = { id: check.id, command: [check.command, ...check.args],
      exitCode: result.status ?? null, signal: result.signal ?? null,
      passed: result.status === 0 && !result.error && !result.signal,
      stdout: `${check.id}.stdout.log`, stderr: `${check.id}.stderr.log` };
    if (result.error) outcome.executionError = result.error.message;
    if (check.id === 'rust') {
      outcome.findings = {};
      for (const match of `${stdout}\n${stderr}`.matchAll(/^error\[([\w-]+)\]/gm)) {
        outcome.findings[match[1]] = (outcome.findings[match[1]] ?? 0) + 1;
      }
    } else {
      try {
        outcome.vulnerabilities = vulnerabilities(stdout);
        if (result.status === 0 && ['moderate', 'high', 'critical'].some(
          (level) => outcome.vulnerabilities[level] > 0)) {
          throw new Error('pnpm returned success with findings at or above the moderate threshold');
        }
      } catch (error) {
        outcome.passed = false;
        outcome.validationError = error.message;
      }
    }
    report.checks.push(outcome);
    persist();
    log(`[audit] ${check.id}: ${outcome.passed ? 'passed' : 'failed'}; exit=${outcome.exitCode}; logs=${relative(cwd, outputDir)}`);
  }
  report.inputsAfter = fingerprints(cwd);
  report.inputsChanged = JSON.stringify(report.inputs) !== JSON.stringify(report.inputsAfter);
  const failed = report.checks.find((check) => !check.passed);
  report.exitCode = failed ? (Number.isInteger(failed.exitCode) && failed.exitCode > 0 ? failed.exitCode : 1)
    : report.inputsChanged ? 1 : 0;
  report.completed = true;
  report.passed = report.exitCode === 0;
  persist();
  log(`[audit] completed=${report.completed}; passed=${report.passed}; report=${relative(cwd, summaryPath)}`);
  return report;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    process.exitCode = auditDependencies().exitCode;
  } catch (error) {
    console.error(`[audit] incomplete: ${error.message}`);
    process.exitCode = 1;
  }
}
