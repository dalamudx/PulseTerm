// Native acceptance receipts are independent of GUI and ordinary-user acceptance.
import fs from 'node:fs';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { groups, definitions } from './test-scope.mjs';
import { verifyGroup } from '../verify-native-tests.mjs';

const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const target = 'x86_64-pc-windows-msvc';

export { verifyGroup };

export function runNative({ root, platform = process.platform, invoke = spawnSync }) {
  const runId = randomUUID();
  const output = path.join(root, 'target/probes/windows-product-native', runId);
  fs.mkdirSync(output, { recursive: true });
  const report = {
    schemaVersion: 2, runId, target, platform, nativeCorePassed: false,
    selectedNativeTests: 23, verifiedNativeTests: 0,
    productionStartupValidated: false, ordinaryUserProductAcceptance: 'pending',
    nativeProcessCompletionConfirmed: true, sourceHashes: {}, groups: [], error: null,
    limits: ['Runner privileges do not establish ordinary-user acceptance', 'GUI, ConPTY, WSL and performance are not exercised by this task'],
  };
  try {
    if (platform !== 'win32') throw new Error('Native Windows is required; this is not an acceptance pass.');
    const tests = definitions(root);
    if (tests.length !== 23 || new Set(tests.map(t => t.name)).size !== 23) throw new Error('Review selected native coverage');
    for (const file of ['Cargo.lock', 'scripts/verify-native-tests.mjs', 'scripts/windows-product-probe/test-native.mjs', 'scripts/windows-product-probe/test-scope.mjs', ...groups.map(g => g.file)]) {
      report.sourceHashes[file] = sha(fs.readFileSync(path.join(root, file)));
    }
    for (const [index, group] of groups.entries()) {
      const stage = `group-${index + 1}`;
      const expected = tests.filter(t => t.name.startsWith(group.prefix)).map(t => t.name);
      const packageName = group.target === 'xtask' ? 'xtask' : group.target.replaceAll('_', '-');
      const args = ['test', '-p', packageName, '--locked', '--target', target, '--color', 'never'];
      if (packageName === 'xtask') args.push('--features', 'windows-settings-probe', '--bin', 'xtask');
      else args.push('--lib');
      args.push(group.prefix, '--', '--test-threads=1', '--color', 'never');
      const entry = { prefix: group.prefix, expected, command: ['cargo', ...args], passed: false, exitCode: null, signal: null, error: null, tests: [], stdout: `${stage}.stdout.log`, stderr: `${stage}.stderr.log` };
      report.groups.push(entry);
      try {
        fs.writeFileSync(path.join(output, entry.stdout), '');
        fs.writeFileSync(path.join(output, entry.stderr), '');
        const result = invoke('cargo', args, { cwd: root, encoding: 'utf8', timeout: 900000, maxBuffer: 16 * 1024 * 1024, shell: false });
        const stdout = result.stdout || '', stderr = result.stderr || '';
        fs.writeFileSync(path.join(output, entry.stdout), stdout);
        fs.writeFileSync(path.join(output, entry.stderr), stderr);
        entry.exitCode = Number.isInteger(result.status) ? result.status : null;
        entry.signal = result.signal || null;
        if (result.error || result.signal || entry.exitCode === null) {
          // No process-tree completion claim after timeout, signal or unknown exit.
          report.nativeProcessCompletionConfirmed = false;
          throw new Error(result.error?.message || `Native exit unavailable: ${entry.signal || 'unknown'}`);
        }
        if (entry.exitCode !== 0) throw new Error(`Native cargo/test exit ${entry.exitCode}`);
        entry.tests = verifyGroup(stdout, expected);
        entry.passed = true;
        report.verifiedNativeTests += entry.tests.length;
      } catch (error) {
        entry.error = error.message;
        if (entry.exitCode === null) report.nativeProcessCompletionConfirmed = false;
      }
      // Unknown native completion must not launch another group using the same fixture root.
      if (!report.nativeProcessCompletionConfirmed) break;
    }
    if (report.groups.length !== groups.length || report.groups.some(g => !g.passed)
        || report.verifiedNativeTests !== 23) throw new Error('Selected native groups did not all pass; inspect raw group logs.');
    report.nativeCorePassed = true;
  } catch (error) { report.error = error.message; }
  finally {
    const text = JSON.stringify(report, null, 2) + '\n';
    fs.writeFileSync(path.join(output, 'summary.json'), text);
    // Replace the latest receipt on failures too; old successful receipts remain in their own run directory.
    fs.writeFileSync(path.join(root, 'target/probes/windows-product-native.json'), text);
  }
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
  const report = runNative({ root });
  console.log(JSON.stringify({ runId: report.runId, nativeCorePassed: report.nativeCorePassed, verifiedNativeTests: report.verifiedNativeTests, error: report.error, receipt: 'target/probes/windows-product-native.json' }));
  if (!report.nativeCorePassed) process.exitCode = 1;
}
