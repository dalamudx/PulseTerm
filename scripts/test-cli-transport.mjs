#!/usr/bin/env node
// Native component test only; authenticated transport never grants GUI or execution authority.
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

import { nativeTestsPassed } from './verify-native-tests.mjs';
const UNIX_PROTOCOL_TESTS = [
  'actual_private_auth_known_action_and_unknown_args_denied',
  'preauth_bad_secret_duplicates_and_oversized_frames_never_dispatch',
  'occupied_endpoint_never_rewrites_token',
  'eof_cancels_but_native_credit_stays_until_actual_exit',
  'unsafe_directory_and_symlink_token_are_not_repaired',
  'cancelled_shutdown_observer_does_not_release_native_operation_or_socket',
  'gui_response_budget_does_not_extend_other_commands_or_wire_deadline',
  'actual_socket_gui_commands_wait_past_wire_deadline',
  'cold_bootstrap_uses_bounded_confirmation_response_not_handshake_budget',
  'continuous_client_calls_are_independent_and_denial_closes_transport',
  'continuous_client_cannot_follow_replaced_server_instance',
].map(name => `cli_control::tests::${name}`);
export const PROTOCOL_TESTS = {
  linux: UNIX_PROTOCOL_TESTS,
  darwin: UNIX_PROTOCOL_TESTS,
  win32: [
    'authenticated_native_pipe_calls_once_and_shutdown_releases_namespace',
    'wrong_token_cannot_reach_handler_and_stale_token_is_never_repaired',
    'cancelled_shutdown_waiter_keeps_native_operation_and_token_until_completion',
  ].map(name => `windows_cli::tests::${name}`),
};
export function transportPassed(result, platform) {
  return nativeTestsPassed(result, PROTOCOL_TESTS[platform]);
}

export const COLD_PLATFORM_CASES = [
  'fixed_sibling_is_pinned_until_lease_release',
  'missing_directory_and_hardlinked_candidates_are_rejected',
  'read_only_image_remains_admissible',
  'endpoint_observation_does_not_create_or_repair_missing_and_partial_namespaces',
].map(name => `windows::cold_cli::tests::${name}`);
export function coldPlatformPassed(result) {
  return nativeTestsPassed(result, COLD_PLATFORM_CASES);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (!['linux', 'darwin', 'win32'].includes(process.platform)) {
    console.error('cli.transport.unsupported_platform');
    process.exitCode = 2;
  } else {
    const scope = process.platform === 'win32'
      ? ['-p', 'xtask', '--features', 'windows-cli-probe', '--bin', 'xtask', 'windows_cli::']
      : ['-p', 'pulse-app', '--features', 'cli-control', '--lib', 'cli_control::'];
    const run = args => {
      const result = spawnSync('cargo', ['test', '--locked', ...args], {
        encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 600_000,
      });
      process.stdout.write(result.stdout ?? '');
      process.stderr.write(result.stderr ?? '');
      return result;
    };
    const result = run(scope);
    const protocolPassed = transportPassed(result, process.platform);
    const coldPassed = process.platform !== 'win32' || (protocolPassed
      && coldPlatformPassed(run(['-p', 'pulse-platform', '--lib', 'windows::cold_cli::tests::'])));
    const passed = protocolPassed && coldPassed;
    console.log(JSON.stringify({ component: 'cli-transport', platform: process.platform,
      expected: process.platform === 'win32' ? 3 : 11, passed,
      nativeColdPlatformCases: process.platform === 'win32' ? COLD_PLATFORM_CASES.length : 0,
      nativeExecution: true, guiValidated: false, coldStartValidated: false }));
    if (!passed) process.exitCode = 1;
  }
}
