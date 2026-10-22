import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { runNative, verifyGroup } from './test-native.mjs';
import { definitions, groups } from './test-scope.mjs';

const checkout = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
function success(names, ending = '\n') {
  return [...names.map(name => `test ${name} ... ok`), `test result: ok. ${names.length} passed; 0 failed; 0 ignored; 0 measured; 100 filtered out; finished in 0.01s`].join(ending) + ending;
}
function fixture(t) {
  const parent = process.env.TMPDIR || process.env.TEMP;
  assert.ok(parent && path.isAbsolute(parent), 'Set a task-owned absolute TMPDIR/TEMP');
  const root = fs.mkdtempSync(path.join(parent, 'native-receipt-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const file of ['Cargo.lock', 'scripts/verify-native-tests.mjs', 'scripts/windows-product-probe/test-native.mjs', 'scripts/windows-product-probe/test-scope.mjs', ...groups.map(g => g.file)]) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.copyFileSync(path.join(checkout, file), path.join(root, file));
  }
  const names = definitions(root).map(t => t.name);
  const invoke = (_command, args, options) => {
    assert.equal(options.shell, false);
    assert.equal(args[args.indexOf('--target') + 1], 'x86_64-pc-windows-msvc');
    const prefix = args.find(arg => groups.some(g => g.prefix === arg));
    return { status: 0, signal: null, stdout: success(names.filter(name => name.startsWith(prefix)), '\r\n'), stderr: 'fixture build stderr 中文\n' };
  };
  return { root, invoke };
}

const expected = ['module::one', 'module::two'];
test('exact identities accept both Rust LF and Windows CRLF', () => {
  for (const ending of ['\n', '\r\n']) assert.deepEqual(verifyGroup(success(expected, ending), expected), expected);
});
test('matching counts cannot hide missing, duplicate or unrelated tests', () => {
  for (const names of [['module::one', 'module::other'], ['module::one', 'module::one'], []]) {
    assert.throws(() => verifyGroup(success(names), expected));
  }
});
test('zero, ignored, failed, measured and duplicate summaries reject', () => {
  const output = success(expected);
  for (const broken of [
    output.replace('test module::one ... ok', 'test module::one ... ignored'),
    output.replace('test module::one ... ok', 'test module::one ... FAILED'),
    output.replace('0 ignored', '1 ignored'), output.replace('0 measured', '1 measured'),
    output.replace('2 passed', '0 passed'), output + output,
  ]) assert.throws(() => verifyGroup(broken, expected));
});
test('22 selected identities and UTF-8 logs produce a bounded native-core receipt', t => {
  const { root, invoke } = fixture(t);
  const report = runNative({ root, platform: 'win32', invoke });
  assert.equal(report.nativeCorePassed, true);
  assert.equal(report.verifiedNativeTests, 22);
  assert.equal(report.productionStartupValidated, false);
  assert.equal(report.ordinaryUserProductAcceptance, 'pending');
  assert.equal(report.nativeProcessCompletionConfirmed, true);
  assert.equal(report.groups.length, 5);
  assert.equal(Object.keys(report.sourceHashes).length, 9);
  for (const group of report.groups) {
    assert.match(fs.readFileSync(path.join(root, 'target/probes/windows-product-native', report.runId, group.stderr), 'utf8'), /中文/);
  }
});
test('nonzero native result preserves all group evidence without success', t => {
  const { root, invoke } = fixture(t);
  let count = 0;
  const report = runNative({ root, platform: 'win32', invoke: (...args) => {
    const result = invoke(...args);
    return count++ === 0 ? { ...result, status: 101 } : result;
  } });
  assert.equal(report.nativeCorePassed, false);
  assert.equal(report.verifiedNativeTests, 15);
  assert.equal(report.groups.length, 5);
  assert.equal(report.groups[0].exitCode, 101);
  assert.match(report.groups[0].error, /101/);
});
test('an unsupported run replaces stale latest success and preserves the old run', t => {
  const { root, invoke } = fixture(t);
  const first = runNative({ root, platform: 'win32', invoke });
  const next = runNative({ root, platform: 'linux', invoke: () => assert.fail('must not execute') });
  const latest = JSON.parse(fs.readFileSync(path.join(root, 'target/probes/windows-product-native.json')));
  assert.equal(first.nativeCorePassed, true);
  assert.equal(latest.runId, next.runId);
  assert.equal(latest.nativeCorePassed, false);
  assert.match(latest.error, /Native Windows/);
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'target/probes/windows-product-native', first.runId, 'summary.json'))).nativeCorePassed, true);
});
for (const mode of ['unknown-exit', 'timeout', 'signal', 'start-throws']) {
  test(`${mode} preserves failure and stops further native work`, t => {
    const { root, invoke } = fixture(t);
    let calls = 0;
    const report = runNative({ root, platform: 'win32', invoke: (...args) => {
      calls++;
      if (mode === 'start-throws') throw new Error('fixed launch fixture');
      const value = invoke(...args);
      if (mode === 'unknown-exit') return { ...value, status: null };
      if (mode === 'signal') return { ...value, status: null, signal: 'SIGTERM' };
      return { ...value, status: null, error: new Error('ETIMEDOUT fixture') };
    } });
    assert.equal(report.nativeCorePassed, false);
    assert.equal(report.nativeProcessCompletionConfirmed, false);
    assert.equal(report.verifiedNativeTests, 0);
    assert.equal(calls, 1);
    assert.ok(report.groups[0].error);
    assert.equal(fs.existsSync(path.join(root, 'target/probes/windows-product-native', report.runId, 'summary.json')), true);
  });
}
test('real source coverage drift fails before cargo starts', t => {
  const { root } = fixture(t);
  fs.appendFileSync(path.join(root, groups[0].file), '\n#[test]\nfn unexpected_native_case() {}\n');
  const report = runNative({ root, platform: 'win32', invoke: () => assert.fail('must not execute') });
  assert.equal(report.nativeCorePassed, false);
  assert.match(report.error, /coverage change/);
});
