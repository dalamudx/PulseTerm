import assert from 'node:assert/strict';
import test from 'node:test';
import { transportPassed, coldPlatformPassed, COLD_PLATFORM_CASES, PROTOCOL_TESTS } from './test-cli-transport.mjs';
const summary = (passed = 11, failed = 0, ignored = 0) => `test result: ok. ${passed} passed; ${failed} failed; ${ignored} ignored; 0 measured; 314 filtered out; finished in 0.01s\n`;
const output = names => names.map(name => `test ${name} ... ok\n`).join('') + summary(names.length);
const valid = { status: 0, signal: null, stdout: output(PROTOCOL_TESTS.linux) };
for (const [platform, names] of Object.entries(PROTOCOL_TESTS)) {
  test(`accept exactly the ${platform} native component scope`, () => {
    assert.equal(transportPassed({ ...valid, stdout: output(names) }, platform), true);
  });
}
for (const [label, patch, platform = 'linux'] of [
  ['zero tests', { stdout: summary(0) }],
  ['partial scope', { stdout: summary(10) }],
  ['ignored native case', { stdout: summary(11, 0, 1) }],
  ['failed native case', { stdout: summary(11, 1, 0) }],
  ['nonzero exit', { status: 101 }],
  ['unknown exit', { status: null }],
  ['signal', { signal: 'SIGTERM' }],
  ['launch error', { error: new Error('fixture.spawn') }],
  ['missing summary', { stdout: '' }],
  ['multiple summaries', { stdout: summary() + summary() }],
  ['unsupported platform', {}, 'freebsd'],
]) {
  test(`reject ${label}`, () => assert.equal(transportPassed({ ...valid, ...patch }, platform), false));
}


const coldStdout = COLD_PLATFORM_CASES.map(name => `test ${name} ... ok\n`).join('') + summary(4);
const coldValid = { ...valid, stdout: coldStdout };
test('accept all exact cold platform names including CRLF', () => {
  assert.equal(coldPlatformPassed(coldValid), true);
  assert.equal(coldPlatformPassed({ ...coldValid, stdout: coldStdout.replaceAll('\n', '\r\n') }), true);
});
for (const [label, patch] of [
  ['same count wrong name', { stdout: coldStdout.replace(COLD_PLATFORM_CASES[0], 'fixture.wrong') }],
  ['duplicate name', { stdout: coldStdout.replace(COLD_PLATFORM_CASES[0], COLD_PLATFORM_CASES[1]) }],
  ['zero tests', { stdout: summary(0) }],
  ['partial scope', { stdout: coldStdout.replace(summary(4), summary(3)) }],
  ['ignored case', { stdout: coldStdout.replace(summary(4), summary(4, 0, 1)) }],
  ['failure', { status: 101 }],
  ['unknown exit', { status: null }],
  ['timeout', { error: new Error('fixture.timeout') }],
  ['multiple summaries', { stdout: coldStdout + summary(4) }],
]) {
  test(`reject cold platform ${label}`, () => assert.equal(coldPlatformPassed({ ...coldValid, ...patch }), false));
}

for (const [platform, names] of Object.entries(PROTOCOL_TESTS)) {
  const stdout = output(names);
  test(`accept ${platform} CRLF identities`, () => {
    assert.equal(transportPassed({ ...valid, stdout: stdout.replaceAll('\n', '\r\n') }, platform), true);
  });
  for (const [label, text] of [
    ['same count wrong identity', stdout.replace(names[0], 'fixture::unrelated')],
    ['duplicate identity', stdout.replace(names[0], names[1])],
    ['missing test row', stdout.slice(stdout.indexOf('\n') + 1)],
    ['summary without test rows', summary(names.length)],
    ['ignored row', stdout.replace('... ok', '... ignored')],
    ['measured result', stdout.replace('0 measured', '1 measured')],
  ]) {
    test(`reject ${platform} ${label}`, () => assert.equal(transportPassed({ ...valid, stdout: text }, platform), false));
  }
}
