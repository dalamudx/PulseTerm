#!/usr/bin/env node
// Native component evidence only. Reading metadata never authorizes SSH or imports a host.
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { nativeTestsPassed } from './verify-native-tests.mjs';
export const expectedTests = [
  'bounded_discovery_preserves_context_without_reading_identity_hints',
  'foreign_reader_query_and_changed_sources_are_rejected',
  'directory_and_file_replacements_are_rejected',
  'windows_aliases_hardlinks_and_excess_budgets_are_rejected',
].map(name => `windows_config::tests::${name}`);
export function nativeSourcesPassed(result, platform) {
  return platform === 'win32' && nativeTestsPassed(result, expectedTests);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.platform !== 'win32') {
    console.error('ssh.config.native_windows_required');
    process.exitCode = 2;
  } else {
    const result = spawnSync('cargo', ['test', '--locked', '-p', 'xtask', '--features', 'windows-config-probe', '--bin', 'xtask', 'windows_config::', '--', '--test-threads=1'], {
      encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 600_000,
    });
    process.stdout.write(result.stdout ?? '');
    process.stderr.write(result.stderr ?? '');
    const passed = nativeSourcesPassed(result, process.platform);
    console.log(JSON.stringify({ component: 'windows-ssh-config-source', expectedTests, passed, nativeExecution: true, guiValidated: false, sshConnectionValidated: false }));
    if (!passed) process.exitCode = 1;
  }
}
